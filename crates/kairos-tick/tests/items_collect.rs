//! 중 collect를 지나는 시험.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_agenda::{ControlFile, StateBuilder};
use kairos_core::model::{Source, SourceName, SourceResult};
use kairos_core::{KairosError, KairosHome};
use kairos_store::{Database, params};
use kairos_tick::CollectStatus;
use tempfile::TempDir;

fn slack_result(when: &str, ts: &str) -> Vec<SourceResult> {
    vec![SourceResult {
        source: SourceName::Slack,
        started_at: when.into(),
        finished_at: when.into(),
        watermark: Some(when.into()),
        events: vec![mention(ts, when)],
        ..result()
    }]
}

fn imminent(when: &str) -> Vec<SourceResult> {
    let object = meeting_at("imminent", "2026-03-02T10:30:00+09:00", "2026-03-02T11:00:00+09:00");
    vec![SourceResult {
        source: SourceName::Calendar,
        started_at: when.into(),
        finished_at: when.into(),
        watermark: Some(when.into()),
        objects: vec![object],
        ..result()
    }]
}

fn calendar_feed() -> Arc<dyn Source> {
    direct(SourceName::Calendar, |_| imminent(T1))
}

fn calendar_home() -> (TempDir, KairosHome) {
    let (dir, home) = temp_home();
    ControlFile::new(&home).write(&control_on()).unwrap();
    prime(&Database::open(&home).unwrap(), SourceName::Calendar, "tick");
    (dir, home)
}

#[test]
fn collect_끝에서_항목을_갱신하고_state_json_건수에_반영한다() {
    let (_dir, home) = temp_home();
    prime_slack(&Database::open(&home).unwrap());
    let sources = vec![direct(SourceName::Slack, |_| slack_result(T1, "1"))];
    let report = tick(&home, T1, sources).run();
    assert_eq!(report.items.unwrap().opened, 1);
    assert_eq!(StateBuilder::read_state(&home).unwrap()["items"]["open"]["P1"], 1);
    assert!(StateBuilder::read_engine_status(&home).unwrap().ok);
}

#[test]
fn 항목_엔진이_실패해도_수집_기록과_state_json은_남고_doctor가_드러낸다() {
    let (_dir, home) = temp_home();
    let sources = || vec![direct(SourceName::Slack, |_| slack_result(T1, "1"))];
    let report = tick(&home, T1, sources()).items(|| Err(KairosError::new("SYNTHETIC_FAILURE"))).run();
    assert_eq!(report.status, CollectStatus::Ran);
    assert_eq!(report.outcomes.iter().map(|row| row.outcome.as_str()).collect::<Vec<_>>(), ["ok"]);
    let db = Database::open(&home).unwrap();
    assert_eq!(count(&db, "source_run"), 1);
    assert_eq!(count(&db, "events"), 1);
    assert!(db.connection().is_autocommit());
    drop(db);
    assert!(StateBuilder::read_state(&home).is_some());
    let status = StateBuilder::read_engine_status(&home).unwrap();
    assert_eq!((status.ok, status.error_code.as_deref()), (false, Some("SYNTHETIC_FAILURE")));
    let doctor = diagnose(&home, T1, sources());
    assert!(doctor.problems.iter().any(|line| line.contains("항목 엔진 실패")));
}

#[test]
fn 항목_엔진이_패닉해도_같고_코드는_engine_failed다() {
    let (_dir, home) = temp_home();
    let sources = vec![direct(SourceName::Slack, |_| slack_result(T1, "1"))];
    let report = tick(&home, T1, sources).items(|| panic!("외부 문자열 노출 금지")).run();
    assert_eq!(report.status, CollectStatus::Ran);
    assert_eq!(report.items, None);
    assert_eq!(StateBuilder::read_engine_status(&home).unwrap().error_code.as_deref(), Some("engine_failed"));
    assert!(StateBuilder::read_state(&home).is_some());
}

#[test]
fn p0_알림_새_p0는_collect_끝에서_1회만_알리고_외부_문자열을_넣지_않는다() {
    let (_dir, home) = calendar_home();
    let recorder = Arc::new(Recorder::new());
    let first = tick(&home, T1, vec![calendar_feed()]).notifier(recorder.clone()).run();
    assert_eq!(first.notified, 1);
    assert_eq!(recorder.messages(), ["회의 임박 1건"]);
    let second = tick(&home, T2, vec![calendar_feed()]).notifier(recorder.clone()).run();
    assert_eq!(second.notified, 0);
    assert_eq!(recorder.messages().len(), 1, "중복 알림이 없다");
    let db = Database::open(&home).unwrap();
    let shown: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM suggestions WHERE surface = ?", params!["osascript"], |row| row.get(0))
        .unwrap();
    assert_eq!(shown, 1);
    assert!(item(&db, 1).p0_notified_at.is_some());
}

#[test]
fn p0_알림_실패가_collect를_깨지_않는다() {
    let (_dir, home) = calendar_home();
    let report = tick(&home, T1, vec![calendar_feed()]).notifier(notifier(|_| panic!("합성 실패"))).run();
    assert_eq!(report.status, CollectStatus::Ran);
    assert_eq!(report.notified, 0);
    assert!(StateBuilder::read_state(&home).is_some());
}

#[test]
fn notify를_주지_않은_collect는_osascript를_띄우지_않고_표시도_남기지_않는다() {
    let (_dir, home) = calendar_home();
    let report = tick(&home, T1, vec![calendar_feed()]).run();
    assert_eq!(report.notified, 0);
    assert_eq!(item(&Database::open(&home).unwrap(), 1).p0_notified_at, None);
}
