//! 감시자. Linux와 Windows의 스케줄러가 5분마다 `kairos watchdog`으로 부른다. macOS는 같은 판정을 하는 zsh 스크립트
//! (`ops/kairos-watchdog.zsh`)를 쓴다. 두 구현은 `tests/watchdog.rs`의 같은 장면으로 시험한다.
//!
//! 수집 코드에 기대지 않는다. 데이터 폴더의 파일과 DB만 읽기 전용으로 본다. 판정 순서는 zsh와 같다.
//! 1. 수집 시간(`watch-window.tsv`, 없으면 월~금 09:00~20:00) 밖이면 아무것도 하지 않는다.
//! 2. 자기 실행 간격이 7분 30초를 넘으면 컴퓨터가 쉰 것으로 보고 `resumed.at`에 깨어난 시각을 쓴다.
//! 3. 마지막 수집이 10분을 넘었거나 소스가 허용 지연을 넘었으면 알린다. 같은 경고(숫자만 다른 것)는 다시 알리지 않는다.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use chrono::{DateTime, Datelike, Timelike};
use chrono_tz::Tz;
use kairos_core::platform::CreateMode;
use kairos_core::{KairosHome, KairosResult};
use regex::Regex;
use rusqlite::{Connection, OpenFlags, OptionalExtension};

const DEFAULT_WINDOW: (i64, i64, &str) = (540, 1200, "12345");
/// 감시자 주기 5분의 1.5배. 넘으면 잠자기다.
const SLEEP_GAP_SECONDS: i64 = 450;
const STALL_SECONDS: i64 = 600;
const SQLITE_TIMEOUT: Duration = Duration::from_millis(3000);
const FILE_MODE: u32 = 0o600;

static DAYS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[1-7]{1,7}$").expect("상수 정규식"));
static NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9_-]+$").expect("상수 정규식"));
static ALERT_NAME_DROP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^A-Za-z0-9_.: -]").expect("상수 정규식"));
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[0-9]").expect("상수 정규식"));

/// 알리는 방법. 성공이면 0, 아니면 헬퍼의 종료 코드다(헬퍼가 없으면 127).
pub trait WatchdogNotify {
    fn notify(&self, message: &str) -> i32;
}

impl<F: Fn(&str) -> i32> WatchdogNotify for F {
    fn notify(&self, message: &str) -> i32 {
        self(message)
    }
}

/// 감시자 한 번. 표준 출력에 찍을 줄을 돌려준다(zsh 판의 stdout과 같다).
pub struct Watchdog<'a> {
    pub home: &'a KairosHome,
    /// 지금 시각, 초.
    pub now: i64,
    pub zone: Tz,
    /// 참이면 알리지 않고 `알림: <문구>`를 찍는다. `KAIROS_WATCHDOG_DRY`다.
    pub dry: bool,
}

/// 첫 줄의 숫자. 없거나 숫자가 아니면 None이다.
fn read_number(path: &Path) -> Option<i64> {
    let text = fs::read_to_string(path).ok()?;
    let first = text.lines().next()?;
    (!first.is_empty() && first.bytes().all(|b| b.is_ascii_digit())).then(|| first.parse().ok()).flatten()
}

fn non_empty(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

/// 0600으로 한 줄을 쓴다. 파일이 있으면 내용만 바꾼다.
fn write_line(path: &Path, line: &str) -> KairosResult<()> {
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).create_mode(FILE_MODE).open(path)?;
    file.write_all(format!("{line}\n").as_bytes())?;
    Ok(())
}

