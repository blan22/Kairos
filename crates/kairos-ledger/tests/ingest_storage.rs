//! 재수집, watermark, 계약 판정, 정제, 실패 처리.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, Outcome, SourceName, SourceResult};
use kairos_ledger::CursorRepo;
use kairos_store::Database;

fn watermark(db: &Database) -> Option<String> {
    CursorRepo::new(db.connection()).get(SourceName::Github, "tick").unwrap().unwrap().watermark
}

#[test]
fn 재수집_멱등_같은_결과를_두_번_넣어도_이벤트는_하나다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let batch = SourceResult { objects: vec![issue(1)], events: vec![comment("1", T1)], ..later() };

    let first = ingest(&db, batch.clone(), T1);
    let stored = event_by_key(&db, "gh:comment:1");
    let second = ingest(&db, batch, T2);

    assert_eq!(first.events_inserted, 1);
    assert_eq!(second.events_inserted, 0);
    assert_eq!(second.events_duplicate, 1);
    assert_eq!(count(&db, "events"), 1);
    assert_eq!(event_by_key(&db, "gh:comment:1"), stored);
}

#[test]
fn 재수집_멱등_겹친_구간을_다시_받아도_객체는_하나고_기준선이_유지된다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let renamed = kairos_core::model::NormalizedObject { text_head: Some("제목 변경".into()), ..issue(1) };
    ingest(&db, SourceResult { objects: vec![renamed], ..later() }, T1);
    assert_eq!(count(&db, "objects"), 1);
    let row = object_row(&db, "gh:acme/widget#1");
    assert!(row.baseline);
    assert_eq!(row.text_head.as_deref(), Some("제목 변경"));
    assert_eq!(row.first_seen_at, T0);
}

fn 실패_시_watermark_불변(outcome: Outcome) {
    let db = test_db();
    seed(&db, result());
    let report = ingest(&db, SourceResult { outcome, watermark: Some(T3.into()), ..later() }, T1);
    assert_eq!(report.outcome, outcome);
    assert!(!report.watermark_advanced);
    assert_eq!(watermark(&db).as_deref(), Some(T0));
    assert_eq!(count(&db, "source_run"), 2);
}

#[test]
fn 실패_시_watermark_불변_partial() {
    실패_시_watermark_불변(Outcome::Partial);
}

#[test]
fn 실패_시_watermark_불변_tool_missing() {
    실패_시_watermark_불변(Outcome::ToolMissing);
}

#[test]
fn 실패_시_watermark_불변_auth() {
    실패_시_watermark_불변(Outcome::Auth);
}

#[test]
fn 실패_시_watermark_불변_rate_limited() {
    실패_시_watermark_불변(Outcome::RateLimited);
}

#[test]
fn 실패_시_watermark_불변_parse_error() {
    실패_시_watermark_불변(Outcome::ParseError);
}

#[test]
fn 실패_시_watermark_불변_timeout() {
    실패_시_watermark_불변(Outcome::Timeout);
}

#[test]
fn 실패_시_watermark_불변_contract_break() {
    실패_시_watermark_불변(Outcome::ContractBreak);
}

#[test]
fn 실패_시_watermark_불변_not_connected() {
    실패_시_watermark_불변(Outcome::NotConnected);
}

#[test]
fn watermark는_ok에서_전진하고_과거로_돌아가지_않는다() {
    let db = test_db();
    seed(&db, result());
    let forward = ingest(&db, SourceResult { watermark: Some(T2.into()), ..later() }, T2);
    assert!(forward.watermark_advanced);
    let backward = ingest(&db, SourceResult { watermark: Some(T1.into()), ..later() }, T2);
    assert!(!backward.watermark_advanced);
    assert_eq!(watermark(&db).as_deref(), Some(T2));
}

#[test]
fn watermark는_시간대가_달라도_순간으로_비교한다() {
    let db = test_db();
    seed(&db, SourceResult { watermark: Some("2026-03-02T01:00:00Z".into()), ..result() });
    ingest(&db, SourceResult { watermark: Some("2026-03-02T09:30:00+09:00".into()), ..later() }, T1);
    assert_eq!(watermark(&db).as_deref(), Some(T0));
}

#[test]
fn partial은_읽은_만큼_저장하지만_전진하지_않고_재수집이_이어받는다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let page = vec![comment("1", T1)];
    let partial = ingest(
        &db,
        SourceResult { outcome: Outcome::Partial, watermark: Some(T2.into()), events: page.clone(), ..later() },
        T1,
    );
    assert_eq!(partial.events_inserted, 1);
    assert_eq!(watermark(&db).as_deref(), Some(T0));

    let events = [page, vec![comment("2", T2)]].concat();
    let retried = ingest(&db, SourceResult { watermark: Some(T2.into()), events, ..later() }, T2);
    assert_eq!(retried.events_inserted, 1);
    assert_eq!(retried.events_duplicate, 1);
    assert_eq!(watermark(&db).as_deref(), Some(T2));
}

#[test]
fn 실패_결과의_객체와_이벤트는_저장하지_않는다() {
    let db = test_db();
    let failed =
        SourceResult { outcome: Outcome::Timeout, objects: vec![issue(1)], events: vec![comment("1", T0)], ..result() };
    ingest(&db, failed, T0);
    assert_eq!(count(&db, "objects"), 0);
    assert_eq!(count(&db, "events"), 0);
}

