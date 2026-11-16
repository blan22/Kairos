//! 합성 시각과 가짜 실행기만 쓴다. 측정값은 .codex/runs/2026-11-04-stage2.md에 있다
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use common::*;
use kairos_core::Instant;
use kairos_core::model::Outcome;
use kairos_core::ports::Clock;
use kairos_ops::{
    ResumeFile, SLEEP_GAP_MS, SLEEP_INTERRUPTED, StallWatch, TickClockRepo, counts_as_failure, parse_sleep_count,
    parse_suspend_success, slept_during,
};

#[derive(Default)]
struct SteppingClock(AtomicI64);

impl SteppingClock {
    fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

impl Clock for SteppingClock {
    fn now(&self) -> Instant {
        Instant::from_millis(self.0.load(Ordering::SeqCst))
    }
}

#[test]
fn 판정_표_얼어_있던_시간과_잠자기_횟수() {
    assert!(slept_during(SLEEP_GAP_MS, None, None));
    assert!(!slept_during(SLEEP_GAP_MS - 1, None, None));
    assert!(!slept_during(0, Some(3), Some(3)));
    assert!(slept_during(0, Some(3), Some(4)));
    assert!(!slept_during(0, Some(9), Some(0)), "재부팅으로 줄어든 값은 잠자기가 아니다");
    assert!(!slept_during(0, None, Some(4)));
}

#[test]
fn 측정한_틱의_시간을_재생한다_268초_115초_1619초_틱은_잠자기다() {
    for seconds in [268, 115, 1619] {
        assert!(slept_during(seconds * 1000, None, None), "{seconds}");
    }
}

#[test]
fn 심장박동은_가장_긴_공백을_잰다() {
    let clock = Arc::new(SteppingClock(AtomicI64::new(1_000)));
    let watch = StallWatch::new(clock.clone());
    for step in [1_000, 1_000, 268_000, 1_000] {
        clock.advance(step);
        watch.beat();
    }
    assert_eq!(watch.max_gap_ms(), 268_000);
    clock.advance(500_000);
    assert_eq!(watch.max_gap_ms(), 500_000, "마지막 박동 뒤의 공백도 센다");
}

#[test]
fn pmset_출력에서_잠자기_횟수를_읽고_모르면_none이다() {
    assert_eq!(parse_sleep_count("Sleep Count:12\nDark Wake Count:3\nUser Wake Count:1\n"), Some(12));
    assert_eq!(parse_sleep_count("다른 출력"), None);
}

#[test]
fn 깨진_깨어난_시각은_없는_것으로_읽는다() {
    let test = TestHome::new();
    let file = ResumeFile::new(&test.home);
    fs::write(file.path(), "x; rm -rf ~\n").unwrap();
    assert_eq!(file.read(), None);
    assert!(!fs::read_to_string(file.path()).unwrap().is_empty());
}

// 아래는 원본에 없는 시험이다. 원본은 collect 시험으로만 지나간 분기를 여기서 직접 고정한다.

/// pmset은 macOS에만 있다.
#[cfg(target_os = "macos")]
mod pmset {
    use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
    use kairos_ops::SleepDetector;

    use super::*;

    struct FakePmset(ProcessOutput);

    impl ProcessRunner for FakePmset {
        fn run(&self, request: &ProcessRequest) -> ProcessOutput {
            assert_eq!(request.program, "/usr/bin/pmset");
            assert_eq!(request.args, ["-g", "stats"]);
            self.0.clone()
        }
    }

