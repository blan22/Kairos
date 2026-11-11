//! 여러 틱을 원장에 저장하며 넘긴 노드가 커서를 풀고, 읽지 못한 객체를 빠진 것이나 닫힌 것으로 만들지 않는지 본다.
//! 메모리 DB와 합성 응답만 쓴다.

use std::sync::Arc;

use kairos_core::Instant;
use kairos_core::model::{CollectContext, DirectSource, Outcome, SourceName, SourceResult};
use kairos_core::ports::{CancelToken, FixedClock};
use kairos_core::process::{ProcessEnd, ProcessOutput};
use kairos_ledger::{CursorRepo, IngestOptions, Ledger, ObjectRepo, SourceRunRepo};
use kairos_store::{Database, OptionalExtension, params};
use serde_json::{Map, Value, json};

use super::fake::{FakeGh, FakeOptions, by_id, config, ids, query, spread};
use crate::GithubSource;
use crate::defs::{DEFS, earliest_window};

fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

fn context_from(db: &Database, now: Instant) -> CollectContext {
    let repo = CursorRepo::new(db.connection());
    let cursors = DEFS.iter().map(|def| (def.id.to_owned(), repo.get(SourceName::Github, def.id).unwrap())).collect();
    CollectContext { now, cursors, thread_reads: None, cancel: CancelToken::new() }
}

/// 틱 한 번: 저장된 커서로 수집하고 결과를 모두 원장에 넣는다.
fn tick(db: &Database, fake: &Arc<FakeGh>, now: &str) -> Vec<SourceResult> {
    let now = at(now);
    let results = GithubSource::new(fake.clone(), Arc::new(FixedClock(now)), config()).collect(&context_from(db, now));
    for result in results.clone() {
        Ledger::new(db).ingest(result, &IngestOptions::at(now)).unwrap();
    }
    results
}

fn watermark(db: &Database, id: &str) -> Option<String> {
    CursorRepo::new(db.connection()).get(SourceName::Github, id).unwrap().and_then(|cursor| cursor.watermark)
}

fn node_map(id: &str, node: Value) -> Map<String, Value> {
    Map::from_iter([(id.to_owned(), node)])
}

fn comment(id: i64, created_at: &str) -> Value {
    json!({
        "databaseId": id, "createdAt": created_at, "bodyText": "합성 댓글",
        "url": format!("https://example.invalid/c/{id}"), "authorAssociation": "MEMBER",
        "author": { "__typename": "User", "login": "synthetic-teammate" },
    })
}

/// 댓글 창의 앞이 잘렸고 보이는 가장 오래된 댓글이 00:40이다.
fn cut_comments() -> Value {
    json!({ "comments": { "pageInfo": { "hasPreviousPage": true }, "nodes": [comment(910, "2026-03-02T00:40:00Z")] } })
}

fn mentions_of_cut_node() -> Arc<FakeGh> {
    FakeGh::new(FakeOptions {
        search: vec![("mentions:", ids(&["I_synth_c"]))],
        overrides: node_map("I_synth_c", spread(&by_id("I_synth_c"), cut_comments())),
        expanded: node_map("I_synth_c", spread(&json!({ "id": "I_synth_c" }), cut_comments())),
        ..FakeOptions::default()
    })
}

// NEEDS-VALIDATION 6의 확인 계획. 고치기 전에는 mentions가 partial로 남아 watermark와 earliest_window가 영영 멈췄다.
#[test]
fn 넓게_읽어도_잘린_노드는_넘기고_다음_틱에는_커서와_잘림_기준이_전진해_풀린다() {
    let db = Database::in_memory().unwrap();
    tick(&db, &FakeGh::new(FakeOptions::default()), "2026-03-02T00:00:00.000Z");

    let fake = mentions_of_cut_node();
    let first = tick(&db, &fake, "2026-03-02T01:00:00.000Z");
    let mentions = query(&first, "mentions");
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
    assert_eq!(fake.calls().iter().filter(|call| call.is_expanded()).count(), 1);
    assert_eq!(watermark(&db, "mentions").as_deref(), Some("2026-03-02T01:00:00.000Z"));
    let run = SourceRunRepo::new(db.connection()).latest(SourceName::Github, "mentions", false).unwrap().unwrap();
    assert_eq!((run.outcome, run.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));

    let later = at("2026-03-02T01:03:00.000Z");
    assert_eq!(earliest_window(&context_from(&db, later)), Some(at("2026-03-02T00:55:00.000Z")));
    let fake = mentions_of_cut_node();
    let second = tick(&db, &fake, "2026-03-02T01:03:00.000Z");
    let mentions = query(&second, "mentions");
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, None));
    assert_eq!(fake.calls().iter().filter(|call| call.is_expanded()).count(), 0, "본 구간의 잘림은 다시 읽지 않는다");
    assert_eq!(watermark(&db, "mentions").as_deref(), Some("2026-03-02T01:03:00.000Z"));
}

