//! 틱과 doctor가 함께 도는 시험이다. 판정 함수는 kairos-ops의 시험이 맡는다
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_agenda::{HealthStatus, SourceHealthBook};
use kairos_core::KairosHome;
use kairos_core::model::{Source, SourceName, SourceResult};
use kairos_ops::{ResumeFile, SLEEP_INTERRUPTED, counts_as_failure};
use kairos_store::Database;
use kairos_tick::{CollectReport, OutcomeRow};

fn fine() -> Arc<dyn Source> {
    direct(SourceName::Github, |_| {
        vec![SourceResult { source: SourceName::Github, objects: vec![issue(1)], parsed_count: 1, ..result() }]
    })
}

fn slack_hung() -> Arc<dyn Source> {
    hung(SourceName::Slack)
}

fn cursor_count(home: &KairosHome) -> i64 {
    let db = Database::open(home).unwrap();
    db.connection().query_row("SELECT count(*) FROM cursor WHERE source = 'slack'", [], |row| row.get(0)).unwrap()
}

fn failures(report: &CollectReport) -> usize {
    let failed = |row: &&OutcomeRow| counts_as_failure(row.outcome, row.error_code.as_deref());
    report.outcomes.iter().filter(failed).count()
}

#[test]
fn 얼어_있던_틱의_시간_초과는_잠자기_코드로_남고_outcome과_cursor는_그대로다() {
    let (_dir, home) = temp_home();
    let report = tick(&home, T1, vec![slack_hung(), fine()]).limit(30).sleep(frozen(1_619_000)).run();
    assert!(report.slept);
    let rows = run_rows(&home);
    let slack: Vec<_> = rows.iter().filter(|row| row.0 == "slack").collect();
    assert_eq!(slack.len(), 1);
    assert_eq!((slack[0].1.as_str(), slack[0].2.as_deref()), ("timeout", Some(SLEEP_INTERRUPTED)));
    assert!(rows.iter().filter(|row| row.0 == "github").all(|row| row.1 == "ok"));
    assert_eq!(cursor_count(&home), 0, "cursor는 움직이지 않는다");
    let reported = report.outcomes.iter().find(|row| row.source == SourceName::Slack).unwrap();
    assert_eq!(reported.error_code.as_deref(), Some(SLEEP_INTERRUPTED));
    assert_eq!(failures(&report), 0);
    assert!(ResumeFile::new(&home).read().is_some(), "깨어난 시각을 남긴다");
}

#[test]
fn 대조_깨어_있는_동안_멈춘_소스는_그대로_고장이다() {
    let (_dir, home) = temp_home();
    let report = tick(&home, T1, vec![slack_hung()]).limit(30).sleep(frozen(1_200)).run();
    assert!(!report.slept);
    let rows = run_rows(&home);
    assert_eq!((rows[0].1.as_str(), rows[0].2.as_deref()), ("timeout", Some("source_timeout")));
    assert_eq!(failures(&report), 1);
    assert_eq!(ResumeFile::new(&home).read(), None);
    let doctor = diagnose(&home, T2, vec![slack_hung()]);
    assert!(doctor.problems.iter().any(|problem| problem.contains("마지막 수집 timeout")));
}

#[test]
fn 대조_잠자기가_있었어도_시간_초과가_아닌_실패는_숨기지_않는다() {
    let (_dir, home) = temp_home();
    let broken = direct(SourceName::Slack, |_| panic!("합성 오류"));
    let report = tick(&home, T1, vec![broken]).limit(30).sleep(frozen(1_619_000)).run();
    let rows = run_rows(&home);
    assert_eq!((rows[0].1.as_str(), rows[0].2.as_deref()), ("parse_error", Some("source_threw")));
    assert_eq!(failures(&report), 1);
}

#[test]
fn 이십초_안에_끝났어도_os의_잠자기_횟수가_늘었으면_잠자기다() {
    let (_dir, home) = temp_home();
    tick(&home, T1, vec![slack_hung()]).limit(30).sleep(counted(3_000, vec![41, 42])).run();
    assert_eq!(run_rows(&home)[0].2.as_deref(), Some(SLEEP_INTERRUPTED));
}

#[test]
fn doctor_잠자기_코드는_문제가_아니라_안내로_나온다() {
    let (_dir, home) = temp_home();
    tick(&home, T1, vec![slack_hung()]).limit(30).sleep(frozen(600_000)).run();
    let report = diagnose(&home, T2, vec![slack_hung()]);
    assert!(!report.problems.iter().any(|problem| problem.contains("마지막 수집")));
    assert!(report.lines.iter().any(|line| line.contains(SLEEP_INTERRUPTED)));
}

/// pmset 출력이 틱의 판정까지 이어지는지 본다. 가짜 측정 없이 실제 경로로 돈다.
#[test]
fn 측정을_주입하지_않으면_pmset의_횟수로_판정한다() {
    let (_dir, home) = temp_home();
    let calls = std::sync::Mutex::new(vec![7, 8]);
    let pmset = runner(move |request| {
        assert_eq!(request.program, "/usr/bin/pmset");
        let count = calls.lock().unwrap().remove(0);
        kairos_core::process::ProcessOutput::exited(0, format!("Sleep Count:{count}\n"))
    });
    let report = tick(&home, T1, Vec::new()).process_runner(pmset).forced();
    assert!(report.slept);
}

#[test]
fn 건강_깨어난_뒤_허용_지연까지는_지연으로_치지_않고_그_뒤에도_ok가_없으면_지연이다() {
    let db = test_db();
    let old = "2026-03-02T01:00:00.000Z";
    ingest(&db, SourceResult { source: SourceName::Slack, ..result() }, old);
    let source = slack_hung();
    let sources = [source.as_ref()];
    let wake = "2026-03-02T02:00:00.000Z";
    let book = SourceHealthBook::new(db.connection());
    let health = |minutes: i64, resumed: Option<&str>| {
        book.source_health(&sources, at(&plus_minutes(wake, minutes)), &none(), resumed).unwrap()
    };
    assert_eq!(health(1, Some(wake))[0].status, HealthStatus::Ok);
    assert_eq!(SourceHealthBook::health_line(&health(9, Some(wake))), None);
    assert_eq!(health(11, Some(wake))[0].status, HealthStatus::Delayed, "실제로 멈춘 수집은 숨기지 않는다");
    assert_eq!(health(1, None)[0].status, HealthStatus::Delayed, "잠자기 기록이 없으면 지연이다");
    let future = plus_minutes(wake, 30);
    assert_eq!(health(1, Some(&future))[0].status, HealthStatus::Delayed, "미래 시각은 믿지 않는다");
}
