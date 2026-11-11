//! 배포 연결, 근무일.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{NormalizedObject, ObjectKind, Outcome, SourceResult};
use kairos_core::{LocalDate, OffDates};
use kairos_ledger::{OffDateBook, upcoming_deploys};
use kairos_policy::DeployLinkStatus;
use kairos_store::Database;

const MONDAY_D1: &str = "2026-03-03T01:00:00.000Z"; // 화요일 10:00 KST, 수요일 배포의 D-1
const DEPLOY_DAY: &str = "2026-03-04T00:30:00.000Z"; // 수요일 09:30 KST
const MILESTONE: &str = "합성 기능 고도화";

fn deploys_at(when: &str) -> SourceResult {
    SourceResult { query: "deploys".into(), ..calendar(when, vec![deploy("d1")]) }
}

fn milestone_issue(number: i64) -> NormalizedObject {
    NormalizedObject { milestone_title: Some(MILESTONE.into()), ..issue(number) }
}

fn deploy_fixture(db: &Database) {
    ingest(db, deploys_at(T0), T0);
    let pr = NormalizedObject { text_head: Some("feat: 합성 기능 고도화 API".into()), ..pull_request(2) };
    ingest(db, SourceResult { snapshot: true, objects: vec![milestone_issue(1), pr], ..result() }, T0);
}

fn risks(db: &Database) -> usize {
    all_events(db).iter().filter(|event| event.event_type.starts_with("deploy_risk")).count()
}

fn linked_count(db: &Database) -> usize {
    upcoming_deploys(db.connection(), at(MONDAY_D1)).unwrap()[0].link.object_keys.len()
}

#[test]
fn 배포_연결_d1에_p1_당일에_p0를_각_1회_만들고_기준선보다_우선한다() {
    let db = test_db();
    deploy_fixture(&db);
    assert_eq!(count(&db, "events"), 1);

    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    let d1 = event_by_key(&db, "deploy:d1:d1@2026-03-04");
    assert_eq!(d1.rule_id, "p1.deploy_d1");
    assert_eq!(d1.baseline, 0);

    ingest(&db, deploys_at(DEPLOY_DAY), DEPLOY_DAY);
    ingest(&db, deploys_at(DEPLOY_DAY), DEPLOY_DAY);
    assert_eq!(event_by_key(&db, "deploy:d1:d0@2026-03-04").rule_id, "p0.deploy_day");
    assert_eq!(risks(&db), 2);
}

#[test]
fn 배포_연결_github를_읽은_적이_없으면_판정하지_않는다() {
    let db = test_db();
    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    assert_eq!(risks(&db), 0);
}

#[test]
fn 배포_연결_조인에_실패하면_연결_없음이고_위험_이벤트가_없다() {
    let db = test_db();
    ingest(&db, deploys_at(T0), T0);
    let other = NormalizedObject { milestone_title: Some("다른 마일스톤".into()), ..issue(1) };
    ingest(&db, SourceResult { objects: vec![other], ..result() }, MONDAY_D1);
    assert_eq!(upcoming_deploys(db.connection(), at(MONDAY_D1)).unwrap()[0].link.status, DeployLinkStatus::NoLink);
    assert_eq!(risks(&db), 0);
}

#[test]
fn 배포_연결_건수는_조회_시점에_계산한다() {
    let db = test_db();
    deploy_fixture(&db);
    assert_eq!(linked_count(&db), 2);
    ingest(&db, SourceResult { snapshot: true, objects: vec![milestone_issue(1)], ..result() }, MONDAY_D1);
    assert_eq!(linked_count(&db), 1);
}

#[test]
fn 배포_연결_연결_건이_0건이_되면_github_ok로_닫힌다() {
    let db = test_db();
    deploy_fixture(&db);
    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    let cleared = vec![
        NormalizedObject { state: Some("closed".into()), ..milestone_issue(1) },
        NormalizedObject { state: Some("merged".into()), ..pull_request(2) },
    ];
    let when = "2026-03-03T02:00:00.000Z";
    ingest(&db, SourceResult { snapshot: true, objects: cleared, ..result_at(when) }, when);
    let closed = event_by_key(&db, "deploy:d1:d1@2026-03-04");
    assert_eq!(closed.close_reason.as_deref(), Some("withdrawn"));
    assert!(closed.close_evidence_event.unwrap().starts_with("deploy:d1:cleared:"));
}

