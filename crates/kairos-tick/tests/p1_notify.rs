//! 중 collect와 doctor를 지나는 시험. 알림 판단 자체는 kairos-agenda의 시험이 맡는다
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::fakes::*;
use common::*;
use kairos_agenda::{
    ControlFile, DisplayGate, DisplayStart, FocusTracker, KairosControl, NativeNotifier, NextContext, NextOptions,
    Presenter,
};
use kairos_core::KairosHome;
use kairos_core::model::{Source, SourceName, SourceResult};
use kairos_core::process::SystemProcessRunner;
use kairos_store::Database;
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

fn scripted() -> Arc<dyn Source> {
    direct(SourceName::Slack, |_| slack_result(T1, "1"))
}

fn collect_home(control: &KairosControl) -> (TempDir, KairosHome) {
    let (dir, home) = temp_home();
    ControlFile::new(&home).write(control).unwrap();
    prime_slack(&Database::open(&home).unwrap());
    (dir, home)
}

fn context_at(now: &str, control: KairosControl) -> NextContext {
    let off_dates = none();
    let start = DisplayStart::compute(Some(BEFORE), at(now), &off_dates).unwrap();
    let gate = DisplayGate::evaluate(&start, &control, at(now));
    NextContext { now: at(now), control, off_dates, gate, health: None }
}

fn p1_notified(home: &KairosHome) -> Option<String> {
    item(&Database::open(home).unwrap(), 1).p1_notified_at
}

#[test]
fn collect_끝에서_알리고_보고에_건수가_남으며_다음_틱은_울리지_않는다() {
    let (_dir, home) = collect_home(&control_on());
    let recorder = Arc::new(Recorder::new());
    let report = tick(&home, T1, vec![scripted()]).notifier(recorder.clone()).run();
    assert_eq!((report.notified_p1, report.notified), (1, 0));
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    let second = tick(&home, T2, vec![scripted()]).notifier(recorder.clone()).run();
    assert_eq!(second.notified_p1, 0);
    assert_eq!(recorder.messages().len(), 1);
}

#[test]
fn 알림이_나간_틱에서만_패널을_열고_못_열어도_틱은_끝까지_돈다() {
    let (_dir, home) = collect_home(&control_on());
    let recorder = Arc::new(Recorder::new());
    let opened = Arc::new(AtomicUsize::new(0));
    let opener = || {
        let opened = opened.clone();
        panel(move || {
            opened.fetch_add(1, Ordering::SeqCst);
            true
        })
    };
    let first = tick(&home, T1, vec![scripted()]).notifier(recorder.clone()).panel(opener()).run();
    assert_eq!((opened.load(Ordering::SeqCst), first.panel_opened), (1, true));
    let second = tick(&home, T2, vec![scripted()]).notifier(recorder.clone()).panel(opener()).run();
    assert_eq!((opened.load(Ordering::SeqCst), second.panel_opened, second.notified_p1), (1, false, 0));

    let (_other_dir, other) = collect_home(&control_on());
    let broken = tick(&other, T1, vec![scripted()]).notifier(recorder).panel(panel(|| panic!("합성 실패"))).run();
    assert_eq!((broken.status, broken.notified_p1, broken.panel_opened), (CollectStatus::Ran, 1, false));
}

#[test]
fn 알림이_실패하면_패널도_열지_않는다() {
    let (_dir, home) = collect_home(&control_on());
    let opened = Arc::new(AtomicUsize::new(0));
    let count = opened.clone();
    let opener = panel(move || {
        count.fetch_add(1, Ordering::SeqCst);
        true
    });
    tick(&home, T1, vec![scripted()]).notifier(Arc::new(Recorder::failing())).panel(opener).run();
    assert_eq!(opened.load(Ordering::SeqCst), 0);
}

#[test]
fn 알림이_던지거나_실패해도_수집_틱은_끝까지_돌고_doctor에_미전달이_나온다() {
    let (_dir, home) = collect_home(&control_on());
    let sources = || vec![scripted()];
    let failing = notifier(|_| panic!("합성 실패"));
    let report = tick(&home, T1, sources()).notifier(failing).run();
    assert_eq!((report.status, report.notified_p1), (CollectStatus::Ran, 0));
    let doctor = diagnose(&home, T1, sources());
    assert!(doctor.problems.iter().any(|line| line == "P1 알림 미전달 1건"), "{:?}", doctor.problems);
    assert!(doctor.lines.iter().any(|line| line == "P1 알림: 켜짐, 보류 0건, 미전달 1건"), "{:?}", doctor.lines);
    let recorder = Arc::new(Recorder::new());
    tick(&home, T2, sources()).notifier(recorder.clone()).run();
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    let after = diagnose(&home, T2, sources());
    assert!(!after.problems.iter().any(|line| line.contains("P1 알림")));
}

