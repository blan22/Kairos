//! 알림의 프로세스 시험 2건.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use serde_json::json;

#[test]
fn kairos_notify_on_off는_control에_남고_기본은_켜짐이며_다른_설정을_지우지_않는다() {
    let cli = Cli::new();
    cli.open_sessions();
    let read = |args: &[&str]| cli.json(&[&["notify"], args].concat()).body();
    assert_eq!(read(&[])["notify"], json!({ "p1": true }));
    assert_eq!(read(&["off"])["notify"], json!({ "p1": false }));
    let file = cli.control_json();
    assert_eq!(file["p1NotifyOff"], true);
    assert_eq!((&file["sessionTestPassedAt"], &file["displayMode"]), (&json!(T0), &json!("on")));
    assert_eq!(cli.run(&["pause", "5"]).status, 0);
    assert_eq!(cli.control_json()["p1NotifyOff"], true, "다른 명령이 저장해도 남는다");
    assert_eq!(read(&["on"])["notify"], json!({ "p1": true }));
    assert!(cli.control_json().get("p1NotifyOff").is_none());
    assert!(cli.run(&["notify", "off"]).out.contains("P1 알림 꺼짐"));
    assert!(cli.run(&["notify", "on"]).out.contains("P1 알림 켜짐"));
    let bad = cli.run(&["notify", "maybe", "--json"]);
    assert_eq!((bad.status, &bad.body()["error"]["code"]), (2, &json!("usage")));
}

#[test]
fn next는_패널_표면만_받는다_알림은_카드_예산_계약을_쓰지_않는다() {
    let cli = Cli::new();
    assert_eq!(cli.run(&["next", "--seen", "--surface", "panel", "--json"]).status, 0);
    let bad = cli.run(&["next", "--seen", "--surface", "notification", "--json"]);
    assert_eq!((bad.status, &bad.body()["error"]["code"]), (2, &json!("usage")));
}

/// Linux는 notify-send로 보낸다. 앱별 권한 창이 없어 setup은 알리기만 한다.
#[cfg(target_os = "linux")]
#[test]
fn linux의_notify_setup은_권한을_묻지_않고_test는_notify_send로_보낸다() {
    let mut cli = Cli::new();
    let (bin, log) = (cli.root.join("fake-notify-send"), cli.root.join("notify-send.log"));
    write_script(&bin, &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n", log.display()));
    cli.set("KAIROS_NOTIFIER", bin.to_str().unwrap());
    let setup = cli.run(&["notify", "setup"]);
    assert_eq!(
        (setup.status, setup.out.as_str()),
        (0, "이 OS의 알림은 따로 권한을 받지 않습니다. kairos notify test로 확인하세요\n")
    );
    assert!(!log.exists(), "setup은 알림을 보내지 않는다");
    let test = cli.json(&["notify", "test"]);
    assert_eq!(test.status, 0, "{}", test.err);
    assert_eq!(test.body()["notify"]["test"], json!("posted"));
    let argv = std::fs::read_to_string(&log).unwrap();
    assert!(argv.starts_with("--app-name=kairos\n--urgency=normal\n--\nkairos\n시험 알림입니다."), "{argv}");

    cli.set("KAIROS_NOTIFIER", cli.root.join("missing").to_str().unwrap());
    assert_eq!(cli.json(&["notify", "test"]).body()["error"]["code"], json!("notifier_missing"));
}