#[test]
fn 배포_연결_github_실패는_연결_건_0건_판정을_내지_않는다() {
    let db = test_db();
    deploy_fixture(&db);
    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    ingest(&db, SourceResult { outcome: Outcome::Auth, ..result() }, MONDAY_D1);
    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    assert_eq!(event_by_key(&db, "deploy:d1:d1@2026-03-04").close_reason, None);
}

#[test]
fn 배포_연결_객체의_내_객체_댓글은_p1이다() {
    let db = test_db();
    deploy_fixture(&db);
    ingest(&db, SourceResult { events: vec![comment("deploy", T1)], ..later() }, T1);
    assert_eq!(event_by_key(&db, "gh:comment:deploy").rule_id, "p1.deploy_owned_comment");
}

fn holiday(id: &str, start: &str, end: &str) -> NormalizedObject {
    NormalizedObject {
        kind: ObjectKind::DayOff,
        all_day: Some(true),
        start_date: Some(start.into()),
        end_date: Some(end.into()),
        start_at: None,
        end_at: None,
        ..meeting(id)
    }
}

fn holidays_at(when: &str, objects: Vec<NormalizedObject>) -> SourceResult {
    SourceResult { query: "holidays".into(), ..calendar(when, objects) }
}

fn dates(values: impl IntoIterator<Item = LocalDate>) -> Vec<String> {
    values.into_iter().map(|date| date.to_string()).collect()
}

#[test]
fn 근무일_day_off_일정은_시작_포함_끝_제외로_펼친다() {
    let db = test_db();
    ingest(&db, holidays_at(T0, vec![holiday("h1", "2026-03-02", "2026-03-04")]), T0);
    let extra: OffDates = [LocalDate::parse("2026-03-09").unwrap()].into_iter().collect();
    let off = OffDateBook::new(db.connection()).load_off_dates(&extra).unwrap();
    assert_eq!(dates(off.iter()), ["2026-03-02", "2026-03-03", "2026-03-09"]);
}

#[test]
fn 배포_d1은_휴일을_건너뛴_직전_근무일이다() {
    let db = test_db();
    ingest(&db, holidays_at(T0, vec![holiday("h2", "2026-03-03", "2026-03-04")]), T0);
    deploy_fixture(&db);
    ingest(&db, deploys_at(T1), "2026-03-02T02:00:00.000Z");
    assert_eq!(event_by_key(&db, "deploy:d1:d1@2026-03-04").rule_id, "p1.deploy_d1");
}

// 아래는 원본에 없는 시험이다. 쉬는 날을 읽는 범위를 고정한다.

fn shared_at(when: &str, objects: Vec<NormalizedObject>) -> SourceResult {
    SourceResult { query: "crew_off".into(), ..calendar(when, objects) }
}

#[test]
fn 공유_캘린더의_쉬는_날은_수집_판정에서_빠지고_보여_주기만_한다() {
    let db = test_db();
    ingest(&db, holidays_at(T0, vec![holiday("h3", "2026-03-05", "2026-03-06")]), T0);
    ingest(&db, shared_at(T0, vec![holiday("h4", "2026-03-01", "2026-03-04")]), T0);
    let book = OffDateBook::new(db.connection());
    let none = OffDates::new();
    assert_eq!(
        dates(book.load_off_dates(&none).unwrap().iter()),
        ["2026-03-01", "2026-03-02", "2026-03-03", "2026-03-05"]
    );
    assert_eq!(dates(book.load_watch_off_dates(&none).unwrap().iter()), ["2026-03-05"]);
    assert_eq!(dates(book.shared_off_dates(at(T0)).unwrap()), ["2026-03-02", "2026-03-03"]);
}

#[test]
fn 쉬는_날_일정은_31일까지만_펼친다() {
    let db = test_db();
    ingest(&db, holidays_at(T0, vec![holiday("h5", "2026-01-01", "2026-12-31")]), T0);
    let off = OffDateBook::new(db.connection()).load_off_dates(&OffDates::new()).unwrap();
    let dates = dates(off.iter());
    assert_eq!(dates.len(), 31);
    assert_eq!(dates.last().map(String::as_str), Some("2026-01-31"));
}

