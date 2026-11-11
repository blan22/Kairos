//! 노드 하나의 실패가 그 노드를 가진 질의에만 머무는지, 읽지 못한 노드가 질의를 영영 멈추지 않는지 본다.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kairos_core::Instant;
use kairos_core::model::{CollectContext, CursorState, DirectSource, Outcome, SourceResult};
use kairos_core::ports::{Clock, FixedClock};
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest};
use serde_json::{Map, Value, json};

use super::fake::{FakeGh, FakeOptions, NOW, by_id, clones, config, context, cursor, ids, now, query, source, spread};
use crate::GithubSource;
use crate::query::EXPANDED_QUERY;

fn collect(fake: &Arc<FakeGh>, context: &CollectContext) -> Vec<SourceResult> {
    source(fake.clone()).collect(context)
}

fn is_expanded(request: &ProcessRequest) -> bool {
    let body: Value = serde_json::from_str(request.stdin.as_deref().unwrap_or("{}")).unwrap_or_default();
    body["query"].as_str() == Some(EXPANDED_QUERY.as_str())
}

fn too_large() -> ProcessOutput {
    ProcessOutput::ended(ProcessEnd::OutputTooLarge)
}

fn node_map(id: &str, node: Value) -> Map<String, Value> {
    Map::from_iter([(id.to_owned(), node)])
}

fn external_ids(result: &SourceResult) -> Vec<&str> {
    result.objects.iter().map(|object| object.external_id.as_str()).collect()
}

fn mentions_at(watermark: &str) -> (&'static str, CursorState) {
    ("mentions", cursor(watermark, Some("2026-03-01T00:00:00.000Z")))
}

fn cursor_of<'a>(id: &'a str, watermark: &str) -> (&'a str, CursorState) {
    (id, CursorState { query: id.to_owned(), ..cursor(watermark, Some("2026-03-01T00:00:00.000Z")) })
}

// NEEDS-VALIDATION 5의 확인 계획. 고치기 전에는 review_requested까지 parse_error로 아무것도 남기지 못했다.
#[test]
fn 멘션_노드의_넓은_읽기가_넘쳐도_review_requested는_ok다() {
    let cut = json!({ "reviewThreads": { "pageInfo": { "hasPreviousPage": true }, "nodes": [] } });
    let fake = FakeGh::new(FakeOptions {
        search: vec![("review-requested:", ids(&["PR_synth_b"])), ("mentions:", ids(&["PR_synth_a"]))],
        overrides: node_map("PR_synth_a", spread(&by_id("PR_synth_a"), cut)),
        hook: Some(Box::new(|request| {
            let hit =
                is_expanded(request) && request.stdin.as_deref().is_some_and(|input| input.contains("PR_synth_a"));
            hit.then(too_large)
        })),
        ..FakeOptions::default()
    });
    let results = collect(&fake, &context(vec![mentions_at("2026-03-02T00:00:00.000Z")]));

    let review = query(&results, "review_requested");
    assert_eq!((review.outcome, review.error_code.as_deref()), (Outcome::Ok, None));
    assert_eq!(review.watermark.as_deref(), Some(NOW));
    assert_eq!(external_ids(review), ["example-org/widget#12"]);
    assert!(review.snapshot);

    // 넓게 읽지 못한 노드는 처음 읽은 만큼 쓰고 넘긴다. 혼자 읽어도 넘쳐 다시 읽어도 같다.
    let mentions = query(&results, "mentions");
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
    assert_eq!(mentions.watermark.as_deref(), Some(NOW));
    assert_eq!(external_ids(mentions), ["example-org/widget#11"]);
    assert!(results.iter().filter(|result| result.query != "mentions").all(|result| result.error_code.is_none()));
}

#[test]
fn graphql_오류가_한_노드를_가리키면_그_노드를_가진_질의만_partial이다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("review-requested:", ids(&["PR_synth_b"])), ("mentions:", ids(&["I_synth_c"]))],
        errors: vec![json!({ "type": "FORBIDDEN", "path": ["nodes", 1, "comments"] })],
        ..FakeOptions::default()
    });
    let results = collect(&fake, &context(Vec::new()));
    let mentions = query(&results, "mentions");
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")));
    assert_eq!(mentions.watermark, None);
    assert_eq!(external_ids(mentions), ["example-org/widget#13"], "오류가 붙어도 읽은 객체는 저장한다");
    for id in ["review_requested", "assigned", "authored", "closed"] {
        let result = query(&results, id);
        assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Ok, None), "{id}");
    }
}

