use std::cmp::Ordering;
use std::fmt;
use std::sync::LazyLock;

use chrono::{DateTime, Datelike, Days, NaiveDate, Offset, TimeDelta, TimeZone, Timelike, Utc};
use regex::{Captures, Regex};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::local_date::LocalDate;

/// 그 순간 프로필 시간대의 UTC 오프셋(밀리초). 서머타임이 있는 지역은 순간마다 다르다.
pub fn local_offset_ms(utc_ms: i64) -> i64 {
    let Some(at) = DateTime::<Utc>::from_timestamp_millis(utc_ms) else {
        return 0;
    };
    i64::from(crate::profile::zone().offset_from_utc_datetime(&at.naive_utc()).fix().local_minus_utc()) * 1000
}

/// 벽시계 시각(UTC 자정 기준 밀리초로 쓴 현지 날짜와 시각)을 순간으로 바꾼다.
/// 서머타임으로 두 번 있는 시각은 앞의 것, 건너뛴 시각은 앞선 오프셋으로 읽는다(V8과 같다).
fn from_wall_ms(wall_ms: i64) -> i64 {
    const HALF_DAY_MS: i64 = 12 * 3_600_000;
    // 전환 앞뒤의 오프셋. 전환은 12시간 안에 두 번 일어나지 않는다.
    let before = wall_ms - local_offset_ms(wall_ms - HALF_DAY_MS);
    let after = wall_ms - local_offset_ms(wall_ms + HALF_DAY_MS);
    let valid = |candidate: i64| candidate + local_offset_ms(candidate) == wall_ms;
    match (valid(before), valid(after)) {
        (true, true) => before.min(after),
        (false, true) => after,
        // 건너뛴 시각이면 앞선 오프셋으로 읽는다.
        _ => before,
    }
}

/// 한 순간. Unix 에포크 이후 밀리초이고 `Date`의 시각 값과 같다.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Instant(i64);

static ISO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^([0-9]{4})(?:-([0-9]{2})(?:-([0-9]{2}))?)?(?:[Tt ]([0-9]{2}):([0-9]{2})(?::([0-9]{2})(?:\.([0-9]+))?)?[ ]*([Zz]|[+-][0-9]{2}:?[0-9]{2})?)?$",
    )
    .expect("상수 정규식")
});
const TIME_AND_ZONE: &str =
    r"(?:[ ]+([0-9]{1,2}):([0-9]{2})(?::([0-9]{2}))?)?[ ]*(GMT|UTC|UT|Z|[+-][0-9]{4})?(?:[ ]*\(.*\))?$";
static DAY_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?:[A-Za-z]{{3,9}},?[ ]+)?([0-9]{{1,2}})[ ]+([A-Za-z]{{3,9}})[ ]+([0-9]{{4}}){TIME_AND_ZONE}"
    ))
    .expect("상수 정규식")
});
static MONTH_FIRST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?:[A-Za-z]{{3,9}},?[ ]+)?([A-Za-z]{{3,9}})[ ]+([0-9]{{1,2}}),?[ ]+([0-9]{{4}}){TIME_AND_ZONE}"
    ))
    .expect("상수 정규식")
});
static SLASHED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^([0-9]{{1,2}})/([0-9]{{1,2}})/([0-9]{{4}}){TIME_AND_ZONE}")).expect("상수 정규식")
});

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// 시각 표기에서 뽑은 숫자들. 범위 판정과 계산을 한곳에 모은다.
struct Fields {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    /// None이면 현지 시각이다.
    offset_ms: Option<i64>,
}

impl Fields {
    fn to_instant(&self) -> Option<Instant> {
        let in_range = (1..=12).contains(&self.month) && (1..=31).contains(&self.day);
        let midnight_end = self.hour == 24 && self.minute == 0 && self.second == 0 && self.millis == 0;
        if !in_range || !(self.hour < 24 || midnight_end) || self.minute > 59 || self.second > 59 {
            return None;
        }
        // V8은 그달에 없는 날(2월 30일)을 다음 달로 넘긴다.
        let first = NaiveDate::from_ymd_opt(self.year, self.month, 1)?;
        let date = first.checked_add_days(Days::new(u64::from(self.day - 1)))?;
        let days = i64::from(date.num_days_from_ce() - EPOCH_DAYS_FROM_CE);
        let clock = i64::from(self.hour) * 3_600_000
            + i64::from(self.minute) * 60_000
            + i64::from(self.second) * 1000
            + i64::from(self.millis);
        let wall = days * 86_400_000 + clock;
        Some(Instant(self.offset_ms.map_or_else(|| from_wall_ms(wall), |offset| wall - offset)))
    }
}

const EPOCH_DAYS_FROM_CE: i32 = 719_163;

fn number(captures: &Captures<'_>, index: usize) -> Option<u32> {
    captures.get(index).map_or(Some(0), |m| m.as_str().parse().ok())
}