#[test]
fn 취소되거나_두_번_빠진_쉬는_날은_세지_않는다() {
    let db = test_db();
    let cancelled = NormalizedObject { state: Some("cancelled".into()), ..holiday("h6", "2026-03-05", "2026-03-06") };
    ingest(&db, holidays_at(T0, vec![holiday("h7", "2026-03-02", "2026-03-03"), cancelled]), T0);
    let book = OffDateBook::new(db.connection());
    assert_eq!(dates(book.load_off_dates(&OffDates::new()).unwrap().iter()), ["2026-03-02"]);
    ingest(&db, holidays_at(T1, vec![]), T1);
    assert_eq!(dates(book.load_off_dates(&OffDates::new()).unwrap().iter()), ["2026-03-02"]);
    ingest(&db, holidays_at(T2, vec![]), T2);
    assert!(book.load_off_dates(&OffDates::new()).unwrap().iter().next().is_none());
}

fn postponed(when: &str) -> SourceResult {
    let later =
        NormalizedObject { start_date: Some("2026-03-06".into()), end_date: Some("2026-03-07".into()), ..deploy("d1") };
    SourceResult { query: "deploys".into(), ..calendar(when, vec![later]) }
}

#[test]
fn 배포가_밀리면_새_날짜의_d1과_당일이_다시_나고_같은_날짜는_한_번이다() {
    const THURSDAY_D1: &str = "2026-03-05T01:00:00.000Z"; // 금요일 배포의 D-1
    const FRIDAY: &str = "2026-03-06T00:30:00.000Z";
    let db = test_db();
    deploy_fixture(&db);
    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    ingest(&db, deploys_at(DEPLOY_DAY), DEPLOY_DAY);
    assert_eq!(risks(&db), 2);

    ingest(&db, postponed(THURSDAY_D1), THURSDAY_D1);
    ingest(&db, postponed(THURSDAY_D1), THURSDAY_D1);
    assert_eq!(event_by_key(&db, "deploy:d1:d1@2026-03-06").rule_id, "p1.deploy_d1");
    assert_eq!(risks(&db), 3);

    ingest(&db, postponed(FRIDAY), FRIDAY);
    ingest(&db, postponed(FRIDAY), FRIDAY);
    assert_eq!(event_by_key(&db, "deploy:d1:d0@2026-03-06").rule_id, "p0.deploy_day");
    assert_eq!(risks(&db), 4);
}

/// 날짜를 키에 넣기 전에 저장된 행으로 만든다.
fn undated(db: &Database, key: &str) {
    let dated = format!("{key}@2026-03-04");
    db.connection().execute("UPDATE events SET dedupe_key = ?1 WHERE dedupe_key = ?2", [key, dated.as_str()]).unwrap();
}

#[test]
fn 날짜_없는_옛_키의_d1은_같은_날짜면_다시_나지_않는다() {
    let db = test_db();
    deploy_fixture(&db);
    ingest(&db, deploys_at(MONDAY_D1), MONDAY_D1);
    undated(&db, "deploy:d1:d1");
    ingest(&db, deploys_at(MONDAY_D1), "2026-03-03T02:00:00.000Z");
    assert_eq!(risks(&db), 1);
}

#[test]
fn 날짜_없는_옛_키의_당일이_지난_날짜면_새_날짜의_당일이_난다() {
    const FRIDAY: &str = "2026-03-06T00:30:00.000Z";
    let db = test_db();
    deploy_fixture(&db);
    ingest(&db, deploys_at(DEPLOY_DAY), DEPLOY_DAY);
    undated(&db, "deploy:d1:d0");
    ingest(&db, postponed(FRIDAY), FRIDAY);
    assert_eq!(event_by_key(&db, "deploy:d1:d0@2026-03-06").rule_id, "p0.deploy_day");
}

#[test]
fn 옛_키가_지워져도_같은_날짜의_단계는_다시_나지_않는다() {
    const FRIDAY: &str = "2026-03-06T00:30:00.000Z";
    let db = test_db();
    deploy_fixture(&db);
    ingest(&db, deploys_at(DEPLOY_DAY), DEPLOY_DAY);
    undated(&db, "deploy:d1:d0");
    ingest(&db, postponed(FRIDAY), FRIDAY);
    db.connection().execute("DELETE FROM events WHERE dedupe_key = 'deploy:d1:d0'", []).unwrap();
    let before = risks(&db);
    ingest(&db, postponed(FRIDAY), "2026-03-06T01:00:00.000Z");
    assert_eq!(risks(&db), before);
}