#[test]
fn 알림_뒤_기록이_실패해도_수집_틱은_끝까지_돌고_표시를_남기지_않아_다음_틱에_다시_시도한다() {
    let (_dir, home) = collect_home(&control_on());
    let inner = home.clone();
    let break_record = notifier(move |_| {
        Database::open(&inner)
            .unwrap()
            .connection()
            .execute_batch("ALTER TABLE suggestions RENAME TO suggestions_hidden")
            .unwrap();
        true
    });
    let report = tick(&home, T1, vec![scripted()]).notifier(break_record).run();
    assert_eq!((report.status, report.notified_p1), (CollectStatus::Ran, 0));
    assert_eq!(p1_notified(&home), None, "실패한 기록은 통째로 되돌린다");
    Database::open(&home)
        .unwrap()
        .connection()
        .execute_batch("ALTER TABLE suggestions_hidden RENAME TO suggestions")
        .unwrap();
    let recorder = Arc::new(Recorder::new());
    tick(&home, T2, vec![scripted()]).notifier(recorder.clone()).run();
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn doctor는_보류를_문제로_올리지_않는다() {
    let (_dir, home) = collect_home(&FocusTracker::start(control_on(), 50, at(T0)));
    let recorder = Arc::new(Recorder::new());
    tick(&home, T1, vec![scripted()]).notifier(recorder.clone()).run();
    assert!(recorder.messages().is_empty());
    let doctor = diagnose(&home, T1, vec![scripted()]);
    assert!(!doctor.problems.iter().any(|line| line.contains("P1 알림")));
    assert!(doctor.lines.iter().any(|line| line == "P1 알림: 켜짐, 보류 1건, 미전달 0건"), "{:?}", doctor.lines);
}

#[test]
fn 틱_도중에_바뀐_일시정지와_끄기를_따른다() {
    let (_dir, home) = collect_home(&control_on());
    let inner = home.clone();
    let source = direct(SourceName::Slack, move |_| {
        let file = ControlFile::new(&inner);
        file.write(&file.read().with_p1_notify(false)).unwrap();
        slack_result(T1, "1")
    });
    let recorder = Arc::new(Recorder::new());
    tick(&home, T1, vec![source]).notifier(recorder.clone()).run();
    assert!(recorder.messages().is_empty());
}

#[test]
fn 가짜_알림_헬퍼까지_post와_json_한_덩어리로_고정_문구만_넘어간다() {
    let (_dir, home) = collect_home(&control_on());
    let fake = home.file("fake-notifier");
    let log = home.file("argv.txt");
    let script = format!("#!/bin/sh\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{}'; done\n", log.display());
    fs::write(&fake, script).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o700)).unwrap();
    let hostile = direct(SourceName::Slack, |_| {
        let mut rows = slack_result(T1, "1");
        rows[0].events[0].text_head = Some("\" & do shell script \"합성".into());
        rows
    });
    let native = NativeNotifier::new(Arc::new(SystemProcessRunner), fake.to_string_lossy().into_owned());
    let report = tick(&home, T1, vec![hostile]).notifier(Arc::new(native)).run();
    assert_eq!(report.notified_p1, 1);
    let lines = fs::read_to_string(&log).unwrap();
    let argv: Vec<&str> = lines.lines().collect();
    assert_eq!(argv.len(), 2);
    assert_eq!(argv[0], "post");
    let payload: serde_json::Value = serde_json::from_str(argv[1]).unwrap();
    assert_eq!(payload["body"], "Slack 멘션 1건");
    assert!(!argv[1].contains("do shell script"), "외부 문자열을 싣지 않는다");
}

#[test]
fn notify를_주지_않은_collect는_p1을_소비하지_않는다() {
    let (_dir, home) = collect_home(&control_on());
    tick(&home, T1, vec![scripted()]).run();
    assert_eq!(p1_notified(&home), None);
}

#[test]
fn p1_알림을_꺼도_패널은_그대로고_doctor에_꺼짐이_나온다() {
    let control = control_on().with_p1_notify(false);
    let (_dir, home) = collect_home(&control);
    let recorder = Arc::new(Recorder::new());
    tick(&home, T1, vec![scripted()]).notifier(recorder.clone()).run();
    assert!(recorder.messages().is_empty());
    let doctor = diagnose(&home, T1, vec![scripted()]);
    let line = "P1 알림: 꺼짐(켜기: kairos notify on), 보류 0건, 미전달 0건";
    assert!(doctor.lines.iter().any(|row| row == line), "{:?}", doctor.lines);
    let db = Database::open(&home).unwrap();
    let response = Presenter::new(&db).next_response(&context_at(T1, control), NextOptions::default()).unwrap();
    assert_eq!(serde_json::to_value(&response).unwrap()["card"]["tier"], "P1");
}

