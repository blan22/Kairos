//! 명령의 프로세스 시험(R26, R27). R28은 kairos-tick에도 있고 여기서는 실제 명령으로 본다.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_agenda::{DisplayMode, KairosControl};
use kairos_core::model::SourceName;
use serde_json::json;

fn home_with_issue() -> Cli {
    let cli = Cli::new();
    seed_issue(&cli.db(), None);
    cli
}

fn display_on() -> KairosControl {
    KairosControl::default().with_display_mode(DisplayMode::On)
}

#[test]
fn r26_차단_시험_표시가_없으면_accept가_세션을_열지_않고_시작_버튼도_없다() {
    let mut cli = home_with_issue();
    let orca = cli.fake_orca();
    cli.write_control(&display_on());
    let done = cli.run(&["accept", "1", "--json"]);
    let body = done.body();
    assert_eq!(done.status, 1);
    assert_eq!(body["error"]["code"], "not_eligible");
    assert_eq!(body["url"], issue_url(1));
    assert!(!orca.called(), "orca를 부르지 않는다");
    let list = cli.run(&["list", "--json"]).body();
    let actions = list["items"][0]["actions"].as_array().unwrap();
    assert!(!actions.iter().any(|action| action["id"] == "start"));
}

#[test]
fn r26_kairos_sessions_on은_사용자가_켜고_세션_안에서는_거절한다() {
    let mut cli = home_with_issue();
    let orca = cli.fake_orca();
    cli.write_control(&display_on());
    let inside = cli.run_with(&[("CLAUDECODE", "1")], &["sessions", "on", "--json"]);
    assert_eq!(inside.status, 1);
    assert_eq!(inside.body()["error"]["code"], "session_caller");
    assert_eq!(cli.control().session_test_passed_at(), None);
    let on = cli.run(&["sessions", "on", "--json"]);
    assert_eq!(on.status, 0);
    assert_eq!(on.body()["sessions"]["enabled"], true);
    assert_eq!(cli.run(&["accept", "1", "--json"]).status, 0);
    assert!(orca.called());
    assert_eq!(cli.run(&["sessions", "off", "--json"]).body()["sessions"]["enabled"], false);
    assert_eq!(cli.control().session_test_passed_at(), None);
}

#[test]
fn r27_게이트가_닫혀_있으면_accept가_세션을_열지_않는다() {
    let mut cli = home_with_issue();
    let orca = cli.fake_orca();
    let closed = [
        KairosControl::default().with_display_mode(DisplayMode::Off).with_sessions(Some(T0)),
        display_on().with_paused_until(Some("indefinite")).with_sessions(Some(T0)),
    ];
    for control in closed {
        cli.write_control(&control);
        let done = cli.run(&["accept", "1", "--json"]);
        assert_eq!((done.status, &done.body()["error"]["code"]), (1, &json!("gate_closed")));
    }
    assert!(!orca.called());
}

#[test]
fn r27_세션_안에서_부른_accept는_거절한다_되돌리기와_이동은_막지_않는다() {
    let mut cli = home_with_issue();
    let orca = cli.fake_orca();
    cli.open_sessions();
    for env in [("CLAUDECODE", "1"), ("ORCA_TERMINAL_HANDLE", "term_session")] {
        let done = cli.run_with(&[env], &["accept", "1", "--json"]);
        assert_eq!((done.status, &done.body()["error"]["code"]), (1, &json!("session_caller")), "{env:?}");
    }
    assert!(!orca.called());
    assert_eq!(cli.run(&["accept", "1", "--json"]).status, 0);
    assert_eq!(cli.run_with(&[("CLAUDECODE", "1")], &["accept", "1", "--cancel", "--json"]).status, 0);
}

#[test]
fn r28_doctor가_세션_잠금_상태를_보여_준다() {
    let mut cli = home_with_issue();
    cli.fake_orca();
    cli.write_control(&KairosControl::default());
    assert!(cli.run(&["doctor"]).out.contains("세션: 잠김"));
}

#[test]
fn 패널_워커처럼_세션_값이_없으면_sessions_on이_켜진다_빈_값은_없는_값이다() {
    let cli = Cli::new();
    prime(&cli.db(), SourceName::Slack, BEFORE);
    let done =
        cli.run_with(&[("CLAUDECODE", ""), ("ORCA_TERMINAL_HANDLE", "term_session")], &["sessions", "on", "--json"]);
    assert_eq!(done.status, 0, "켜기는 CLAUDECODE만 본다");
}