/// `시작분\t끝분\t요일`. 모양이 틀리면 기본값이다.
fn window(home: &KairosHome) -> (i64, i64, String) {
    let fallback = || (DEFAULT_WINDOW.0, DEFAULT_WINDOW.1, DEFAULT_WINDOW.2.to_owned());
    let Ok(text) = fs::read_to_string(home.file("watch-window.tsv")) else {
        return fallback();
    };
    let mut fields = text.lines().next().unwrap_or("").split('\t');
    let (Some(a), Some(b), Some(c)) = (fields.next(), fields.next(), fields.next()) else {
        return fallback();
    };
    let number = |text: &str| {
        (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.parse::<i64>().ok()).flatten()
    };
    match (number(a), number(b)) {
        (Some(start), Some(end)) if DAYS.is_match(c) && start < end && end <= 1440 => (start, end, c.to_owned()),
        _ => fallback(),
    }
}

fn open_read_only(path: &Path) -> Option<Connection> {
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    connection.busy_timeout(SQLITE_TIMEOUT).ok()?;
    connection.execute_batch("PRAGMA query_only = 1").ok()?;
    Some(connection)
}

/// `coalesce(strftime('%s', max(finished_at)), '')`. 빈 값이면 Some(None)이다. 읽지 못하면 None이다.
fn last_finished(connection: &Connection, filter: Option<(&str, &str)>) -> Option<Option<i64>> {
    let text: Option<String> = match filter {
        None => connection
            .query_row("SELECT coalesce(strftime('%s', max(finished_at)), '') FROM source_run", [], |row| row.get(0))
            .optional()
            .ok()?,
        Some((source, query)) => connection
            .query_row(
                "SELECT coalesce(strftime('%s', max(finished_at)), '') FROM source_run WHERE source = ?1 AND query = ?2 AND outcome = 'ok'",
                [source, query],
                |row| row.get(0),
            )
            .optional()
            .ok()?,
    };
    Some(text.filter(|value| !value.is_empty()).and_then(|value| value.parse().ok()))
}