#[test]
fn p1_알림을_꺼도_p0_알림은_나간다() {
    let (_dir, home) = collect_home(&control_on().with_p1_notify(false));
    prime(&Database::open(&home).unwrap(), SourceName::Calendar, "tick");
    let source = direct(SourceName::Calendar, |_| {
        let object = meeting_at("imminent", "2026-03-02T10:30:00+09:00", "2026-03-02T11:00:00+09:00");
        vec![SourceResult {
            source: SourceName::Calendar,
            started_at: T1.into(),
            finished_at: T1.into(),
            watermark: Some(T1.into()),
            objects: vec![object],
            ..result()
        }]
    });
    let recorder = Arc::new(Recorder::new());
    let report = tick(&home, T1, vec![source]).notifier(recorder.clone()).run();
    assert_eq!(report.notified, 1);
    assert_eq!(recorder.messages(), ["회의 임박 1건"]);
}

/// 정해 둔 판을 답하고 물은 횟수를 센다.
struct Releases {
    latest: Option<kairos_ops::Version>,
    asked: AtomicUsize,
}

impl kairos_ops::ReleaseProbe for Releases {
    fn latest(&self) -> Option<kairos_ops::Version> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        self.latest
    }
}

fn releases(latest: Option<kairos_ops::Version>) -> Arc<Releases> {
    Arc::new(Releases { latest, asked: AtomicUsize::new(0) })
}

/// 지금 판보다 큰 판.
fn newer() -> kairos_ops::Version {
    kairos_ops::Version::parse("999.0.0").unwrap()
}

#[test]
fn 새_판은_하루_한_번_묻고_판마다_한_번만_알린다() {
    let (_dir, home) = collect_home(&control_on());
    let recorder = Arc::new(Recorder::new());
    let probe = releases(Some(newer()));
    let first = tick(&home, T1, vec![]).notifier(recorder.clone()).releases(probe.clone()).run();
    assert_eq!(first.update_notified.as_deref(), Some("999.0.0"));
    let alert = recorder.alerts().pop().unwrap();
    assert_eq!((alert.title.as_str(), alert.items.len()), (kairos_ops::UPDATE_TITLE, 0));
    assert_eq!(alert.body, kairos_ops::update_body(newer()));
    // 같은 날의 다음 틱은 묻지도 알리지도 않는다. 보고에 키가 없다.
    let second = tick(&home, T2, vec![]).notifier(recorder.clone()).releases(probe.clone()).run();
    assert_eq!((second.update_notified.as_deref(), probe.asked.load(Ordering::SeqCst)), (None, 1));
    assert!(!serde_json::to_string(&second).unwrap().contains("updateNotified"));
    assert_eq!(recorder.alerts().len(), 1);
    assert_eq!(kairos_ops::UpdateFile::new(&home).read().notified, Some(newer()));
}

#[test]
fn 새_판_알림은_p1처럼_조용한_때를_따르고_못_보내면_다음_틱에_다시_보낸다() {
    // 표시가 닫혀 있으면 알리지 않지만 확인은 적는다.
    let (_dir, home) = collect_home(&KairosControl::default());
    let recorder = Arc::new(Recorder::new());
    let report = tick(&home, T1, vec![]).notifier(recorder.clone()).releases(releases(Some(newer()))).run();
    assert_eq!((report.update_notified, recorder.alerts().len()), (None, 0));
    assert_eq!(kairos_ops::UpdateFile::new(&home).read().latest, Some(newer()));

    // 전달에 실패하면 알린 판을 적지 않는다. 다음 틱은 묻지 않고 적힌 판으로 다시 알린다.
    let (_other_dir, other) = collect_home(&control_on());
    let failing = Arc::new(Recorder::failing());
    let probe = releases(Some(newer()));
    let failed = tick(&other, T1, vec![]).notifier(failing).releases(probe.clone()).run();
    assert_eq!(failed.update_notified, None);
    let retried = tick(&other, T2, vec![]).notifier(recorder).releases(probe.clone()).run();
    assert_eq!((retried.update_notified.as_deref(), probe.asked.load(Ordering::SeqCst)), (Some("999.0.0"), 1));
}

#[test]
fn 지금_판이_최신이거나_탐침이_없으면_알리지_않는다() {
    let (_dir, home) = collect_home(&control_on());
    let recorder = Arc::new(Recorder::new());
    let same = releases(Some(kairos_ops::Version::current()));
    assert_eq!(tick(&home, T1, vec![]).notifier(recorder.clone()).releases(same).run().update_notified, None);
    let (_other_dir, other) = collect_home(&control_on());
    assert_eq!(tick(&other, T1, vec![]).notifier(recorder.clone()).run().update_notified, None);
    assert!(!kairos_ops::UpdateFile::new(&other).read().ok);
    assert!(recorder.alerts().is_empty());
}
