//! 기준선.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{NormalizedEvent, Outcome, SourceName, SourceResult};
use kairos_ledger::CursorRepo;
use kairos_store::Database;

fn cursor(db: &Database) -> kairos_core::model::CursorState {
    CursorRepo::new(db.connection()).get(SourceName::Github, "tick").unwrap().unwrap()
}

fn mentioned(id: &str, at: &str) -> NormalizedEvent {
    NormalizedEvent { mentions_me: Some(true), mention_count: Some(1), ..comment(id, at) }
}

#[test]
fn 기준선_첫_정상_수집의_항목은_baseline_1이고_표시_대상이_아니다() {
    let db = test_db();
    let report = ingest(
        &db,
        SourceResult {
            objects: vec![issue(1), issue(2), pull_request(3)],
            events: vec![comment("old", BEFORE), mentioned("run", T0)],
            ..result()
        },
        T0,
    );
    assert!(report.t0_established);
    assert_eq!(count(&db, "objects"), 3);
    let baselined: i64 =
        db.connection().query_row("SELECT count(*) FROM objects WHERE baseline = 1", [], |row| row.get(0)).unwrap();
    assert_eq!(baselined, 3);
    assert!(all_events(&db).iter().all(|event| event.baseline == 1));
    assert!(would_show_events(&db).is_empty());
}

#[test]
fn 기준선_t0_이후_새_객체와_새_사건은_표시_대상이다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    ingest(&db, SourceResult { objects: vec![issue(1), issue(2)], events: vec![comment("new", T1)], ..later() }, T1);
    assert!(!object_row(&db, "gh:acme/widget#2").baseline);
    assert_eq!(event_by_key(&db, "gh:comment:new").baseline, 0);
    let shown: Vec<String> = would_show_events(&db).into_iter().map(|row| row.0).collect();
    assert_eq!(shown, ["gh:comment:new"]);
}

#[test]
fn 기준선_t0_이후에_처음_보여도_사건이_t0_이전이면_baseline_1이다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    ingest(&db, SourceResult { events: vec![comment("old", BEFORE)], ..later() }, T1);
    assert_eq!(event_by_key(&db, "gh:comment:old").baseline, 1);
}

#[test]
fn 기준선_첫_정상_수집_전의_partial도_baseline_1이고_t0를_세우지_않는다() {
    let db = test_db();
    let report = ingest(
        &db,
        SourceResult { outcome: Outcome::Partial, objects: vec![issue(1)], events: vec![comment("1", T0)], ..result() },
        T0,
    );
    assert!(!report.t0_established);
    assert_eq!(cursor(&db).t0, None);
    assert_eq!(cursor(&db).first_run_at.as_deref(), Some(T0));
    assert!(object_row(&db, "gh:acme/widget#1").baseline);
    assert_eq!(event_by_key(&db, "gh:comment:1").baseline, 1);
}

#[test]
fn 기준선_첫_시도_뒤에_온_사건은_첫_ok_전이어도_묻히지_않고_t0는_첫_시도_시각이다() {
    let db = test_db();
    let partial = |when: &str, events: Vec<NormalizedEvent>| SourceResult {
        outcome: Outcome::Partial,
        watermark: None,
        events,
        ..result_at(when)
    };
    ingest(&db, partial(T0, vec![comment("old", BEFORE)]), T0);
    ingest(&db, partial(T1, vec![mentioned("between", T1), comment("older", BEFORE)]), T1);
    let report = ingest(&db, SourceResult { events: vec![comment("late", T2)], ..result_at(T3) }, T3);
    let baselines: Vec<i64> = ["old", "older", "between", "late"]
        .iter()
        .map(|id| event_by_key(&db, &format!("gh:comment:{id}")).baseline)
        .collect();
    assert_eq!(baselines, [1, 1, 0, 0]);
    assert!(report.t0_established);
    assert_eq!(cursor(&db).t0.as_deref(), Some(T0));
}

#[test]
fn 기준선_첫_ok_전의_일정_변화는_시각이_지금이라_쏟아지지_않게_baseline이다() {
    let db = test_db();
    let partial = |when: &str, id: &str| SourceResult {
        outcome: Outcome::Partial,
        watermark: None,
        snapshot: false,
        ..calendar(when, vec![meeting(id)])
    };
    ingest(&db, partial(T0, "m1"), T0);
    ingest(&db, partial(T1, "m2"), T1);
    assert!(all_events(&db).iter().all(|event| event.baseline == 1));
}

#[test]
fn 기준선_t0는_질의별이라_늦게_성공한_질의의_첫_수집도_쏟아지지_않는다() {
    let db = test_db();
    seed(&db, SourceResult { query: "assigned".into(), objects: vec![issue(1)], ..result() });
    ingest(&db, SourceResult { query: "authored".into(), objects: vec![pull_request(9)], ..later() }, T1);
    assert!(object_row(&db, "gh:acme/widget#9").baseline);
}

#[test]
fn 기준선_객체도_사람의_새_사건은_올라온다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], events: vec![comment("old", T0)], ..result() });
    ingest(&db, SourceResult { events: vec![comment("new", T1)], ..later() }, T1);
    let fresh = event_by_key(&db, "gh:comment:new");
    assert_eq!(fresh.disposition, "opened");
    assert_eq!(fresh.baseline, 0);
}