impl Watchdog<'_> {
    pub fn run(&self, notify: &dyn WatchdogNotify) -> KairosResult<Vec<String>> {
        let home = self.home;
        let Some(local) = DateTime::from_timestamp(self.now, 0).map(|utc| utc.with_timezone(&self.zone)) else {
            return Ok(Vec::new());
        };
        let (start, end, days) = window(home);
        let weekday = local.weekday().number_from_monday().to_string();
        let minutes_now = i64::from(local.hour() * 60 + local.minute());
        if !days.contains(&weekday) || minutes_now < start || minutes_now >= end {
            return Ok(Vec::new());
        }
        let day_start = self.now - ((minutes_now - start) * 60 + i64::from(local.second()));
        let today = local.date_naive();

        let (beat_file, resume_file) = (home.file("watchdog.beat"), home.file("resumed.at"));
        if let Some(last_beat) = read_number(&beat_file).filter(|_| non_empty(&beat_file))
            && self.now - last_beat > SLEEP_GAP_SECONDS
        {
            write_line(&resume_file, &self.now.to_string())?;
        }
        write_line(&beat_file, &self.now.to_string())?;
        let resumed = read_number(&resume_file).filter(|at| *at <= self.now).unwrap_or(0);

        let problems = self.problems(day_start, resumed, today);
        let last_file = home.file("watchdog.last");
        if problems.is_empty() {
            if non_empty(&last_file) {
                fs::write(&last_file, "")?;
            }
            return Ok(Vec::new());
        }
        let mut unique: Vec<&str> = Vec::new();
        for problem in &problems {
            if !unique.contains(&problem.as_str()) {
                unique.push(problem);
            }
        }
        let message = unique.join(", ");
        let key = DIGITS.replace_all(&message, "").into_owned();
        let previous = fs::read_to_string(&last_file).unwrap_or_default();
        if !previous.is_empty() && previous.trim_end_matches('\n') == key {
            return Ok(Vec::new());
        }
        if self.dry {
            write_line(&last_file, &key)?;
            return Ok(vec![format!("알림: {message}")]);
        }
        // 알림이 나간 뒤에만 기록한다. 먼저 쓰면 실패한 알림이 재시도 없이 사라진다.
        match notify.notify(&message) {
            0 => {
                write_line(&last_file, &key)?;
                Ok(Vec::new())
            }
            code => Ok(vec![format!("notify_failed {code}")]),
        }
    }

    fn problems(&self, day_start: i64, resumed: i64, today: chrono::NaiveDate) -> Vec<String> {
        let home = self.home;
        let db = home.db_path();
        if !db.is_file() {
            return vec!["kairos 데이터베이스 없음".to_owned()];
        }
        let watch = home.file("watch.tsv");
        let watch_text = fs::read_to_string(&watch).ok();
        // 수집기가 쉬는 날(kairos off)이면 오늘은 판정하지 않는다.
        if let Some(text) = &watch_text
            && text.lines().next() == Some("active\t0")
        {
            let modified = fs::metadata(&watch).and_then(|metadata| metadata.modified()).ok();
            let modified_day = modified
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|since| DateTime::from_timestamp(i64::try_from(since.as_secs()).ok()?, 0))
                .map(|utc| utc.with_timezone(&self.zone).date_naive());
            if modified_day == Some(today) {
                return Vec::new();
            }
        }
        let Some(connection) = open_read_only(&db) else {
            return vec!["kairos 데이터베이스를 읽지 못함".to_owned()];
        };
        let last_run = match last_finished(&connection, None) {
            None => return vec!["kairos 데이터베이스를 읽지 못함".to_owned()],
            Some(None) => return vec!["kairos 수집 기록 없음".to_owned()],
            Some(Some(last_run)) => last_run,
        };
        let mut problems = Vec::new();
        let age = self.now - last_run.max(day_start).max(resumed);
        let stalled = age > STALL_SECONDS;
        if stalled {
            problems.push(format!("kairos 수집기 {}분째 멈춤", age / 60));
        }
        for line in watch_text.as_deref().unwrap_or("").lines() {
            let mut fields = line.split('\t');
            let kind = fields.next().unwrap_or("");
            let name = fields.next().unwrap_or("");
            if kind == "alert" {
                problems.push(format!("kairos 점검 필요: {}", ALERT_NAME_DROP.replace_all(name, "")));
                continue;
            }
            // 수집기 자체가 멈췄으면 소스별 지연은 같은 원인이다. 따로 적지 않는다.
            if kind != "source" || stalled {
                continue;
            }
            let (query, threshold) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
            let threshold_ok = !threshold.is_empty() && threshold.bytes().all(|b| b.is_ascii_digit());
            if !NAME.is_match(name) || !NAME.is_match(query) || !threshold_ok {
                continue;
            }
            let Ok(threshold) = threshold.parse::<i64>() else {
                continue;
            };
            let ok_at = last_finished(&connection, Some((name, query))).flatten().unwrap_or(0);
            let ok_age = self.now - ok_at.max(day_start).max(resumed);
            if ok_age > threshold {
                problems.push(format!("{name} {}분 지연", ok_age / 60));
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 알림이_실패하면_기록하지_않고_코드만_찍는다() {
        let dir = tempfile::tempdir().unwrap();
        let home = KairosHome::at(dir.path());
        // 월요일 10:00 KST, DB 없음.
        let watchdog = Watchdog { home: &home, now: 1_772_413_200, zone: chrono_tz::Asia::Seoul, dry: false };
        assert_eq!(watchdog.run(&|_: &str| 7).unwrap(), ["notify_failed 7"]);
        assert!(!home.file("watchdog.last").exists());
        let seen = std::sync::Mutex::new(Vec::new());
        let record = |message: &str| {
            seen.lock().unwrap().push(message.to_owned());
            0
        };
        assert!(watchdog.run(&record).unwrap().is_empty());
        assert_eq!(*seen.lock().unwrap(), ["kairos 데이터베이스 없음"]);
        assert_eq!(fs::read_to_string(home.file("watchdog.last")).unwrap(), "kairos 데이터베이스 없음\n");
        assert!(watchdog.run(&|_: &str| panic!("같은 경고는 다시 알리지 않는다")).unwrap().is_empty());
    }
}
