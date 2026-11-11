//! 값 검증. 설정 값은 검색어, argv, 세션 프롬프트, 정규식(이스케이프 뒤)으로 흘러가므로 모양을 좁게 받는다.
//! 받은 값은 정규화(소문자 등)해서 돌려준다.

use std::sync::LazyLock;

use chrono_tz::Tz;
use regex::Regex;

use crate::ConfigError;

fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("상수 정규식")
}

static GITHUB_NAME: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$"));
static REPO_NAME: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Za-z0-9_.-]{1,100}$"));
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^[A-Za-z0-9._%+-]{1,64}@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}$"));
static DOMAIN: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[a-z0-9-]+(?:\.[a-z0-9-]+)*\.[a-z]{2,}$"));
static CALENDAR_ID: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^[A-Za-z0-9._%+#-]{1,128}@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*$"));
static SLACK_USER: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[UW][A-Z0-9]{2,20}$"));
static BRANCH: LazyLock<Regex> = LazyLock::new(|| pattern(r"^origin/[A-Za-z0-9._-]+(?:/[A-Za-z0-9._-]+)*$"));
static UUID: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"));
static TOOL_NAME: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Za-z0-9_-]{1,64}$"));
static CLOCK: LazyLock<Regex> = LazyLock::new(|| pattern(r"^([01][0-9]|2[0-3]):([0-5][0-9])$|^24:00$"));
/// 비밀이 설정 파일로 새지 않게 한다. 토큰 모양의 값은 어느 칸이든 거부한다.
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r"(?:github_pat_|ghp_|gho_|ghu_|ghs_|ghr_|xox[abprs]-|sk-ant-|sk-[A-Za-z0-9]{20}|AKIA[0-9A-Z]{16}|-----BEGIN)",
    )
});

/// 검증 맥락. 오류에 파일과 키를 남기고 값은 넣지 않는다.
#[derive(Clone, Copy)]
pub(crate) struct At {
    pub file: &'static str,
    pub key: &'static str,
}

impl At {
    pub const fn new(file: &'static str, key: &'static str) -> Self {
        Self { file, key }
    }

    fn fail(self, code: &'static str) -> ConfigError {
        ConfigError::key(self.file, self.key, code)
    }

    fn check(self, value: &str, valid: bool) -> Result<(), ConfigError> {
        if SECRET.is_match(value) {
            return Err(self.fail("config_secret_value"));
        }
        if valid { Ok(()) } else { Err(self.fail("config_invalid_value")) }
    }
}

/// 사람이 읽는 짧은 글. 제어 문자와 정규식, 셸이 다르게 읽을 글자가 없어야 한다.
fn plain(value: &str, max: usize) -> bool {
    let count = value.chars().count();
    (1..=max).contains(&count)
        && value.trim() == value
        && !value.chars().any(|c| c.is_control() || matches!(c, '`' | '$' | '\\' | '"' | '\'' | '<' | '>'))
}

pub(crate) fn github_name(at: At, value: &str) -> Result<String, ConfigError> {
    at.check(value, GITHUB_NAME.is_match(value) && !value.ends_with('-'))?;
    Ok(value.to_ascii_lowercase())
}

pub(crate) fn repo_name(at: At, value: &str) -> Result<String, ConfigError> {
    at.check(value, REPO_NAME.is_match(value) && value != "." && value != "..")?;
    Ok(value.to_owned())
}

/// `owner/name`.
pub(crate) fn full_repo(at: At, value: &str) -> Result<String, ConfigError> {
    let Some((owner, name)) = value.split_once('/') else {
        return Err(at.fail("config_invalid_value"));
    };
    Ok(format!("{}/{}", github_name(at, owner)?, repo_name(at, name)?))
}

pub(crate) fn email(at: At, value: &str) -> Result<String, ConfigError> {
    at.check(value, EMAIL.is_match(value))?;
    Ok(value.to_ascii_lowercase())
}

pub(crate) fn domain(at: At, value: &str) -> Result<String, ConfigError> {
    let lower = value.to_ascii_lowercase();
    at.check(value, DOMAIN.is_match(&lower) && lower.len() <= 253)?;
    Ok(lower)
}

pub(crate) fn calendar_id(at: At, value: &str) -> Result<String, ConfigError> {
    at.check(value, CALENDAR_ID.is_match(value))?;
    Ok(value.to_owned())
}

pub(crate) fn slack_user(at: At, value: &str) -> Result<String, ConfigError> {
    at.check(value, SLACK_USER.is_match(value))?;
    Ok(value.to_owned())
}

/// 본인 이름. 정규식에 넣을 때는 이스케이프하지만, 일치 판정이 넓어지지 않게 글자를 좁힌다.
pub(crate) fn person_name(at: At, value: &str) -> Result<String, ConfigError> {
    let letters = value.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '.');
    at.check(value, plain(value, 40) && letters && value.chars().filter(|c| c.is_alphabetic()).count() >= 2)?;
    Ok(value.to_owned())
}

pub(crate) fn label(at: At, value: &str, max: usize) -> Result<String, ConfigError> {
    at.check(value, plain(value, max))?;
    Ok(value.to_owned())
}

/// `origin/…`. `..`, 빈 조각, `.lock` 끝, `-`로 시작하는 조각은 받지 않는다(git이 인자로 읽는다).
pub(crate) fn base_branch(at: At, value: &str) -> Result<String, ConfigError> {
    let parts_ok = value.split('/').all(|part| !part.starts_with('-') && !part.starts_with('.'));
    let valid =
        BRANCH.is_match(value) && !value.contains("..") && !value.ends_with(".lock") && parts_ok && value.len() <= 200;
    at.check(value, valid)?;
    Ok(value.to_owned())
}

