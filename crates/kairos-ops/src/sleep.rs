//! 잠자기와 고장을 나눈다(설계 9.6, 2026-11-05 실측). 수집 틱과 cursor는 바꾸지 않고 기록의 코드와 건강 판정만 바꾼다.
//!
//! 틱이 잠자기에 끊겼다는 판정은 측정 값 둘 중 하나다.
//!   심장박동 공백  프로세스가 1초 박동을 `SLEEP_GAP_MS` 넘게 못 돌렸다. 깨어 있는 동안 자식이 멈추면 박동은 계속 돈다.
//!   잠자기 횟수    pmset -g stats의 Sleep Count가 틱 사이에 늘었다. 짧은 잠자기도 잡는다.
//! 그 틱의 timeout만 `SLEEP_INTERRUPTED`로 바꾼다. 다른 실패는 건드리지 않아 진짜 고장을 숨기지 않는다.
//!
//! 깨어난 시각은 건강 판정과 감시자의 하한이다.
//! 그 뒤 허용 지연 안에서만 지연으로 세지 않으므로 깨어난 뒤에도 수집이 없으면 그대로 경고가 된다.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use kairos_core::home::write_file_atomic;
use kairos_core::model::Outcome;
use kairos_core::ports::Clock;
use kairos_core::process::{ProcessRequest, ProcessRunner};
use kairos_core::{Instant, KairosHome, KairosResult, js};
use regex::Regex;

pub const SLEEP_INTERRUPTED: &str = "sleep_interrupted";
/// 동기 DB 대기는 busy_timeout 5초가 상한이다. 측정한 얼어붙음은 115초 이상이다.
pub const SLEEP_GAP_MS: i64 = 30_000;
pub const BEAT_MS: u64 = 1_000;
/// 틱 하나를 건너뛴 공백은 복귀가 아니다. launchd 주기 3분의 두 배다.
pub const TICK_GAP_MS: i64 = 6 * 60_000;
const PMSET_TIMEOUT: Duration = Duration::from_millis(2_000);
/// execFile의 기본 maxBuffer. 넘으면 실패로 읽는다.
const PMSET_MAX_OUTPUT: usize = 1024 * 1024;
static EPOCH_SECONDS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{9,11}$").expect("상수 정규식"));

/// 심장박동의 가장 긴 공백을 잰다. 박동 스레드와 판정 쪽이 함께 쥔다.
/// 벽시계로 잰다. macOS의 monotonic 시계는 잠자기 동안 멈춰 공백이 보이지 않는다.
pub struct StallWatch {
    clock: Arc<dyn Clock>,
    last: AtomicI64,
    longest: AtomicI64,
}

impl StallWatch {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let last = AtomicI64::new(clock.now().millis());
        Self { clock, last, longest: AtomicI64::new(0) }
    }

    pub fn beat(&self) {
        let now = self.clock.now().millis();
        let previous = self.last.swap(now, Ordering::SeqCst);
        self.longest.fetch_max(now - previous, Ordering::SeqCst);
    }

    /// 지금까지의 가장 긴 공백. 마지막 박동 뒤의 공백도 센다.
    pub fn max_gap_ms(&self) -> i64 {
        let since_last = self.clock.now().millis() - self.last.load(Ordering::SeqCst);
        self.longest.load(Ordering::SeqCst).max(since_last)
    }
}

/// `/^Sleep Count:(\d+)$/m`. JS의 `^ $`는 네 가지 줄 끝을 본다. Rust의 `(?m)`은 `\n`만 본다.
pub fn parse_sleep_count(text: &str) -> Option<i64> {
    text.split(['\n', '\r', '\u{2028}', '\u{2029}']).find_map(|line| {
        let digits = line.strip_prefix("Sleep Count:")?;
        (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
    })
}

/// OS의 잠자기 횟수를 읽는다.
#[derive(Clone)]
pub struct SleepDetector {
    runner: Arc<dyn ProcessRunner>,
}

impl SleepDetector {
    pub fn new(runner: Arc<dyn ProcessRunner>) -> Self {
        Self { runner }
    }

    /// 읽지 못하면 None이고 심장박동만으로 판정한다.
    pub fn sleep_count(&self) -> Option<i64> {
        let request =
            ProcessRequest::new("/usr/bin/pmset", ["-g", "stats"], PMSET_TIMEOUT).max_output(PMSET_MAX_OUTPUT);
        let output = self.runner.run(&request);
        if output.success() { parse_sleep_count(&output.stdout) } else { None }
    }
}

pub fn slept_during(gap_ms: i64, before: Option<i64>, after: Option<i64>) -> bool {
    gap_ms >= SLEEP_GAP_MS || before.zip(after).is_some_and(|(before, after)| after > before)
}

/// 수집 요약의 실패 건수. 미연결과 잠자기는 고장이 아니다.
pub fn counts_as_failure(outcome: Outcome, error_code: Option<&str>) -> bool {
    outcome != Outcome::Ok && outcome != Outcome::NotConnected && error_code != Some(SLEEP_INTERRUPTED)
}

/// resumed.at. 감시자(zsh)도 쓰고 읽으므로 초 단위 숫자 한 줄이다.
#[derive(Clone, Debug)]
pub struct ResumeFile {
    path: PathBuf,
}

impl ResumeFile {
    pub fn new(home: &KairosHome) -> Self {
        Self { path: home.file("resumed.at") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, at: Instant) -> KairosResult<()> {
        write_file_atomic(&self.path, &format!("{}\n", at.millis().div_euclid(1000)))
    }

    /// 없거나 깨졌으면 None이다.
    pub fn read(&self) -> Option<Instant> {
        let content = fs::read_to_string(&self.path).ok()?;
        let text = js::trim(&content);
        if !EPOCH_SECONDS.is_match(text) {
            return None;
        }
        text.parse::<i64>().ok().map(|seconds| Instant::from_millis(seconds * 1000))
    }
}