fn numeric_offset(text: &str) -> Option<i64> {
    let sign = if text.starts_with('-') { -1 } else { 1 };
    let digits: String = text[1..].chars().filter(char::is_ascii_digit).collect();
    let hours: i64 = digits.get(..2)?.parse().ok()?;
    let minutes: i64 = digits.get(2..4)?.parse().ok()?;
    Some(sign * (hours * 3_600_000 + minutes * 60_000))
}

fn parse_iso(text: &str) -> Option<Instant> {
    let captures = ISO.captures(text)?;
    let has_time = captures.get(4).is_some();
    let millis = captures.get(7).map_or(Some(0), |m| {
        // 넷째 자리부터는 버린다. V8과 같다.
        let digits = format!("{:0<3}", m.as_str());
        digits[..3].parse().ok()
    })?;
    let zone = captures.get(8).map(|m| m.as_str());
    let offset_ms = match zone {
        Some("Z" | "z") => Some(0),
        Some(numeric) => Some(numeric_offset(numeric)?),
        // 날짜만 쓰면 UTC, 시각이 있으면 현지다.
        None if has_time => None,
        None => Some(0),
    };
    Fields {
        year: captures[1].parse().ok()?,
        month: captures.get(2).map_or(Some(1), |m| m.as_str().parse().ok())?,
        day: captures.get(3).map_or(Some(1), |m| m.as_str().parse().ok())?,
        hour: number(&captures, 4)?,
        minute: number(&captures, 5)?,
        second: number(&captures, 6)?,
        millis,
        offset_ms,
    }
    .to_instant()
}

fn month_number(name: &str) -> Option<u32> {
    let prefix = name.get(..3)?.to_ascii_lowercase();
    MONTHS.iter().position(|month| *month == prefix).map(|index| index as u32 + 1)
}

fn legacy_zone(captures: &Captures<'_>) -> Option<Option<i64>> {
    match captures.get(7).map(|m| m.as_str()) {
        None => Some(None),
        Some("GMT" | "UTC" | "UT" | "Z") => Some(Some(0)),
        Some(numeric) => numeric_offset(numeric).map(Some),
    }
}

fn legacy_fields(captures: &Captures<'_>, month: u32, day: u32, year: i32) -> Option<Instant> {
    Fields {
        year,
        month,
        day,
        hour: number(captures, 4)?,
        minute: number(captures, 5)?,
        second: number(captures, 6)?,
        millis: 0,
        offset_ms: legacy_zone(captures)?,
    }
    .to_instant()
}

fn parse_legacy(text: &str) -> Option<Instant> {
    if let Some(captures) = DAY_FIRST.captures(text) {
        return legacy_fields(
            &captures,
            month_number(&captures[2])?,
            captures[1].parse().ok()?,
            captures[3].parse().ok()?,
        );
    }
    if let Some(captures) = MONTH_FIRST.captures(text) {
        return legacy_fields(
            &captures,
            month_number(&captures[1])?,
            captures[2].parse().ok()?,
            captures[3].parse().ok()?,
        );
    }
    let captures = SLASHED.captures(text)?;
    legacy_fields(&captures, captures[1].parse().ok()?, captures[2].parse().ok()?, captures[3].parse().ok()?)
}

impl Instant {
    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    pub const fn millis(self) -> i64 {
        self.0
    }

    /// `Date.parse`. 메일 날짜까지 받는다. 해석하지 못하면 None이다(`NaN`).
    pub fn parse(text: &str) -> Option<Self> {
        let text = crate::js::trim(text);
        parse_iso(text).or_else(|| parse_legacy(text))
    }

    /// `Date.prototype.toISOString`. SQL이 문자열로 비교하므로 형식을 바꾸면 안 된다.
    pub fn to_iso(self) -> String {
        match DateTime::<Utc>::from_timestamp_millis(self.0) {
            Some(at) => format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                at.year(),
                at.month(),
                at.day(),
                at.hour(),
                at.minute(),
                at.second(),
                at.timestamp_subsec_millis()
            ),
            None => String::new(),
        }
    }

    #[must_use]
    pub const fn plus_ms(self, delta: i64) -> Self {
        Self(self.0 + delta)
    }

    #[must_use]
    pub const fn minus_ms(self, delta: i64) -> Self {
        Self(self.0 - delta)
    }

    pub const fn ms_since(self, earlier: Self) -> i64 {
        self.0 - earlier.0
    }

    /// 벽시계 밀리초. 프로필 시간대의 현지 날짜와 시각을 UTC인 척 쓴 값이다.
    fn wall_ms(self) -> i64 {
        self.0 + local_offset_ms(self.0)
    }

    /// 이 순간의 현지 날짜. 시스템 시간대가 아니라 프로필의 시간대다.
    pub fn local_date(self) -> LocalDate {
        let shifted = DateTime::<Utc>::UNIX_EPOCH + TimeDelta::milliseconds(self.wall_ms());
        LocalDate::from_naive(shifted.date_naive())
    }

    /// 현지 자정 이후 분.
    pub fn local_minutes(self) -> u32 {
        (self.wall_ms().rem_euclid(86_400_000) / 60_000) as u32
    }

    /// 현지 분 안의 초.
    pub fn local_seconds(self) -> u32 {
        (self.wall_ms().rem_euclid(60_000) / 1000) as u32
    }

    /// 현지 날짜와 자정 이후 분을 순간으로 바꾼다.
    pub fn at_local(date: LocalDate, minutes: u32) -> Self {
        Self(from_wall_ms(date.utc_midnight_ms() + i64::from(minutes) * 60_000))
    }

    pub fn start_of_local_day(self) -> Self {
        Self::at_local(self.local_date(), 0)
    }

    /// 두 시각 표기를 순간으로 비교한다. 해석할 수 없는 쪽이 있으면 None이다.
    pub fn compare_texts(a: &str, b: &str) -> Option<Ordering> {
        Some(Self::parse(a)?.cmp(&Self::parse(b)?))
    }
}

