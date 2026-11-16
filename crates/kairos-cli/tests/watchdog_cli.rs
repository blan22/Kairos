//! `kairos watchdog`. Linux와 Windows의 감시자. 판정 장면은 kairos-ops의 watchdog 시험이 두 구현에 함께 돌린다.
//! 여기서는 명령의 연결(시각, dry, 알림, 출력)만 본다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;

use common::*;

/// 월요일 10:00 KST.
const MONDAY_10: &str = "1772413200";

#[test]
fn watchdog는_dry면_알릴_문구만_찍고_같은_경고는_다시_찍지_않는다() {
    let cli = Cli::new();
    let env = [("KAIROS_WATCHDOG_NOW", MONDAY_10), ("KAIROS_WATCHDOG_DRY", "1")];
    let done = cli.run_with(&env, &["watchdog"]);
    assert_eq!(done.status, 0, "{}", done.err);
    assert_eq!(done.out.trim(), "알림: kairos 데이터베이스 없음");
    assert_eq!(cli.run_with(&env, &["watchdog"]).out.trim(), "");
}

#[test]
fn watchdog는_알림이_실패하면_코드만_찍고_성공하면_기록한다() {
    let mut cli = Cli::new();
    let env = [("KAIROS_WATCHDOG_NOW", MONDAY_10)];
    let fail = cli.root.join("fake-notifier-fail");
    write_script(&fail, "#!/bin/sh\nexit 2\n");
    cli.set("KAIROS_NOTIFIER", fail.to_str().unwrap());
    let done = cli.run_with(&env, &["watchdog"]);
    assert_eq!((done.status, done.out.trim()), (0, "notify_failed 1"), "{}", done.err);
    assert!(!cli.root.join("watchdog.last").exists());

    let (pass, log) = (cli.root.join("fake-notifier"), cli.root.join("notifier.log"));
    write_script(&pass, &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n", log.display()));
    cli.set("KAIROS_NOTIFIER", pass.to_str().unwrap());
    let done = cli.run_with(&env, &["watchdog"]);
    assert_eq!((done.status, done.out.trim()), (0, ""), "{}", done.err);
    let argv = fs::read_to_string(&log).unwrap();
    assert!(argv.contains("kairos 데이터베이스 없음"), "{argv}");
    assert_eq!(fs::read_to_string(cli.root.join("watchdog.last")).unwrap(), "kairos 데이터베이스 없음\n");
}

#[test]
fn watchdog는_인자를_받지_않는다() {
    let cli = Cli::new();
    assert_ne!(cli.run(&["watchdog", "--now", "1"]).status, 0);
}