    #[test]
    fn 잠자기_횟수는_pmset이_성공했을_때만_읽는다() {
        let count = |output| SleepDetector::new(Arc::new(FakePmset(output))).sleep_count();
        assert_eq!(count(ProcessOutput::exited(0, "Sleep Count:7\n")), Some(7));
        assert_eq!(count(ProcessOutput::exited(1, "Sleep Count:7\n")), None);
        assert_eq!(count(ProcessOutput::ended(ProcessEnd::TimedOut)), None);
        assert_eq!(count(ProcessOutput::ended(ProcessEnd::Missing)), None);
    }
}

#[test]
fn linux는_커널이_센_절전_횟수를_읽고_모양이_다르면_none이다() {
    assert_eq!(parse_suspend_success("12\n"), Some(12));
    assert_eq!(parse_suspend_success("0"), Some(0));
    for bad in ["", "\n", "-1\n", "12 3\n", "abc"] {
        assert_eq!(parse_suspend_success(bad), None, "{bad:?}");
    }
}

#[test]
fn pmset의_줄_끝은_js처럼_네_가지를_본다() {
    assert_eq!(parse_sleep_count("x\r\nSleep Count:5\r\ny"), Some(5));
    assert_eq!(parse_sleep_count("Sleep Count:5\u{2028}y"), Some(5));
    assert_eq!(parse_sleep_count("Sleep Count:\n"), None);
    assert_eq!(parse_sleep_count(" Sleep Count:5\n"), None);
    assert_eq!(parse_sleep_count("Sleep Count:5 \n"), None);
}

#[test]
fn 미연결과_잠자기는_고장이_아니다() {
    assert!(!counts_as_failure(Outcome::Ok, None));
    assert!(!counts_as_failure(Outcome::NotConnected, Some("x")));
    assert!(!counts_as_failure(Outcome::Timeout, Some(SLEEP_INTERRUPTED)));
    assert!(counts_as_failure(Outcome::Timeout, Some("source_timeout")));
    assert!(counts_as_failure(Outcome::ParseError, None));
}

#[test]
fn 깨어난_시각은_초_단위_한_줄이고_읽으면_초로_내림한_시각이다() {
    let test = TestHome::new();
    let file = ResumeFile::new(&test.home);
    assert_eq!(file.read(), None);
    file.write(at("2026-03-02T01:00:00.999Z")).unwrap();
    assert_eq!(fs::read_to_string(file.path()).unwrap(), "1772413200\n");
    assert_eq!(file.read().map(Instant::to_iso).as_deref(), Some("2026-03-02T01:00:00.000Z"));
    fs::write(file.path(), "12345678\n").unwrap();
    assert_eq!(file.read(), None, "9자리 미만은 믿지 않는다");
}

fn resumed(repo: TickClockRepo<'_>, when: &str, frozen: bool) -> Option<String> {
    repo.note_tick(at(when), frozen).unwrap().map(Instant::to_iso)
}

#[test]
fn 틱_공백이_6분을_넘어야_복귀다() {
    let test = TestHome::new();
    let db = test.open_db();
    let repo = TickClockRepo::new(db.connection());
    assert_eq!(resumed(repo, T0, false), None, "첫 틱은 복귀가 아니다");
    assert_eq!(resumed(repo, "2026-03-02T01:06:00.000Z", false), None, "6분 공백은 복귀가 아니다");
    let woke = "2026-03-02T01:12:00.001Z";
    assert_eq!(resumed(repo, woke, false).as_deref(), Some(woke));
    assert_eq!(resumed(repo, "2026-03-02T01:15:00.000Z", false).as_deref(), Some(woke), "다음 복귀까지 값을 잇는다");
}

#[test]
fn 밤사이_공백은_복귀가_아니고_그날_시작부터_잰다() {
    let test = TestHome::new();
    let db = test.open_db();
    let repo = TickClockRepo::new(db.connection());
    assert_eq!(resumed(repo, "2026-03-02T10:57:00.000Z", false), None); // 월요일 19:57 KST
    assert_eq!(resumed(repo, "2026-03-03T00:03:00.000Z", false), None, "09:03 첫 틱은 복귀가 아니다");
    let late = "2026-03-03T00:30:00.000Z";
    assert_eq!(resumed(repo, late, false).as_deref(), Some(late));
}

#[test]
fn 얼어붙은_틱은_공백과_무관하게_복귀다() {
    let test = TestHome::new();
    let db = test.open_db();
    let repo = TickClockRepo::new(db.connection());
    assert_eq!(resumed(repo, T0, true).as_deref(), Some(T0));
    let stored: (String, Option<String>) = db
        .connection()
        .query_row("SELECT last_tick_at, resumed_at FROM tick_clock WHERE id = 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(stored, (T0.to_owned(), Some(T0.to_owned())));
}

#[test]
fn 읽지_못하는_마지막_틱은_복귀로_세지_않는다() {
    let test = TestHome::new();
    let db = test.open_db();
    db.connection()
        .execute_batch("INSERT INTO tick_clock (id, last_tick_at, resumed_at) VALUES (1, '깨진 값', NULL)")
        .unwrap();
    assert_eq!(resumed(TickClockRepo::new(db.connection()), T3, false), None);
}
