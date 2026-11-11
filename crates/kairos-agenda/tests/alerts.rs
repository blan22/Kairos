//! P0 알림과 알림 헬퍼 시험, 패널 명령 시험.
//! CLI와 collect를 거치지 않는 시험만 여기 있다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use common::*;
use kairos_agenda::{
    DisplayGate, DisplayMode, KairosControl, NativeNotifier, NotifyReport, OrcaPanelOpener, P0_TITLE, P0Alerts,
    PANEL_OPEN_ARGS,
};
use kairos_core::model::SourceName;
use kairos_core::ports::{Alert, AlertTier, Delivery, Notifier, PanelOpener};
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner, SystemProcessRunner};
use kairos_store::Database;
use serde_json::{Value, json};

fn imminent_meeting() -> Database {
    let db = test_db();
    prime(&db, SourceName::Calendar, "tick");
    let object = meeting_at("imminent", &format!("{MON}T10:30:00+09:00"), &format!("{MON}T11:00:00+09:00"));
    Push::from(SourceName::Calendar, "tick", T1).objects(vec![object]).run(&db);
    refresh(&db, T1);
    db
}

fn open_gate() -> DisplayGate {
    let control = KairosControl::default().with_display_mode(DisplayMode::On);
    gate_at(Some(BEFORE), &control, T1, &none())
}

fn alert(tier: AlertTier, body: &str, items: Vec<i64>) -> Alert {
    Alert { tier, title: "합성 제목".to_owned(), body: body.to_owned(), items }
}

fn fake_bin(dir: &Path, name: &str, log: &Path) -> String {
    let bin = dir.join(name);
    fs::write(&bin, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\n", log.display())).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    bin.to_string_lossy().into_owned()
}

#[test]
fn p0_알림_전달에_실패하면_표시를_남기지_않고_다음_틱에_다시_알린다() {
    let db = imminent_meeting();
    let gate = open_gate();
    let failing = Recorder::failing();
    assert_eq!(P0Alerts::new(&db, &failing).notify(at(T1), &gate).unwrap(), NotifyReport { sent: 0, failed: 1 });
    assert_eq!(item(&db, 1).p0_notified_at, None);
    let working = Recorder::new();
    let alerts = P0Alerts::new(&db, &working);
    assert_eq!(alerts.notify(at(T2), &gate).unwrap(), NotifyReport { sent: 1, failed: 0 });
    assert_eq!(failing.messages().len() + working.messages().len(), 2);
    assert_eq!(alerts.notify(at(T3), &gate).unwrap(), NotifyReport { sent: 0, failed: 0 });
    assert_eq!(working.messages(), ["회의 임박 1건"]);
    let sent = &working.alerts()[0];
    assert_eq!((sent.tier, sent.title.as_str(), sent.items.clone()), (AlertTier::P0, P0_TITLE, vec![1]));
}

#[test]
fn p0_알림_표시_게이트가_닫혀_있으면_알리지_않고_항목은_남는다_여는_뒤_살아_있으면_알린다() {
    let db = imminent_meeting();
    let recorder = Recorder::new();
    let closed = gate_at(Some(BEFORE), &KairosControl::default(), T1, &none());
    assert!(!closed.open);
    assert_eq!(P0Alerts::new(&db, &recorder).notify(at(T1), &closed).unwrap(), NotifyReport::default());
    assert!(recorder.messages().is_empty());
    assert_eq!(item(&db, 1).p0_notified_at, None);
}

#[test]
fn p0_알림_닫힌_항목과_대기_항목은_알리지_않고_알림은_건수만_합친다() {
    let db = imminent_meeting();
    db.connection()
        .execute_batch("UPDATE items SET state = 'closed', closed_at = 'x', close_reason = 'user_done'")
        .unwrap();
    let recorder = Recorder::new();
    assert_eq!(P0Alerts::new(&db, &recorder).notify(at(T2), &open_gate()).unwrap(), NotifyReport::default());
    assert!(recorder.messages().is_empty(), "부르면 안 된다");
    assert_eq!(P0Alerts::message(2, 1), "회의 임박 2건, 배포 당일 위험 1건");
}

#[test]
fn 패널_명령의_인자는_고정이고_셸을_거치지_않는다() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("fake-orca.log");
    let bin = fake_bin(dir.path(), "fake-orca", &log);
    assert!(OrcaPanelOpener::with_bin(Arc::new(SystemProcessRunner), bin).open());
    let argv = fs::read_to_string(&log).unwrap();
    assert_eq!(argv.trim().split('\n').collect::<Vec<_>>(), PANEL_OPEN_ARGS);
}

struct RecordingRunner {
    requests: Mutex<Vec<ProcessRequest>>,
}

