//! 감시자. 실제 ops/kairos-watchdog.zsh를 임시 홈으로 돌린다
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use common::*;
use kairos_core::Instant;
use kairos_core::model::{Outcome, SourceName, SourceResult};
use kairos_core::ports::{Clock, SystemClock};
use kairos_ops::ResumeFile;
use regex::Regex;

const START_ISO: &str = "2026-03-02T01:00:00.000Z"; // 월요일 10:00 KST

fn start() -> i64 {
    at(START_ISO).millis()
}

fn matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).unwrap().is_match(text)
}

fn write_watch(home: &Path, content: &str) {
    fs::write(home.join("watch.tsv"), content).unwrap();
    fs::set_permissions(home.join("watch.tsv"), fs::Permissions::from_mode(0o600)).unwrap();
}

fn script(path: &Path, content: &str) {
    let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o700).open(path).unwrap();
    std::io::Write::write_all(&mut file, content.as_bytes()).unwrap();
}

/// 그 시각에 멈춘 수집기가 있는 홈.
fn stopped(last_run: &str) -> TestHome {
    let test = TestHome::new();
    let db = test.open_db();
    let run = SourceResult { query: "tick".into(), ..result_at(last_run) };
    ingest(&db, run, last_run);
    drop(db);
    write_watch(test.path(), "active\t1\nsource\tgithub\ttick\t600\n");
    test
}

fn empty_db() -> TestHome {
    let test = TestHome::new();
    drop(test.open_db());
    test
}

fn not_connected(when: &str) -> SourceResult {
    SourceResult {
        source: SourceName::Notion,
        query: "q".into(),
        outcome: Outcome::NotConnected,
        watermark: None,
        ..result_at(when)
    }
}

#[test]
fn 감시자_자기_실행_간격이_벌어지면_잠자기로_보고_깨어난_직후에는_알리지_않는다() {
    let test = stopped("2026-03-02T00:58:00.000Z");
    let home = test.path();
    assert_eq!(watchdog(home, start()), "");
    assert_eq!(watchdog(home, start() + 59 * MINUTE_MS), "", "59분 뒤 처음 도는 실행");
    assert!(ResumeFile::new(&test.home).path().exists());
    assert_eq!(watchdog(home, start() + 64 * MINUTE_MS), "", "깨어난 뒤 허용 지연 안");
    assert!(watchdog(home, start() + 70 * MINUTE_MS).contains("멈춤"), "깨어난 뒤에도 수집이 없으면 알린다");
}

#[test]
fn 감시자_대조_깨어_있는_동안_5분마다_돌았으면_멈춘_수집을_그대로_알린다() {
    let test = stopped("2026-03-02T00:58:00.000Z");
    let seen: Vec<String> =
        (0..=15).step_by(5).map(|minute| watchdog(test.path(), start() + minute * MINUTE_MS)).collect();
    assert_eq!(seen[..2].concat(), "");
    assert!(matches(r"수집기 [0-9]+분째 멈춤", &seen.join("\n")));
    assert!(!ResumeFile::new(&test.home).path().exists());
}

#[test]
fn 감시자_수집이_남긴_깨어난_시각도_같은_기준이다() {
    let test = stopped("2026-03-02T00:58:00.000Z");
    let home = test.path();
    watchdog(home, start());
    watchdog(home, start() + 5 * MINUTE_MS);
    ResumeFile::new(&test.home).write(Instant::from_millis(start() + 9 * MINUTE_MS)).unwrap();
    assert_eq!(watchdog(home, start() + 10 * MINUTE_MS), "");
    assert_eq!(watchdog(home, start() + 15 * MINUTE_MS), "");
    assert!(watchdog(home, start() + 20 * MINUTE_MS).contains("멈춤"));
}

#[test]
fn 감시자_마지막_run_행이_없으면_경고하고_같은_경고는_반복하지_않는다() {
    let test = empty_db();
    assert!(watchdog(test.path(), start()).contains("수집 기록 없음"));
    assert_eq!(watchdog(test.path(), start() + 300_000), "");
}

#[test]
fn 감시자_알림이_실패하면_기록하지_않고_다음_판정에서_다시_알린다_로그에는_코드만_남긴다() {
    let test = empty_db();
    let fail = test.join("fake-notifier");
    script(&fail, "#!/bin/sh\nexit 7\n");
    let fail_text = fail.to_string_lossy();
    let out = run_watchdog(test.path(), start(), &[("KAIROS_WATCHDOG_NOTIFIER", &fail_text)]);
    assert_eq!(out, "notify_failed 7");
    assert!(!test.join("watchdog.last").exists());

    let missing = test.join("missing-notifier");
    let missing_text = missing.to_string_lossy();
    let out = run_watchdog(test.path(), start() + 300_000, &[("KAIROS_WATCHDOG_NOTIFIER", &missing_text)]);
    assert_eq!(out, "notify_failed 127", "헬퍼가 없으면 osascript로 넘기지 않는다");

    let pass = test.join("fake-notifier-ok");
    let log = test.join("argv.log");
    script(&pass, &format!("#!/bin/sh\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{}'; done\n", log.display()));
    let pass_text = pass.to_string_lossy();
    let out = run_watchdog(test.path(), start() + 600_000, &[("KAIROS_WATCHDOG_NOTIFIER", &pass_text)]);
    assert_eq!(out, "");
    assert!(fs::read_to_string(test.join("watchdog.last")).unwrap().contains("수집 기록 없음"));
    let argv = fs::read_to_string(&log).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    assert_eq!(argv[0], "post");
    let payload: serde_json::Value = serde_json::from_str(argv[1]).unwrap();
    assert_eq!(payload["body"], "kairos 수집 기록 없음");
    assert_eq!(payload["thread"], "kairos.watchdog");
    assert_eq!(payload["items"], serde_json::json!([]));
}