#[test]
fn 시각과_무관하게_늘_잘린_노드도_틱마다_커서가_전진한다() {
    let db = Database::in_memory().unwrap();
    tick(&db, &FakeGh::new(FakeOptions::default()), "2026-03-02T00:00:00.000Z");
    let threads = json!({ "reviewThreads": { "pageInfo": { "hasPreviousPage": true }, "nodes": [] } });
    let fake = || {
        FakeGh::new(FakeOptions {
            search: vec![("review-requested:", ids(&["PR_synth_b"]))],
            overrides: node_map("PR_synth_b", spread(&by_id("PR_synth_b"), threads.clone())),
            expanded: node_map("PR_synth_b", spread(&json!({ "id": "PR_synth_b" }), threads.clone())),
            ..FakeOptions::default()
        })
    };
    for now in ["2026-03-02T01:00:00.000Z", "2026-03-02T01:03:00.000Z", "2026-03-02T01:06:00.000Z"] {
        let results = tick(&db, &fake(), now);
        let review = query(&results, "review_requested");
        assert_eq!((review.outcome, review.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")), "{now}");
        assert!(review.snapshot, "읽은 객체가 있으면 스냅샷이다");
        assert_eq!(watermark(&db, "review_requested").as_deref(), Some(now));
    }
}

fn open_event(db: &Database, key: &str) -> Option<(String, Option<String>)> {
    db.connection()
        .query_row("SELECT disposition, closed_at FROM events WHERE dedupe_key = ?", params![key], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()
        .unwrap()
}

// 스냅샷 질의의 ok는 빠진 객체의 missing_runs를 올리고 종료 판정을 돈다. 넘긴 노드는 읽지 못했을 뿐 빠진 것이 아니다.
#[test]
fn 스냅샷_질의에서_넘긴_노드의_객체는_빠지거나_닫히지_않는다() {
    let db = Database::in_memory().unwrap();
    let authored = || vec![("author:", ids(&["PR_synth_a", "PR_synth_b"]))];
    let first = tick(
        &db,
        &FakeGh::new(FakeOptions { search: authored(), ..FakeOptions::default() }),
        "2026-03-02T01:00:00.000Z",
    );
    let keys: Vec<String> = query(&first, "authored").objects.iter().map(|object| object.object_key.clone()).collect();
    assert_eq!(keys.len(), 2);

    // 다음 틱에 새 댓글이 열린다.
    let base = by_id("PR_synth_a");
    let mut comments = base["comments"]["nodes"].as_array().unwrap().clone();
    comments.push(comment(201, "2026-03-02T01:02:00Z"));
    let fresh = json!({ "comments": { "pageInfo": { "hasPreviousPage": false }, "nodes": comments } });
    let fake = FakeGh::new(FakeOptions {
        search: authored(),
        overrides: node_map("PR_synth_a", spread(&base, fresh)),
        ..FakeOptions::default()
    });
    tick(&db, &fake, "2026-03-02T01:03:00.000Z");
    assert_eq!(open_event(&db, "gh:comment:201"), Some(("opened".to_owned(), None)));

    // 그 뒤 두 틱은 a를 혼자 읽어도 넘친다. 부재 두 번이면 철회 기준에 닿는다.
    for now in ["2026-03-02T01:06:00.000Z", "2026-03-02T01:09:00.000Z"] {
        let fake = FakeGh::new(FakeOptions {
            search: authored(),
            hook: Some(Box::new(|request| {
                let hit = request.stdin.as_deref().is_some_and(|input| input.contains("PR_synth_a"));
                hit.then(|| ProcessOutput::ended(ProcessEnd::OutputTooLarge))
            })),
            ..FakeOptions::default()
        });
        let results = tick(&db, &fake, now);
        let authored = query(&results, "authored");
        assert_eq!((authored.outcome, authored.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
        assert!(!authored.snapshot);
        assert_eq!(watermark(&db, "authored").as_deref(), Some(now));
    }

    let objects = ObjectRepo::new(db.connection());
    for key in &keys {
        assert_eq!(objects.get(key).unwrap().unwrap().missing_runs, 0, "{key}");
    }
    assert_eq!(open_event(&db, "gh:comment:201"), Some(("opened".to_owned(), None)), "읽지 못한 동안 닫히지 않는다");
}

fn progress(db: &Database, id: &str) -> Option<String> {
    CursorRepo::new(db.connection()).get(SourceName::Github, id).unwrap().and_then(|cursor| cursor.progress_at)
}

/// 모든 질의의 후보에 늘 시간 초과인 I_slow가 있다. 전진하는 질의가 없어 뒤처짐으로는 풀리지 않는다.
fn slow_everywhere() -> Arc<FakeGh> {
    let with_slow = |id: &str| ids(&["I_slow", id]);
    FakeGh::new(FakeOptions {
        search: vec![
            ("review-requested:", with_slow("PR_synth_b")),
            ("assignee:", with_slow("I_synth_c")),
            ("author:", with_slow("PR_synth_a")),
            ("mentions:", ids(&["I_slow"])),
            ("is:closed", with_slow("I_synth_e")),
        ],
        hook: Some(Box::new(|request| {
            let slow = request.stdin.as_deref().is_some_and(|input| input.contains("\"I_slow\""));
            slow.then(|| ProcessOutput::ended(ProcessEnd::TimedOut))
        })),
        ..FakeOptions::default()
    })
}

// 섹션 5의 반론: 모든 질의가 함께 partial이면 기준 시각이 서지 않아 30분 규칙이 영영 걸리지 않는다.
// 이어진 partial이 2시간이면 넘긴다. 그동안에도 같은 묶음의 다른 노드는 틱마다 읽어 저장한다.
#[test]
fn 모든_질의가_느린_노드로_함께_멈춰도_partial이_2시간_이어지면_넘긴다() {
    let db = Database::in_memory().unwrap();
    tick(&db, &FakeGh::new(FakeOptions::default()), "2026-03-02T00:00:00.000Z");

    for now in ["2026-03-02T01:00:00.000Z", "2026-03-02T02:00:00.000Z", "2026-03-02T02:59:00.000Z"] {
        let results = tick(&db, &slow_everywhere(), now);
        for def in &DEFS {
            let result = query(&results, def.id);
            assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Partial, Some("node_unreadable")));
            assert_eq!(watermark(&db, def.id).as_deref(), Some("2026-03-02T00:00:00.000Z"));
            assert_eq!(
                progress(&db, def.id).as_deref(),
                Some("2026-03-02T01:00:00.000Z"),
                "첫 partial의 시각에 머문다"
            );
        }
        assert_eq!(query(&results, "assigned").objects.len(), 1, "같은 묶음의 다른 노드는 읽어 저장한다");
    }

    let results = tick(&db, &slow_everywhere(), "2026-03-02T03:00:00.000Z");
    for def in &DEFS {
        let result = query(&results, def.id);
        assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Ok, Some("node_quarantined")));
        assert!(!result.snapshot, "읽지 못한 I_slow가 있어 스냅샷이 아니다");
        assert_eq!(watermark(&db, def.id).as_deref(), Some("2026-03-02T03:00:00.000Z"));
        assert_eq!(progress(&db, def.id), None);
    }
    let objects = ObjectRepo::new(db.connection());
    for object in
        ["review_requested", "assigned", "authored", "closed"].iter().flat_map(|id| &query(&results, id).objects)
    {
        assert_eq!(objects.get(&object.object_key).unwrap().unwrap().missing_runs, 0, "{}", object.object_key);
    }

    // 넘긴 뒤에는 커서가 전진해 잘림 기준도 옮겨 간다.
    assert_eq!(
        earliest_window(&context_from(&db, at("2026-03-02T03:03:00.000Z"))),
        Some(at("2026-03-02T02:55:00.000Z"))
    );
}