impl ProcessRunner for RecordingRunner {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        self.requests.lock().unwrap().push(request.clone());
        ProcessOutput::exited(1, "")
    }
}

#[test]
fn 패널_명령은_루트에서_띄운다() {
    // 원본에 없는 시험이다. execFile의 timeout과 cwd 인자를 고정한다.
    let runner = Arc::new(RecordingRunner { requests: Mutex::new(Vec::new()) });
    assert!(!OrcaPanelOpener::new(runner.clone()).open());
    let requests = runner.requests.lock().unwrap();
    assert_eq!(requests[0].program, "/Applications/Orca.app/Contents/Resources/bin/orca");
    assert_eq!(requests[0].cwd.as_deref(), Some(Path::new("/")));
}

#[test]
fn 알림_헬퍼에는_post와_json_한_덩어리만_넘긴다() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("argv.log");
    let fake = fake_bin(dir.path(), "fake-notifier", &log);
    let notifier = NativeNotifier::new(Arc::new(SystemProcessRunner), fake);
    assert!(notifier.notify(&alert(AlertTier::P1, "Slack 멘션 1건", vec![3, 7])));
    let argv = fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = argv.trim_end().split('\n').collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0], "post");
    let payload: Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(
        payload,
        json!({ "title": "합성 제목", "body": "Slack 멘션 1건", "items": [3, 7], "tier": "P1", "thread": "kairos.p1" })
    );
    let p0: Value =
        serde_json::from_str(&NativeNotifier::payload(&alert(AlertTier::P0, "회의 임박 1건", vec![1]))).unwrap();
    assert_eq!((p0["tier"].as_str(), p0["thread"].as_str()), (Some("P0"), Some("kairos.p0")));
}

#[test]
fn 알림_헬퍼는_종료_0만_게시로_보고_시간_제한은_5초다() {
    let runner = Arc::new(RecordingRunner { requests: Mutex::new(Vec::new()) });
    assert!(!NativeNotifier::new(runner.clone(), "/synthetic/KairosNotifier").notify(&alert(
        AlertTier::P0,
        "합성",
        vec![1]
    )));
    let requests = runner.requests.lock().unwrap();
    assert_eq!(requests[0].program, "/synthetic/KairosNotifier");
    assert_eq!(requests[0].timeout, std::time::Duration::from_secs(5));
}

/// 정해 둔 모양으로 끝나는 실행기.
struct Ending(ProcessEnd);

impl ProcessRunner for Ending {
    fn run(&self, _request: &ProcessRequest) -> ProcessOutput {
        ProcessOutput::ended(self.0)
    }
}

#[test]
fn 알림_헬퍼는_게시하지_않은_것이_확실한_실패와_모르는_실패를_나눈다() {
    let sent = alert(AlertTier::P0, "합성", vec![1]);
    for (end, expected) in [
        (ProcessEnd::Exited(0), Delivery::Posted),
        (ProcessEnd::Exited(2), Delivery::NotPosted),
        (ProcessEnd::Exited(3), Delivery::NotPosted),
        (ProcessEnd::Exited(5), Delivery::NotPosted),
        (ProcessEnd::Missing, Delivery::NotPosted),
        (ProcessEnd::SpawnFailed, Delivery::NotPosted),
        (ProcessEnd::Signaled, Delivery::NotPosted),
        (ProcessEnd::Exited(4), Delivery::Unknown),
        (ProcessEnd::TimedOut, Delivery::Unknown),
        (ProcessEnd::Cancelled, Delivery::Unknown),
        (ProcessEnd::OutputTooLarge, Delivery::Unknown),
    ] {
        let notifier = NativeNotifier::new(Arc::new(Ending(end)), "/synthetic/KairosNotifier");
        assert_eq!(notifier.deliver(&sent), expected, "{end:?}");
        assert_eq!(notifier.notify(&sent), expected == Delivery::Posted, "{end:?}");
    }
}

#[test]
fn p0_알림_알린_회의가_다시_임박_시각으로_옮겨지면_또_알린다() {
    let db = imminent_meeting();
    let notifier = Recorder::new();
    let alerts = P0Alerts::new(&db, &notifier);
    assert_eq!(alerts.notify(at(T1), &open_gate()).unwrap().sent, 1);
    assert_eq!(alerts.notify(at(T1), &open_gate()).unwrap().sent, 0);

    let object = meeting_at("imminent", &format!("{MON}T10:40:00+09:00"), &format!("{MON}T11:10:00+09:00"));
    Push::from(SourceName::Calendar, "tick", T2).objects(vec![object]).run(&db);
    refresh(&db, T2);
    assert_eq!(alerts.notify(at(T2), &open_gate()).unwrap().sent, 1);
}
