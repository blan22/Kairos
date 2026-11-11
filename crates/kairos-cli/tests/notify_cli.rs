//! 알림의 프로세스 시험 2건.
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
