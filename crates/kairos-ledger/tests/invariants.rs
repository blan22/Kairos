//! 커서와 불변식. 나머지는 kairos-store의 tests/database.rs에 있다
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::SourceName;
use kairos_ledger::{CursorRepo, Invariants, check_invariants};
use kairos_store::Database;

fn window_start(db: &Database) -> Option<String> {
    let cursor = CursorRepo::new(db.connection()).get(SourceName::Slack, "q").unwrap();
    cursor.and_then(|cursor| cursor.window_start()).map(kairos_core::Instant::to_iso)
}

#[test]
fn 조회_시작은_watermark에서_overlap을_뺀_값이다() {
    let db = test_db();
    assert_eq!(window_start(&db), None);
    db.connection().execute_batch(
        "INSERT INTO cursor (source, query, watermark, overlap_s) VALUES ('slack', 'q', '2026-03-02T01:10:00.000Z', 600)",
    )
    .unwrap();
    assert_eq!(window_start(&db).as_deref(), Some("2026-03-02T01:00:00.000Z"));
}

fn insert_opened(db: &Database, extra: &str) -> Result<(), kairos_store::rusqlite::Error> {
    db.connection().execute_batch(&format!(
        "INSERT INTO events (
           dedupe_key, source, thread_key, type, actor_kind, occurred_at, first_seen_at, run_id,
           disposition, rule_id, close_reason, close_evidence_event, closed_at
         ) VALUES ('k', 'github', 'th', 'gh_comment', 'human', 'a', 'a', 1, 'opened', 'p2.owned_comment', {extra})"
    ))
}

#[test]
fn 근거_없는_종료는_스키마가_막는다() {
    let db = test_db();
    for extra in ["'done_evidence', NULL, 'a'", "'withdrawn', NULL, 'a'", "'closed_by_absence', 'x', 'a'"] {
        assert!(insert_opened(&db, extra).unwrap_err().to_string().contains("CHECK"), "{extra}");
    }
}

#[test]
fn 없는_이벤트를_근거로_삼은_종료는_불변식에_잡힌다() {
    let db = test_db();
    insert_opened(&db, "'done_evidence', 'gh:comment:ghost', 'a'").unwrap();
    assert_eq!(check_invariants(db.connection()).unwrap().closures_without_evidence, 1);
}

// 아래는 원본에 없는 시험이다.

#[test]
fn 불변식은_maintenance_json의_키로_직렬화된다() {
    let text = serde_json::to_string(&Invariants::default()).unwrap();
    assert_eq!(
        text,
        r#"{"eventsWithoutDisposition":0,"closuresWithoutEvidence":0,"unclassified":0,"itemClosuresWithoutEvidence":0}"#
    );
}

#[test]
fn 없는_이벤트를_근거로_닫힌_항목도_불변식에_잡힌다() {
    let db = test_db();
    db.connection().execute_batch(
        "INSERT INTO items (object_key, reason, tier, state, opened_at, closed_at, close_reason, close_evidence_event)
         VALUES ('k', 'assigned', 'P2', 'closed', 'a', 'b', 'done_evidence', 'ghost')",
    )
    .unwrap();
    assert_eq!(check_invariants(db.connection()).unwrap().item_closures_without_evidence, 1);
}
