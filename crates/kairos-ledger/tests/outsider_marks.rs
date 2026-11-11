//! 세션 게이트의 외부 글 표시. 보존 기한과 정비가 지나도 남는다. 고정값은 합성이다.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::KairosHome;
use kairos_core::model::{ActorKind, NormalizedEvent, SourceName, SourceResult, TrustTier};
use kairos_ledger::{EVENT_RETENTION_DAYS, EventRepo, OutsiderMarkRepo};
use kairos_store::{Database, MIGRATIONS, params};

const DAY_MS: i64 = 86_400_000;
const OBJECT: &str = "gh:acme/widget#1";

fn by(tier: Option<TrustTier>, id: &str, occurred_at: &str) -> NormalizedEvent {
    NormalizedEvent { trust_tier: tier, ..comment(id, occurred_at) }
}

fn has_outsider(db: &Database) -> bool {
    EventRepo::new(db.connection()).has_outsider_on_object(OBJECT).unwrap()
}

fn first_seen(db: &Database) -> Option<String> {
    db.connection()
        .query_row("SELECT first_seen_at FROM outsider_marks WHERE object_key = ?", params![OBJECT], |row| row.get(0))
        .ok()
}

fn seeded() -> Database {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    db
}

#[test]
fn 보존_기한이_지난_외부_글은_이벤트로_저장하지_않아도_표시를_남긴다() {
    let db = seeded();
    let old = at(T1).minus_ms((EVENT_RETENTION_DAYS + 1) * DAY_MS).to_iso();
    let report = ingest(&db, SourceResult { events: vec![by(Some(TrustTier::T3), "old", &old)], ..later() }, T1);
    assert_eq!(report.events_inserted, 0);
    assert!(find_event(&db, "gh:comment:old").is_none(), "되살아나는 이벤트는 여전히 막는다");
    assert!(has_outsider(&db));
    assert_eq!(first_seen(&db).as_deref(), Some(T1));
}

#[test]
fn 표시는_이벤트가_지워져도_남고_처음_본_시각을_지킨다() {
    let db = seeded();
    ingest(&db, SourceResult { events: vec![by(Some(TrustTier::T2), "t2", T1)], ..later() }, T1);
    db.connection().execute("DELETE FROM events", []).unwrap();
    assert!(has_outsider(&db), "정비가 이벤트를 지워도 외부 글이 없던 것이 되지 않는다");
    ingest(&db, SourceResult { events: vec![by(Some(TrustTier::T3), "t3", T2)], ..result_at(T2) }, T2);
    assert_eq!(first_seen(&db).as_deref(), Some(T1));
}

#[test]
fn 등급_없는_사람의_글도_표시하고_t0_t1_봇_다른_소스는_표시하지_않는다() {
    let db = seeded();
    let bot = NormalizedEvent { actor_kind: ActorKind::Bot, ..by(Some(TrustTier::T2), "bot", T1) };
    let me = NormalizedEvent { actor_kind: ActorKind::SelfActor, ..by(Some(TrustTier::T0), "me", T1) };
    let quiet = vec![by(Some(TrustTier::T1), "t1", T1), by(Some(TrustTier::T0), "t0", T1), bot, me];
    ingest(&db, SourceResult { events: quiet, ..later() }, T1);
    assert!(!has_outsider(&db));
    let marks = OutsiderMarkRepo::new(db.connection());
    assert!(!marks.contains(OBJECT).unwrap());

    // 세션은 GitHub 글만 읽는다. 다른 소스의 사람 글은 영구 표시로 남기지 않는다.
    let slack = NormalizedEvent {
        source: SourceName::Slack,
        object_key: Some("sl:C0SYNTH".into()),
        ..by(Some(TrustTier::T3), "slack", T1)
    };
    ingest(&db, SourceResult { source: SourceName::Slack, events: vec![slack], ..later() }, T1);
    assert!(!marks.contains("sl:C0SYNTH").unwrap());

    ingest(&db, SourceResult { events: vec![by(None, "unknown", T2)], ..result_at(T2) }, T2);
    assert!(has_outsider(&db), "등급이 없으면 모르는 것이라 믿지 않는다");
}

#[test]
fn v8_이전_db의_외부_글_이벤트를_표시로_옮긴다() {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at(dir.path().join("home"));
    home.ensure().unwrap();
    {
        let raw = kairos_store::Connection::open(home.db_path()).unwrap();
        for migration in &MIGRATIONS[..7] {
            raw.execute_batch(migration).unwrap();
        }
        raw.execute_batch(
            "PRAGMA user_version = 7;
             INSERT INTO events (dedupe_key, source, object_key, thread_key, type, actor_kind, occurred_at, first_seen_at, run_id, disposition, rule_id, trust_tier)
               VALUES ('a', 'github', 'gh:o#1', 't', 'gh_comment', 'human', 'x', '2026-02-02T00:00:00.000Z', 1, 'folded', 'r', 'T3'),
                      ('b', 'github', 'gh:o#1', 't', 'gh_comment', 'human', 'x', '2026-02-01T00:00:00.000Z', 1, 'folded', 'r', NULL),
                      ('c', 'github', 'gh:o#2', 't', 'gh_comment', 'human', 'x', 'y', 1, 'folded', 'r', 'T1'),
                      ('d', 'github', 'gh:o#3', 't', 'gh_comment', 'bot', 'x', 'y', 1, 'folded', 'r', 'T2'),
                      ('e', 'slack', 'sl:o#4', 't', 'slack_dm', 'human', 'x', 'y', 1, 'folded', 'r', 'T3');",
        )
        .unwrap();
    }
    let db = Database::open(&home).unwrap();
    let mut statement = db.connection().prepare("SELECT object_key, first_seen_at FROM outsider_marks").unwrap();
    let rows: Vec<(String, String)> =
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(rows, [("gh:o#1".to_owned(), "2026-02-01T00:00:00.000Z".to_owned())]);
}