#[test]
fn 건수가_맞지_않는_ok는_parse_error로_기록하고_저장하지_않는다() {
    let db = test_db();
    seed(&db, result());
    let report = ingest(
        &db,
        SourceResult {
            expected_count: Some(3),
            parsed_count: 2,
            watermark: Some(T2.into()),
            events: vec![comment("1", T1)],
            ..later()
        },
        T1,
    );
    assert_eq!(report.outcome, Outcome::ParseError);
    assert_eq!(report.error_code.as_deref(), Some("count_mismatch"));
    assert_eq!(count(&db, "events"), 0);
    assert_eq!(watermark(&db).as_deref(), Some(T0));
}

#[test]
fn 신원이_다르면_실패로_기록한다() {
    let db = test_db();
    let wrong =
        ingest(&db, SourceResult { identity: Some("someone-else".into()), objects: vec![issue(1)], ..result() }, T0);
    assert_eq!(wrong.outcome, Outcome::ContractBreak);
    assert_eq!(wrong.error_code.as_deref(), Some("identity_mismatch"));
    assert_eq!(count(&db, "objects"), 0);
    assert_eq!(CursorRepo::new(db.connection()).get(SourceName::Github, "tick").unwrap(), None);
    let identity = SourceName::Github.self_identity().map(str::to_owned);
    assert_eq!(ingest(&db, SourceResult { identity, ..result() }, T0).outcome, Outcome::Ok);
}

#[test]
fn dm_기록에는_본문_앞부분도_넣을_수_없다() {
    let db = test_db();
    let dm = NormalizedEvent {
        text_head: Some("합성 DM 내용".into()),
        ..NormalizedEvent::new("slack:D1:1", SourceName::Slack, EventType::SlackDmSeen, T0, ActorKind::Human)
    };
    let report = ingest(&db, SourceResult { source: SourceName::Slack, events: vec![dm], ..result() }, T0);
    assert_eq!(report.error_code.as_deref(), Some("forbidden_field"));
}

#[test]
fn 소스가_다른_항목은_거부한다() {
    let db = test_db();
    let report = ingest(&db, SourceResult { source: SourceName::Slack, objects: vec![issue(1)], ..result() }, T0);
    assert_eq!(report.outcome, Outcome::ContractBreak);
}

#[test]
fn 소스는_ingest가_만드는_이벤트_종류를_낼_수_없다() {
    let db = test_db();
    let forged = NormalizedEvent { event_type: EventType::DeployRiskD0, ..comment("1", T0) };
    let report = ingest(&db, SourceResult { events: vec![forged], ..result() }, T0);
    assert_eq!(report.error_code.as_deref(), Some("bad_event"));
}

#[test]
fn 외부_문자열은_정제되고_120자로_저장된다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let text = format!("\u{1b}[31m경고\u{1b}[0m\u{202e}{}\n둘째 줄", "가".repeat(300));
    let event = NormalizedEvent { text_head: Some(text), ..comment("1", T1) };
    ingest(&db, SourceResult { events: vec![event], ..later() }, T1);
    let head = event_by_key(&db, "gh:comment:1").text_head.unwrap();
    assert_eq!(head.chars().count(), 120);
    assert!(head.starts_with("경고가"));
    assert!(!head.chars().any(|c| c < '\u{20}' || c == '\u{202e}'));
}

#[test]
fn 저장이_중간에_실패하면_전부_되돌리고_실패_행만_남긴다() {
    let db = test_db();
    let broken = kairos_core::model::NormalizedObject { object_key: "\u{7}".into(), ..issue(2) };
    let report =
        ingest(&db, SourceResult { objects: vec![issue(1), broken], watermark: Some(T3.into()), ..result() }, T0);
    assert_eq!(report.outcome, Outcome::ContractBreak);
    assert_eq!(report.error_code.as_deref(), Some("ingest_failed"));
    assert_eq!(count(&db, "objects"), 0);
    assert_eq!(count(&db, "source_run"), 1);
    assert_eq!(CursorRepo::new(db.connection()).get(SourceName::Github, "tick").unwrap(), None);
}

#[test]
fn not_connected는_기록만_남고_t0를_세우지_않는다() {
    let db = test_db();
    let report = ingest(
        &db,
        SourceResult {
            source: SourceName::Notion,
            query: "mail".into(),
            outcome: Outcome::NotConnected,
            watermark: None,
            ..result()
        },
        T0,
    );
    assert_eq!(report.outcome, Outcome::NotConnected);
    let outcome: String = db.connection().query_row("SELECT outcome FROM source_run", [], |row| row.get(0)).unwrap();
    assert_eq!(outcome, "not_connected");
    assert_eq!(CursorRepo::new(db.connection()).get(SourceName::Notion, "mail").unwrap(), None);
}

#[test]
fn _30분_이상_지난_새_사건은_catchup이다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let late = "2026-03-02T02:00:00.000Z";
    let events = vec![comment("late", T1), comment("fresh", "2026-03-02T01:59:00.000Z")];
    ingest(&db, SourceResult { events, ..later() }, late);
    assert_eq!(event_by_key(&db, "gh:comment:late").catchup, 1);
    assert_eq!(event_by_key(&db, "gh:comment:fresh").catchup, 0);
}