impl fmt::Display for Instant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_iso())
    }
}

impl Serialize for Instant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_iso())
    }
}

impl<'de> Deserialize<'de> for Instant {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("invalid instant"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기대값은 Node 26의 `Date.parse` 실측이다. 스크립트 출력을 그대로 옮겼다.
    #[test]
    fn date_parse와_같다() {
        let cases = [
            ("2026-11-05", "2026-11-05T00:00:00.000Z"),
            ("2026-11", "2026-11-01T00:00:00.000Z"),
            ("2026", "2026-01-01T00:00:00.000Z"),
            ("2026-11-05T01:02", "2026-11-04T16:02:00.000Z"),
            ("2026-11-05T01:02:03", "2026-11-04T16:02:03.000Z"),
            ("2026-11-05T01:02:03.1", "2026-11-04T16:02:03.100Z"),
            ("2026-11-05T01:02:03.123456Z", "2026-11-05T01:02:03.123Z"),
            ("2026-11-05T01:02:03Z", "2026-11-05T01:02:03.000Z"),
            ("2026-11-05T01:02:03z", "2026-11-05T01:02:03.000Z"),
            ("2026-11-05t01:02:03Z", "2026-11-05T01:02:03.000Z"),
            ("2026-11-05T01:02:03+09:00", "2026-11-04T16:02:03.000Z"),
            ("2026-11-05T01:02:03+0900", "2026-11-04T16:02:03.000Z"),
            ("2026-11-05T01:02:03-08:30", "2026-11-05T09:32:03.000Z"),
            ("2026-11-05 01:02:03", "2026-11-04T16:02:03.000Z"),
            ("2026-11-05 01:02", "2026-11-04T16:02:00.000Z"),
            ("2026-11-05 01:02:03 +0900", "2026-11-04T16:02:03.000Z"),
            ("2026-11-05 01:02:03Z", "2026-11-05T01:02:03.000Z"),
            ("2026-02-30", "2026-03-02T00:00:00.000Z"),
            ("2026-02-30T00:00:00Z", "2026-03-02T00:00:00.000Z"),
            ("2026-11-05T24:00:00Z", "2026-11-06T00:00:00.000Z"),
            ("Wed, 05 Nov 2026 10:00:00 +0900", "2026-11-05T01:00:00.000Z"),
            ("Wed, 5 Nov 2026 10:00:00 GMT", "2026-11-05T10:00:00.000Z"),
            ("5 Nov 2026 10:00:00 +0900", "2026-11-05T01:00:00.000Z"),
            ("Thu, 05 Nov 2026 10:00:00 +0900 (KST)", "2026-11-05T01:00:00.000Z"),
            ("Nov 5, 2026", "2026-11-04T15:00:00.000Z"),
            ("Nov 5, 2026 10:00:00", "2026-11-05T01:00:00.000Z"),
            ("11/05/2026", "2026-11-04T15:00:00.000Z"),
        ];
        for (input, expected) in cases {
            assert_eq!(Instant::parse(input).map(Instant::to_iso).as_deref(), Some(expected), "{input}");
        }
    }

    #[test]
    fn 해석할_수_없으면_none이다() {
        for input in ["2026-13-01", "2026-11-05T24:00:01Z", "2026-11-05T25:00:00Z", "", "abc", "1730000000"] {
            assert_eq!(Instant::parse(input), None, "{input}");
        }
    }

    #[test]
    fn 현지_날짜와_분() {
        let at = Instant::parse("2026-11-05T15:30:00Z").unwrap();
        assert_eq!(at.local_date().to_string(), "2026-11-06");
        assert_eq!(at.local_minutes(), 30);
        assert_eq!(Instant::at_local(at.local_date(), 30), at);
        assert_eq!(at.start_of_local_day().to_iso(), "2026-11-05T15:00:00.000Z");
    }
}
