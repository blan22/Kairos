//! `Promise.all`을 스레드로 옮긴 부분의 시험. 원본에 없다.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use kairos_core::model::{DirectSource, Outcome};
use kairos_core::ports::CancelToken;
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest};
use serde_json::{Map, Value, json};

use super::fake::{FakeGh, FakeOptions, context, query, search_table, source};
use crate::error::GhError;
use crate::parallel::try_join;

fn synthetic_node(index: usize) -> Value {
    json!({
        "__typename": "Issue", "id": format!("N{index}"), "number": index, "title": "합성", "state": "OPEN",
        "repository": { "nameWithOwner": "example-org/widget" },
        "author": { "__typename": "User", "login": "synthetic-teammate" },
        "comments": { "nodes": [
            { "databaseId": index * 2, "createdAt": "2026-03-01T00:00:00Z", "bodyText": "합성" },
            { "databaseId": index * 2 + 1, "createdAt": "2026-03-01T00:00:01Z", "bodyText": "합성" },
        ] },
    })
}

fn synthetic(count: usize) -> (Vec<String>, Map<String, Value>) {
    let ids: Vec<String> = (0..count).map(|index| format!("N{index}")).collect();
    let nodes = (0..count).map(|index| (format!("N{index}"), synthetic_node(index))).collect();
    (ids, nodes)
}

fn first_requested_index(request: &ProcessRequest) -> Option<usize> {
    let body: Value = serde_json::from_str(request.stdin.as_deref()?).ok()?;
    body["variables"]["ids"][0].as_str()?.strip_prefix('N')?.parse().ok()
}

#[test]
fn 앞_묶음의_응답이_늦어도_객체와_이벤트는_후보_순서다() {
    let (ids, overrides) = synthetic(60);
    let fake = FakeGh::new(FakeOptions {
        search: vec![("author:", ids)],
        overrides,
        hook: Some(Box::new(|request| {
            // 앞 묶음일수록 늦게 끝난다. 완료 순서로 모으면 순서가 뒤집힌다.
            let delay = match first_requested_index(request) {
                Some(0) => 60,
                Some(20) => 30,
                _ => 0,
            };
            thread::sleep(Duration::from_millis(delay));
            None
        })),
        ..FakeOptions::default()
    });
    let results = source(fake).collect(&context(Vec::new()));
    let authored = query(&results, "authored");
    assert_eq!(authored.outcome, Outcome::Ok);
    let objects: Vec<&str> = authored.objects.iter().map(|object| object.external_id.as_str()).collect();
    let expected: Vec<String> = (0..60).map(|index| format!("example-org/widget#{index}")).collect();
    assert_eq!(objects, expected);
    let events: Vec<&str> = authored.events.iter().map(|event| event.dedupe_key.as_str()).collect();
    let expected: Vec<String> = (0..120).map(|index| format!("gh:comment:{index}")).collect();
    assert_eq!(events, expected);
}

#[test]
fn 결과는_질의_우선순위_순서다() {
    let fake = FakeGh::new(FakeOptions {
        search: search_table(),
        hook: Some(Box::new(|request| {
            // 앞 질의일수록 늦게 끝난다.
            let q = request.args.get(5).map_or("", String::as_str);
            let delay = if q.contains("review-requested") {
                40
            } else if q.contains("assignee") {
                20
            } else {
                0
            };
            thread::sleep(Duration::from_millis(delay));
            None
        })),
        ..FakeOptions::default()
    });
    let results = source(fake).collect(&context(Vec::new()));
    let order: Vec<&str> = results.iter().map(|result| result.query.as_str()).collect();
    assert_eq!(order, ["review_requested", "assigned", "authored", "mentions", "closed"]);
}