#[test]
fn 어느_노드인지_모르는_graphql_오류는_그_묶음의_질의만_partial이다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("review-requested:", ids(&["PR_synth_b"])), ("mentions:", ids(&["I_synth_c"]))],
        errors: vec![json!({ "type": "FORBIDDEN" })],
        ..FakeOptions::default()
    });
    let results = collect(&fake, &context(Vec::new()));
    assert_eq!(query(&results, "review_requested").error_code.as_deref(), Some("node_unreadable"));
    assert_eq!(query(&results, "mentions").error_code.as_deref(), Some("node_unreadable"));
    for id in ["assigned", "authored", "closed"] {
        assert_eq!(query(&results, id).outcome, Outcome::Ok, "후보가 없는 질의는 무르지 않는다: {id}");
    }
}

#[test]
fn 실패한_묶음은_나눠_읽어_나쁜_노드만_남긴다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("author:", ids(&["PR_synth_a", "PR_synth_b"])), ("mentions:", ids(&["I_synth_c"]))],
        hook: Some(Box::new(|request| {
            request.stdin.as_deref().is_some_and(|input| input.contains("I_synth_c")).then(too_large)
        })),
        ..FakeOptions::default()
    });
    let results = collect(&fake, &context(Vec::new()));
    let authored = query(&results, "authored");
    assert_eq!((authored.outcome, authored.error_code.as_deref()), (Outcome::Ok, None));
    assert_eq!(external_ids(authored), ["example-org/widget#11", "example-org/widget#12"]);
    let mentions = query(&results, "mentions");
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
    let mut reads: Vec<Vec<String>> =
        fake.calls().iter().filter(|call| call.reads_nodes()).map(|call| call.node_ids()).collect();
    reads[1..].sort();
    assert_eq!(
        reads,
        [
            ids(&["PR_synth_a", "PR_synth_b", "I_synth_c"]),
            ids(&["I_synth_c"]),
            ids(&["PR_synth_a"]),
            ids(&["PR_synth_b"])
        ]
    );
}

fn synthetic(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("N{index}")).collect()
}

