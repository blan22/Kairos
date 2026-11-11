//! 일정 변화, 종일 일정, 철회, 시각 경과.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{NormalizedObject, Outcome, SourceResult};
use kairos_policy::Tier;

fn soon(id: &str) -> NormalizedObject {
    NormalizedObject {
        start_at: Some("2026-03-02T10:30:00+09:00".into()),
        end_at: Some("2026-03-02T11:00:00+09:00".into()),
        ..meeting(id)
    }
}

fn types(events: &[EventRow]) -> Vec<&str> {
    events.iter().map(|event| event.event_type.as_str()).collect()
}

#[test]
fn 일정_시작이_바뀌면_같은_객체의_이동이고_임박하면_p0다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![meeting("m1")]), T0);
    ingest(&db, calendar(T1, vec![soon("m1")]), T1);

    assert_eq!(count(&db, "objects"), 1);
    let events = all_events(&db);
    assert_eq!(types(&events), ["cal_created", "cal_moved"]);
    assert_eq!(events[0].baseline, 1);
    assert_eq!(events[1].rule_id, "p0.meeting_imminent");
    assert_eq!(events[1].baseline, 0);
}

#[test]
fn 일정_표기만_다른_같은_시각은_이동이_아니다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![meeting("m1")]), T0);
    let same_instant = NormalizedObject { start_at: Some("2026-03-03T01:00:00Z".into()), ..meeting("m1") };
    ingest(&db, calendar(T1, vec![same_instant]), T1);
    assert_eq!(types(&all_events(&db)), ["cal_created", "cal_updated"]);
}

#[test]
fn 일정_변화_없으면_이벤트가_없고_재수집이_멱등이다() {
    let db = test_db();
    for when in [T0, T1, T2] {
        ingest(&db, calendar(when, vec![meeting("m1")]), when);
    }
    assert_eq!(count(&db, "events"), 1);
}

#[test]
fn 일정_t0_이후_13분_전에_생긴_회의는_p0로_기록되고_표시_대상이다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![]), T0);
    let soon = NormalizedObject {
        start_at: Some("2026-03-02T10:16:00+09:00".into()),
        end_at: Some("2026-03-02T10:46:00+09:00".into()),
        my_response: Some("needsAction".into()),
        ..meeting("m2")
    };
    ingest(&db, calendar(T1, vec![soon]), T1);
    let shown = would_show_events(&db);
    assert_eq!(shown[0].2, Tier::P0);
    assert_eq!(shown[0].1, "p0.meeting_imminent");
}

fn all_day(id: &str, start: &str, end: &str) -> NormalizedObject {
    NormalizedObject {
        all_day: Some(true),
        start_date: Some(start.into()),
        end_date: Some(end.into()),
        start_at: None,
        end_at: None,
        ..meeting(id)
    }
}

#[test]
fn 종일_일정_날짜_함정_날짜_문자열_그대로_저장한다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![all_day("m3", "2026-11-05", "2026-11-06")]), T0);
    let row = object_row(&db, "gcal:m3");
    assert_eq!(row.start_date.as_deref(), Some("2026-11-05"));
    assert_eq!(row.end_date.as_deref(), Some("2026-11-06"));
    assert_eq!(row.start_at, None);
}

#[test]
fn 종일_일정_날짜_함정_날짜가_아닌_값이나_시각이_섞이면_거부한다() {
    let db = test_db();
    let as_instant = all_day("m4", "2026-11-05T00:00:00Z", "2026-11-06");
    assert_eq!(ingest(&db, calendar(T0, vec![as_instant]), T0).error_code.as_deref(), Some("bad_all_day"));
    let mixed = NormalizedObject {
        all_day: Some(true),
        start_date: Some("2026-11-05".into()),
        end_date: Some("2026-11-06".into()),
        ..meeting("m5")
    };
    assert_eq!(ingest(&db, calendar(T0, vec![mixed]), T0).error_code.as_deref(), Some("bad_all_day"));
}

#[test]
fn 시간_일정은_오프셋을_유지하고_오프셋이_없으면_거부한다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![meeting("m1")]), T0);
    assert_eq!(object_row(&db, "gcal:m1").start_at.as_deref(), Some("2026-03-03T10:00:00+09:00"));
    let naive = NormalizedObject { start_at: Some("2026-03-03T10:00:00".into()), ..meeting("m6") };
    assert_eq!(ingest(&db, calendar(T1, vec![naive]), T1).error_code.as_deref(), Some("bad_timed_event"));
}