#[test]
fn 노드_읽기의_동시_실행은_8개를_넘지_않는다() {
    let (ids, overrides) = synthetic(200);
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let (now, top) = (running.clone(), peak.clone());
    let fake = FakeGh::new(FakeOptions {
        search: vec![("author:", ids)],
        overrides,
        hook: Some(Box::new(move |request| {
            if first_requested_index(request).is_some() {
                top.fetch_max(now.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                // 묶음은 8개와 2개다. 둘 이상 겹칠 때까지 기다려 기계가 바빠도 동시 실행을 본다.
                while top.load(Ordering::SeqCst) < 2 {
                    thread::sleep(Duration::from_millis(1));
                }
                thread::sleep(Duration::from_millis(30));
                now.fetch_sub(1, Ordering::SeqCst);
            }
            None
        })),
        ..FakeOptions::default()
    });
    let results = source(fake.clone()).collect(&context(Vec::new()));
    assert_eq!(query(&results, "authored").objects.len(), 200);
    let peak = peak.load(Ordering::SeqCst);
    assert!((2..=8).contains(&peak), "{peak}");
}

#[test]
fn 모든_프로세스_호출에_취소_토큰이_실린다() {
    let fake = FakeGh::new(FakeOptions { search: search_table(), ..FakeOptions::default() });
    source(fake.clone()).collect(&context(Vec::new()));
    let calls = fake.calls();
    assert!(calls.len() > 8);
    assert!(calls.iter().all(|call| call.has_cancel));
}

#[test]
fn 자식_환경은_원본의_키_목록과_gh_token뿐이다() {
    let fake = FakeGh::new(FakeOptions { search: search_table(), ..FakeOptions::default() });
    source(fake.clone()).collect(&context(Vec::new()));
    let base = ["GH_HOST", "GH_NO_UPDATE_NOTIFIER", "GH_PROMPT_DISABLED", "HOME", "LANG", "NO_COLOR", "PATH"];
    let with_token =
        ["GH_HOST", "GH_NO_UPDATE_NOTIFIER", "GH_PROMPT_DISABLED", "GH_TOKEN", "HOME", "LANG", "NO_COLOR", "PATH"];
    for (index, call) in fake.calls().iter().enumerate() {
        let keys: Vec<&str> = call.env.keys().map(String::as_str).collect();
        assert_eq!(keys, if index == 0 { &base[..] } else { &with_token[..] });
        assert_eq!(call.env["PATH"], "/synthetic/bin");
        assert_eq!(call.env["GH_HOST"], "github.com");
        assert_eq!(call.program, "gh");
    }
}

#[test]
fn 토큰은_argv_입력_결과_어디에도_없다() {
    let fake = FakeGh::new(FakeOptions { search: search_table(), ..FakeOptions::default() });
    let results = source(fake.clone()).collect(&context(Vec::new()));
    for call in fake.calls() {
        assert!(!call.args.iter().any(|arg| arg.contains("synthetic-token")));
        assert!(!call.stdin.unwrap_or_default().contains("synthetic-token"));
    }
    assert!(!format!("{results:?}").contains("synthetic-token"));
}

#[test]
fn 이미_취소됐으면_프로세스_결과와_상관없이_gh_timeout이다() {
    // 원본은 프로세스를 띄운 뒤 곧바로 죽이고 aborted로 적는다. 빨리 끝난 성공도 없던 일이다.
    let fake = FakeGh::new(FakeOptions { search: search_table(), ..FakeOptions::default() });
    let context = context(Vec::new());
    context.cancel.cancel();
    let results = source(fake.clone()).collect(&context);
    for result in &results {
        assert_eq!(result.outcome, Outcome::Timeout);
        assert_eq!(result.error_code.as_deref(), Some("gh_timeout"));
        assert_eq!(result.identity, None);
    }
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn 이미_취소됐으면_gh가_없어도_gh_timeout이다() {
    let context = context(Vec::new());
    context.cancel.cancel();
    let missing = super::fake::FnRunner(|_: &_| ProcessOutput::ended(ProcessEnd::Missing));
    let results = source(Arc::new(missing)).collect(&context);
    assert!(results.iter().all(|result| result.outcome == Outcome::Timeout));
}

/// 진짜 러너처럼 토큰이 켜진 뒤의 호출을 Cancelled로 끝낸다.
fn cancelled_after(needle: &'static str) -> FakeOptions {
    FakeOptions {
        search: search_table(),
        hook: Some(Box::new(move |request| {
            let cancel = request.cancel.as_ref().unwrap();
            let hit = request.args.iter().chain(request.stdin.iter()).any(|text| text.contains(needle));
            if hit {
                cancel.cancel();
            }
            cancel.is_cancelled().then(|| ProcessOutput::ended(ProcessEnd::Cancelled))
        })),
        ..FakeOptions::default()
    }
}

#[test]
fn 검색_중에_취소되면_그_질의는_gh_timeout이고_신원은_남는다() {
    let results = source(FakeGh::new(cancelled_after("search/issues"))).collect(&context(Vec::new()));
    for result in &results {
        assert_eq!(result.outcome, Outcome::Timeout);
        assert_eq!(result.error_code.as_deref(), Some("gh_timeout"));
        assert_eq!(result.identity.as_deref(), Some("kairos-dev"));
    }
}

#[test]
fn 노드_읽기_중에_취소되면_후보가_있던_질의는_gh_timeout이다() {
    let results = source(FakeGh::new(cancelled_after("PR_synth_a"))).collect(&context(Vec::new()));
    assert!(results.iter().all(|result| result.outcome == Outcome::Timeout));
    assert!(results.iter().all(|result| result.watermark.is_none()));
}

#[test]
fn 실패는_입력_순서가_아니라_시간상_먼저_난_것을_낸다() {
    let cancel = CancelToken::new();
    let slow = GhError::new(Outcome::Timeout, "gh_timeout");
    let fast = GhError::new(Outcome::RateLimited, "gh_rate_limited");
    let fast_failed = AtomicBool::new(false);
    // 앞 일은 뒤 일이 실패한 것을 본 뒤에야 실패한다. 기계가 바빠도 시간 순서가 뒤집히지 않는다.
    let fail_slowly = || -> Result<(), GhError> {
        while !fast_failed.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(1));
        }
        thread::sleep(Duration::from_millis(100));
        Err(slow.clone())
    };
    let fail_fast = || -> Result<(), GhError> {
        fast_failed.store(true, Ordering::SeqCst);
        Err(fast.clone())
    };
    assert_eq!(try_join(fail_slowly, fail_fast, &cancel), Err(fast));
}

#[test]
fn 패닉한_일은_gh_unexpected이고_취소_뒤라면_gh_timeout이다() {
    let cancel = CancelToken::new();
    let panics = || -> Result<(), GhError> { panic!("시험용 패닉") };
    assert_eq!(try_join(panics, || Ok(()), &cancel).unwrap_err().code(), "gh_unexpected");
    cancel.cancel();
    assert_eq!(try_join(panics, || Ok(()), &cancel).unwrap_err().code(), "gh_timeout");
}