// 최악: 후보 60개가 모두 넘친다. 처음 3번에 나눠 읽기 24번을 넘지 않는다(20개 묶음 셋이 8조각씩 나뉘면 상한이다).
#[test]
fn 나눠_읽기는_한_수집에_24번을_넘지_않는다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("author:", synthetic(60))],
        hook: Some(Box::new(|request| {
            request.stdin.as_deref().is_some_and(|input| input.contains("\"N")).then(too_large)
        })),
        ..FakeOptions::default()
    });
    let results = collect(&fake, &context(Vec::new()));
    assert_eq!(fake.calls().iter().filter(|call| call.reads_nodes()).count(), 3 + 24);
    let authored = query(&results, "authored");
    assert_eq!((authored.outcome, authored.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")));
    assert_eq!(query(&results, "mentions").outcome, Outcome::Ok);
}

fn late_source(fake: &Arc<FakeGh>, elapsed_ms: i64) -> GithubSource {
    GithubSource::new(fake.clone(), Arc::new(FixedClock(now().plus_ms(elapsed_ms))), config())
}

#[test]
fn 노드_읽기_호출은_소스_상한_전의_남은_시간만_받는다() {
    let fake = FakeGh::new(FakeOptions { search: vec![("author:", ids(&["PR_synth_a"]))], ..FakeOptions::default() });
    let results = late_source(&fake, 10_000).collect(&context(Vec::new()));
    assert_eq!(query(&results, "authored").outcome, Outcome::Ok);
    let reads: Vec<Duration> = fake.calls().iter().filter(|call| call.reads_nodes()).map(|call| call.timeout).collect();
    assert_eq!(reads, [Duration::from_secs(8)]);
}

#[test]
fn 남은_시간이_없으면_부르지_않고_후보가_있던_질의만_partial이다() {
    let fake = FakeGh::new(FakeOptions { search: vec![("author:", ids(&["PR_synth_a"]))], ..FakeOptions::default() });
    let results = late_source(&fake, 19_000).collect(&context(Vec::new()));
    assert!(!fake.calls().iter().any(|call| call.reads_nodes()));
    assert_eq!(query(&results, "authored").error_code.as_deref(), Some("node_unreadable"));
    assert_eq!(query(&results, "mentions").outcome, Outcome::Ok);
}

#[test]
fn 다시_읽으면_읽힐_노드는_다른_질의가_전진하는_동안_30분까지만_다시_읽는다() {
    let options = || FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        null_ids: vec!["I_synth_c"],
        ..FakeOptions::default()
    };
    let mentions = |authored: &str| {
        let context = context(vec![mentions_at("2026-03-02T00:00:00.000Z"), cursor_of("authored", authored)]);
        query(&collect(&FakeGh::new(options()), &context), "mentions").clone()
    };
    let fresh = mentions("2026-03-02T00:20:00.000Z");
    assert_eq!((fresh.outcome, fresh.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")));
    assert_eq!(fresh.watermark, None);
    let stalled = mentions("2026-03-02T00:30:00.000Z");
    assert_eq!((stalled.outcome, stalled.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
    assert_eq!(stalled.watermark.as_deref(), Some(NOW));
}

// 밤사이처럼 모든 질의가 함께 멈춘 시간은 세지 않는다. 아침 첫 틱의 일시 실패를 넘기지 않는다.
#[test]
fn 모든_질의가_함께_쉬었으면_오래된_watermark만으로_넘기지_않는다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        null_ids: vec!["I_synth_c"],
        ..FakeOptions::default()
    });
    let yesterday = "2026-03-01T10:00:00.000Z";
    let context = context(vec![mentions_at(yesterday), cursor_of("authored", yesterday)]);
    let mentions = query(&collect(&fake, &context), "mentions").clone();
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")));
}

#[test]
fn 읽지_못한_후보가_있는_스냅샷_질의는_ok여도_스냅샷이라_하지_않는다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("author:", ids(&["PR_synth_a", "PR_synth_b"]))],
        hook: Some(Box::new(|request| {
            request.stdin.as_deref().unwrap_or_default().contains("PR_synth_a").then(too_large)
        })),
        ..FakeOptions::default()
    });
    let authored = query(&collect(&fake, &context(Vec::new())), "authored").clone();
    assert_eq!((authored.outcome, authored.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
    assert_eq!(external_ids(&authored), ["example-org/widget#12"]);
    assert!(!authored.snapshot, "빠진 a가 ok 스냅샷의 부재로 세지면 안 된다");
}

fn reads_node(request: &ProcessRequest, id: &str) -> bool {
    request.args.get(1).is_some_and(|arg| arg == "graphql")
        && request.stdin.as_deref().is_some_and(|input| input.contains(&format!("\"{id}\"")))
}

fn timed_out() -> ProcessOutput {
    ProcessOutput::ended(ProcessEnd::TimedOut)
}

// 2차 한도는 노드와 무관하다. 기준선처럼 읽기 전체가 실패하고 다음 틱에 다시 읽는다. 넘기거나 나눠 읽지 않는다.
#[test]
fn 노드_읽기가_2차_한도에_걸리면_넘기지_않고_후보가_있는_질의를_rate_limited로_낸다() {
    let fake = FakeGh::new(FakeOptions {
        search: vec![("author:", ids(&["PR_synth_a", "PR_synth_b"])), ("mentions:", ids(&["I_synth_c"]))],
        hook: Some(Box::new(|request| {
            reads_node(request, "PR_synth_a").then(|| ProcessOutput {
                end: ProcessEnd::Exited(1),
                stdout: r#"{"message":"You have exceeded a secondary rate limit"}"#.to_owned(),
                stderr: "gh: You have exceeded a secondary rate limit (HTTP 403)".to_owned(),
            })
        })),
        ..FakeOptions::default()
    });
    let results = collect(&fake, &context(Vec::new()));
    for id in ["authored", "mentions"] {
        let result = query(&results, id);
        assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::RateLimited, Some("gh_rate_limited")));
        assert_eq!(result.watermark, None);
    }
    assert!(results.iter().all(|result| result.error_code.as_deref() != Some("node_quarantined")));
    assert_eq!(fake.calls().iter().filter(|call| call.reads_nodes()).count(), 1, "한도에 걸리면 나눠 읽지 않는다");
}