/// MCP 도구 이름. claude의 권한 이름(`mcp__서버__도구`)에 그대로 들어간다.
pub(crate) fn tool_name(at: At, value: &str) -> Result<String, ConfigError> {
    at.check(value, TOOL_NAME.is_match(value))?;
    Ok(value.to_owned())
}

/// 절대 경로. `..`, 제어 문자, 셸이 다르게 읽을 글자는 받지 않는다(worktree 경로가 터미널 명령에 들어간다).
pub(crate) fn absolute_path(at: At, value: &str) -> Result<std::path::PathBuf, ConfigError> {
    let valid = value.starts_with('/')
        && value.len() <= 1024
        && !value.split('/').any(|part| part == "..")
        && !value.chars().any(|c| {
            c.is_control()
                || matches!(c, '`' | '$' | '\\' | '"' | '\'' | ';' | '&' | '|' | '<' | '>' | '*' | '?' | '\n')
        });
    at.check(value, valid)?;
    Ok(std::path::PathBuf::from(value.trim_end_matches('/')))
}

pub(crate) fn orca_repo(at: At, value: &str) -> Result<String, ConfigError> {
    let lower = value.to_ascii_lowercase();
    at.check(value, UUID.is_match(&lower))?;
    Ok(lower)
}

pub(crate) fn zone(at: At, value: &str) -> Result<Tz, ConfigError> {
    at.check(value, true)?;
    value.parse::<Tz>().map_err(|_| at.fail("config_unknown_timezone"))
}

/// `HH:MM` → 자정 이후 분. `24:00`은 하루 끝이다.
pub(crate) fn clock(at: At, value: &str) -> Result<u32, ConfigError> {
    let captures = CLOCK.captures(value).ok_or_else(|| at.fail("config_invalid_time"))?;
    let (Some(hours), Some(minutes)) = (captures.get(1), captures.get(2)) else {
        return Ok(24 * 60);
    };
    let parse = |text: &str| text.parse::<u32>().map_err(|_| at.fail("config_invalid_time"));
    Ok(parse(hours.as_str())? * 60 + parse(minutes.as_str())?)
}

pub(crate) fn range(at: At, value: &[String; 2]) -> Result<kairos_core::profile::MinuteRange, ConfigError> {
    let (start, end) = (clock(at, &value[0])?, clock(at, &value[1])?);
    if start >= end {
        return Err(at.fail("config_empty_range"));
    }
    Ok(kairos_core::profile::MinuteRange::new(start, end))
}

/// 근무 요일. `mon`~`sun`만 받고 비거나 겹치면 거부한다.
pub(crate) fn days(at: At, values: &[String]) -> Result<kairos_core::profile::WorkDays, ConfigError> {
    unique(at, values)?;
    let days = kairos_core::profile::WorkDays::from_names(values.iter().map(String::as_str))
        .ok_or_else(|| at.fail("config_invalid_day"))?;
    if days.is_empty() {
        return Err(at.fail("config_no_days"));
    }
    Ok(days)
}

/// 점심 구간. 빈 목록은 끈 것이다(`start == end`).
pub(crate) fn optional_range(at: At, value: &[String]) -> Result<kairos_core::profile::MinuteRange, ConfigError> {
    match value {
        [] => Ok(kairos_core::profile::MinuteRange::new(0, 0)),
        [start, end] => range(at, &[start.clone(), end.clone()]),
        _ => Err(at.fail("config_invalid_range")),
    }
}

/// 값 목록의 중복을 거부한다. 같은 값을 두 번 쓰면 둘 중 하나는 오타일 가능성이 크다.
pub(crate) fn unique(at: At, values: &[String]) -> Result<(), ConfigError> {
    let mut seen: Vec<&String> = values.iter().collect();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    if seen.len() == before { Ok(()) } else { Err(at.fail("config_duplicate_value")) }
}

/// 비밀 검사만 한다. 모양은 쓰는 쪽이 따로 본다.
pub(crate) fn no_secret(at: At, value: &str) -> Result<(), ConfigError> {
    at.check(value, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: At = At::new("test", "key");

    #[test]
    fn 기준_브랜치는_안전한_모양만_받는다() {
        assert!(base_branch(AT, "origin/fe-stage").is_ok());
        assert!(base_branch(AT, "origin/release/2026.11").is_ok());
        for bad in ["fe-stage", "origin/../x", "origin/-x", "origin/x.lock", "origin/a b", "origin/$(x)", "origin//x"] {
            assert!(base_branch(AT, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn 비밀_모양은_어디서든_거부한다() {
        assert_eq!(label(AT, "ghp_abcdef", 40).unwrap_err().code, "config_secret_value");
        assert_eq!(email(AT, "github_pat_x@a.example").unwrap_err().code, "config_secret_value");
    }

    #[test]
    fn 시각은_hh_mm만_받는다() {
        assert_eq!(clock(AT, "13:30").unwrap(), 810);
        assert_eq!(clock(AT, "24:00").unwrap(), 1440);
        assert!(clock(AT, "9:00").is_err());
        assert!(clock(AT, "24:01").is_err());
    }

    #[test]
    fn 이름은_글자만_받는다() {
        assert!(person_name(AT, "김준서").is_ok());
        assert!(person_name(AT, "Jane Doe").is_ok());
        assert!(person_name(AT, "a.*").is_err());
        assert!(person_name(AT, "x").is_err());
    }

    #[test]
    fn 식별자는_정규화한다() {
        assert_eq!(github_name(AT, "Kairos-Dev").unwrap(), "kairos-dev");
        assert_eq!(email(AT, "A@Example.COM").unwrap(), "a@example.com");
        assert!(slack_user(AT, "u123").is_err());
        assert!(calendar_id(AT, "ko.south_korea#holiday@group.v.calendar.google.com").is_ok());
    }
}