#[test]
fn 감시자_window_파일의_요일과_시간에만_판정한다() {
    let test = empty_db();
    let saturday = at("2026-03-07T01:00:00.000Z").millis(); // 토요일 10:00 KST
    let night = at("2026-03-02T13:00:00.000Z").millis(); // 월요일 22:00 KST
    assert_eq!(watchdog(test.path(), saturday), "", "기본은 월~금");
    fs::write(test.join("watch-window.tsv"), "0\t1440\t1234567\n").unwrap();
    assert!(watchdog(test.path(), saturday).contains("수집 기록 없음"), "주말도 받는 설정");
    let _ = fs::remove_file(test.join("watchdog.last"));
    assert!(watchdog(test.path(), night).contains("수집 기록 없음"), "하루 종일 받는 설정");
    fs::write(test.join("watch-window.tsv"), "600\t540\t12345\n").unwrap();
    assert_eq!(watchdog(test.path(), night), "", "틀린 줄은 기본값으로 본다");
}

#[test]
fn 감시자_수집기가_멈추면_소스별_지연은_따로_적지_않고_같은_문구는_한_번만_적는다() {
    let test = stopped("2026-03-02T00:38:00.000Z");
    write_watch(test.path(), "active\t1\nsource\tgithub\ttick\t600\nsource\tgithub\tother\t600\n");
    let out = watchdog(test.path(), at("2026-03-02T01:00:00.000Z").millis());
    assert!(matches(r"^알림: kairos 수집기 [0-9]+분째 멈춤$", &out), "{out}");
}

#[test]
fn 감시자_한_소스의_여러_질의가_같은_지연이면_한_번만_적는다() {
    let test = TestHome::new();
    let db = test.open_db();
    let iso = "2026-03-02T01:55:00.000Z";
    ingest(&db, not_connected(iso), iso);
    write_watch(test.path(), "active\t1\nsource\tgithub\ttick\t600\nsource\tgithub\tother\t600\n");
    let out = watchdog(test.path(), at("2026-03-02T02:00:00.000Z").millis());
    assert_eq!(out.matches("github").count(), 1, "{out}");
}

#[test]
fn 감시자_정상이면_조용하고_멈추면_알리고_복구_뒤_다시_멈추면_또_알린다() {
    let test = TestHome::new();
    let db = test.open_db();
    let home = test.path();
    ingest(&db, not_connected("2026-03-02T00:58:00.000Z"), "2026-03-02T00:58:00.000Z");
    let now = start();
    // 감시자는 5분마다 돈다. 그보다 벌어지면 잠자기로 본다.
    assert_eq!(watchdog(home, now), "");
    assert_eq!(watchdog(home, now + 5 * MINUTE_MS), "");
    assert!(matches(r"수집기 [0-9]+분째 멈춤", &watchdog(home, now + 10 * MINUTE_MS)));
    assert_eq!(watchdog(home, now + 15 * MINUTE_MS), "");
    ingest(&db, not_connected("2026-03-02T01:18:00.000Z"), "2026-03-02T01:18:00.000Z");
    assert_eq!(watchdog(home, now + 20 * MINUTE_MS), "");
    assert_eq!(watchdog(home, now + 25 * MINUTE_MS), "");
    assert!(watchdog(home, now + 30 * MINUTE_MS).contains("멈춤"));
}

#[test]
fn 감시자_주말_근무_시간_밖_쉬는_날에는_판정하지_않는다() {
    let test = empty_db();
    assert_eq!(watchdog(test.path(), at("2026-03-07T01:00:00.000Z").millis()), "");
    assert_eq!(watchdog(test.path(), at("2026-03-02T12:00:00.000Z").millis()), "");
    write_watch(test.path(), "active\t0\n");
    // 파일 수정 시각과 같은 KST 날짜의 10:00이어야 한다. 원본은 UTC 날짜라 KST 오전에 돌리면 어긋난다.
    let today = SystemClock.now().local_date();
    assert_eq!(watchdog(test.path(), at(&format!("{today}T01:00:00.000Z")).millis()), "");
}

#[test]
fn 감시자_소스별_ok가_허용_지연을_넘으면_소스_이름으로_알린다() {
    let test = TestHome::new();
    let db = test.open_db();
    let iso = "2026-03-02T01:55:00.000Z";
    let auth = SourceResult { query: "tick".into(), outcome: Outcome::Auth, watermark: None, ..result_at(iso) };
    ingest(&db, auth, iso);
    ingest(&db, not_connected(iso), iso);
    write_watch(test.path(), "active\t1\nsource\tgithub\ttick\t600\nsource\tbad name\t\t600\n");
    let out = watchdog(test.path(), at("2026-03-02T02:00:00.000Z").millis());
    assert!(matches(r"github [0-9]+분 지연", &out));
    assert!(!out.contains("bad"));
}