// 5xx의 HTML과 GitHub 쪽 시간 초과(data:null)는 혼자 읽어도 일시 실패다. 넘기지 않고 다음 틱에 다시 읽는다.
#[test]
fn 혼자_읽어도_일시_실패인_노드는_넘기지_않고_partial이다() {
    let replies: [fn() -> ProcessOutput; 3] = [
        || ProcessOutput {
            end: ProcessEnd::Exited(1),
            stdout: "<html>502 Bad Gateway</html>".to_owned(),
            stderr: "gh: HTTP 502".to_owned(),
        },
        || ProcessOutput::exited(0, r#"{"data":null,"errors":[{"message":"timeout"}]}"#),
        || ProcessOutput::exited(0, "<html></html>"),
    ];
    for reply in replies {
        let fake = FakeGh::new(FakeOptions {
            search: vec![("author:", ids(&["PR_synth_a", "PR_synth_b"]))],
            hook: Some(Box::new(move |request| reads_node(request, "PR_synth_a").then(reply))),
            ..FakeOptions::default()
        });
        let authored = query(&collect(&fake, &context(Vec::new())), "authored").clone();
        assert_eq!((authored.outcome, authored.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")));
        assert_eq!(external_ids(&authored), ["example-org/widget#12"], "나눠 읽어 b는 저장한다");
    }
}

// 섹션 5가 꼽은 느린 실패. 20개 묶음이 시간 초과로 끝나도 같은 수집 안에서 나눠 읽어 무고한 노드 19개를 읽는다.
#[test]
fn 시간_초과된_묶음도_나눠_읽어_느린_노드를_가진_질의만_partial이다() {
    let (clone_ids, overrides) = clones(16);
    let fake = FakeGh::new(FakeOptions {
        search: vec![
            ("review-requested:", ids(&["I_slow", "PR_synth_b"])),
            ("assignee:", ids(&["I_synth_c"])),
            ("author:", [ids(&["PR_synth_a"]), clone_ids].concat()),
            ("mentions:", ids(&["I_slow"])),
        ],
        overrides,
        hook: Some(Box::new(|request| reads_node(request, "I_slow").then(timed_out))),
        ..FakeOptions::default()
    });
    let results = collect(&fake, &context(Vec::new()));

    for id in ["review_requested", "mentions"] {
        let result = query(&results, id);
        assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")), "{id}");
    }
    assert_eq!(external_ids(query(&results, "review_requested")), ["example-org/widget#12"]);
    for id in ["assigned", "authored", "closed"] {
        let result = query(&results, id);
        assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Ok, None), "{id}");
    }
    assert_eq!(external_ids(query(&results, "assigned")), ["example-org/widget#13"]);
    assert_eq!(query(&results, "authored").objects.len(), 17);

    // 20개 한 묶음 -> 3개씩 7조각 -> 느린 조각을 하나씩. 나눌 수 있는 호출은 남은 시간의 절반만 받는다.
    let mut reads: Vec<(usize, Duration)> = fake
        .calls()
        .iter()
        .filter(|call| call.reads_nodes())
        .map(|call| (call.node_ids().len(), call.timeout))
        .collect();
    reads.sort();
    let (nine, fifteen) = (Duration::from_secs(9), Duration::from_secs(15));
    let expected = [vec![(1, fifteen); 3], vec![(2, nine)], vec![(3, nine); 6], vec![(20, nine)]].concat();
    assert_eq!(reads, expected);
}

/// 시간 초과로 끝난 호출만 그 제한만큼 시각을 옮기는 시계. 성공한 호출은 시간이 들지 않는다.
struct VirtualClock(Mutex<Instant>);

impl Clock for VirtualClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

// 단계마다 느린 조각 하나만 시간 초과다. 검색에 2초를 쓴 뒤라도 첫 묶음이 절반만 받아 같은 수집 안에서 느린 노드까지 좁힌다.
// 모든 호출이 노드 읽기 상한(18초) 안에 끝나 소스 상한(20초)에 걸리지 않는다.
#[test]
fn 느린_노드는_노드_읽기_상한_안에서_하나로_좁혀진다() {
    let clock = Arc::new(VirtualClock(Mutex::new(now().plus_ms(2_000))));
    let ticking = clock.clone();
    let (clone_ids, overrides) = clones(18);
    let fake = FakeGh::new(FakeOptions {
        search: vec![("author:", [ids(&["I_slow", "PR_synth_a"]), clone_ids].concat())],
        overrides,
        hook: Some(Box::new(move |request| {
            reads_node(request, "I_slow").then(|| {
                let mut at = ticking.0.lock().unwrap();
                *at = at.plus_ms(i64::try_from(request.timeout.as_millis()).unwrap());
                timed_out()
            })
        })),
        ..FakeOptions::default()
    });
    let results = GithubSource::new(fake.clone(), clock.clone(), config()).collect(&context(Vec::new()));
    let authored = query(&results, "authored");
    assert_eq!((authored.outcome, authored.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")));
    assert_eq!(authored.objects.len(), 19, "느린 노드 말고는 모두 읽었다");
    let slow: Vec<(usize, Duration)> = fake
        .calls()
        .iter()
        .filter(|call| call.node_ids().contains(&"I_slow".to_owned()))
        .map(|call| (call.node_ids().len(), call.timeout))
        .collect();
    let secs = Duration::from_secs;
    assert_eq!(slow, [(20, secs(8)), (3, secs(4)), (1, secs(4))]);
    assert!(clock.now() <= now().plus_ms(crate::node_reader::READ_LIMIT_MS));
}