#[test]
fn 철회_ok_스냅샷_2회_연속_부재여야_닫힌다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![]), T0);
    ingest(&db, calendar(T1, vec![soon("m7")]), T1);
    let opened = "gcal:m7#created";

    ingest(&db, calendar(T2, vec![]), T2);
    assert_eq!(event_by_key(&db, opened).close_reason, None);

    ingest(&db, calendar(T3, vec![]), T3);
    let closed = event_by_key(&db, opened);
    assert_eq!(closed.close_reason.as_deref(), Some("withdrawn"));
    assert!(closed.close_evidence_event.unwrap().contains("#withdrawn:"));
}

#[test]
fn 철회_실패와_partial_실행은_부재를_세지_않는다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![meeting("m8")]), T0);
    ingest(&db, SourceResult { outcome: Outcome::Partial, ..calendar(T1, vec![]) }, T1);
    ingest(&db, SourceResult { outcome: Outcome::Timeout, ..calendar(T2, vec![]) }, T2);
    assert_eq!(object_row(&db, "gcal:m8").missing_runs, 0);
}

#[test]
fn 철회_다시_나타나면_부재_횟수가_초기화된다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![meeting("m9")]), T0);
    ingest(&db, calendar(T1, vec![]), T1);
    ingest(&db, calendar(T2, vec![meeting("m9")]), T2);
    assert_eq!(object_row(&db, "gcal:m9").missing_runs, 0);
}

#[test]
fn 철회_다른_질의의_스냅샷은_부재로_세지_않는다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![meeting("m10")]), T0);
    ingest(&db, SourceResult { query: "deploys".into(), ..calendar(T1, vec![]) }, T1);
    assert_eq!(object_row(&db, "gcal:m10").missing_runs, 0);
}

#[test]
fn 시각_경과_끝난_회의는_근거_이벤트_없이_expired로_닫힌다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![]), T0);
    ingest(&db, calendar(T1, vec![soon("m11")]), T1);
    let after = "2026-03-02T03:00:00.000Z";
    ingest(&db, calendar(after, vec![soon("m11")]), after);
    let closed = event_by_key(&db, "gcal:m11#created");
    assert_eq!(closed.close_reason.as_deref(), Some("expired"));
    assert_eq!(closed.close_evidence_event, None);
}

#[test]
fn 시각_경과도_ok_실행에서만_판정한다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![]), T0);
    ingest(&db, calendar(T1, vec![soon("m12")]), T1);
    let after = "2026-03-02T03:00:00.000Z";
    ingest(&db, SourceResult { outcome: Outcome::Auth, ..calendar(after, vec![]) }, after);
    assert_eq!(event_by_key(&db, "gcal:m12#created").close_reason, None);
}

#[test]
fn 일정이_왕복해도_이동마다_이벤트가_남고_재수집은_늘리지_않는다() {
    let db = test_db();
    ingest(&db, calendar(T0, vec![meeting("m1")]), T0);
    ingest(&db, calendar(T1, vec![soon("m1")]), T1);
    ingest(&db, calendar(T2, vec![meeting("m1")]), T2);
    ingest(&db, calendar(T3, vec![soon("m1")]), T3);
    ingest(&db, calendar(T3, vec![soon("m1")]), T3);
    let events = all_events(&db);
    assert_eq!(types(&events), ["cal_created", "cal_moved", "cal_moved", "cal_moved"]);
    assert_eq!(events[3].rule_id, "p0.meeting_imminent");
    assert_eq!(events[3].baseline, 0);
}

#[test]
fn 취소가_되살아났다가_다시_취소되어도_이벤트가_남는다() {
    let db = test_db();
    let cancelled = || NormalizedObject { state: Some("cancelled".into()), ..meeting("m1") };
    ingest(&db, calendar(T0, vec![meeting("m1")]), T0);
    ingest(&db, calendar(T1, vec![cancelled()]), T1);
    ingest(&db, calendar(T2, vec![meeting("m1")]), T2);
    ingest(&db, calendar(T3, vec![cancelled()]), T3);
    let kinds = all_events(&db);
    assert_eq!(types(&kinds).iter().filter(|kind| **kind == "cal_cancelled").count(), 2);
}
