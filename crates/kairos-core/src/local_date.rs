use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use chrono::{Datelike, Days, NaiveDate, Weekday};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// 일정 하나가 쉬는 날로 인정받는 최대 일수.
pub const MAX_OFF_SPAN_DAYS: i64 = 31;

/// 현지 달력의 날짜. 시각이 없다.
///
/// 날짜는 두 종류다. 섞으면 하루가 어긋난다.
/// - `LocalDate`(YYYY-MM-DD): 종일 일정과 근무일.
/// - `Instant`: 시간 일정과 이벤트 시각.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct LocalDate(NaiveDate);

impl LocalDate {
    pub(crate) const fn from_naive(date: NaiveDate) -> Self {
        Self(date)
    }

    /// `YYYY-MM-DD` 모양이고 달력에 있는 날이면 값을 준다.
    pub fn parse(text: &str) -> Option<Self> {
        if !is_date_only(text) {
            return None;
        }
        NaiveDate::parse_from_str(text, "%Y-%m-%d").ok().map(Self)
    }

    /// 종일 일정의 date 값에서 앞 10자만 현지 날짜로 취한다. 순간으로 바꾸지 않는다.
    /// `2026-11-05T00:00:00Z`를 순간으로 바꿔 KST로 읽으면 11/5 09:00 시작이 된다.
    pub fn from_all_day(value: &str) -> Option<Self> {
        Self::parse(value.get(..10)?)
    }

    #[must_use]
    pub fn add_days(self, days: i64) -> Self {
        let moved = if days >= 0 {
            self.0.checked_add_days(Days::new(days.unsigned_abs()))
        } else {
            self.0.checked_sub_days(Days::new(days.unsigned_abs()))
        };
        Self(moved.unwrap_or(self.0))
    }

    pub fn weekday(self) -> Weekday {
        self.0.weekday()
    }

    pub fn is_weekend(self) -> bool {
        matches!(self.0.weekday(), Weekday::Sat | Weekday::Sun)
    }

    /// 이 날짜의 UTC 자정. `Date.UTC`와 같다.
    pub(crate) fn utc_midnight_ms(self) -> i64 {
        i64::from(self.0.num_days_from_ce() - 719_163) * 86_400_000
    }

    pub fn days_until(self, later: Self) -> i64 {
        (later.0 - self.0).num_days()
    }
}

/// `/^\d{4}-\d{2}-\d{2}$/`.
pub fn is_date_only(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

impl fmt::Display for LocalDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.0.year(), self.0.month(), self.0.day())
    }
}

impl FromStr for LocalDate {
    type Err = crate::KairosError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text).ok_or_else(|| crate::KairosError::new("invalid_date"))
    }
}

impl Serialize for LocalDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for LocalDate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("invalid date"))
    }
}

/// 쉬는 날 집합. 근무일 판정은 이 객체가 맡는다.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OffDates(BTreeSet<LocalDate>);

impl OffDates {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, date: LocalDate) {
        self.0.insert(date);
    }

    pub fn contains(&self, date: LocalDate) -> bool {
        self.0.contains(&date)
    }

    pub fn iter(&self) -> impl Iterator<Item = LocalDate> + '_ {
        self.0.iter().copied()
    }

    /// 프로필의 근무 요일이고 쉬는 날 목록에 없으면 근무일이다. 기본 근무 요일은 월~금이다.
    pub fn is_workday(&self, date: LocalDate) -> bool {
        crate::profile::current().schedule.days.contains(date.weekday()) && !self.contains(date)
    }

    /// date 직전의 근무일.
    pub fn previous_workday(&self, date: LocalDate) -> LocalDate {
        let mut cursor = date.add_days(-1);
        while !self.is_workday(cursor) {
            cursor = cursor.add_days(-1);
        }
        cursor
    }

    /// date 직후의 근무일.
    pub fn next_workday(&self, date: LocalDate) -> LocalDate {
        let mut cursor = date.add_days(1);
        while !self.is_workday(cursor) {
            cursor = cursor.add_days(1);
        }
        cursor
    }
}

impl FromIterator<LocalDate> for OffDates {
    fn from_iter<T: IntoIterator<Item = LocalDate>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Extend<LocalDate> for OffDates {
    fn extend<T: IntoIterator<Item = LocalDate>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> LocalDate {
        LocalDate::parse(text).unwrap()
    }

    #[test]
    fn 종일_값은_앞_10자만_본다() {
        assert_eq!(LocalDate::from_all_day("2026-11-05T00:00:00Z"), Some(date("2026-11-05")));
        assert_eq!(LocalDate::from_all_day("2026"), None);
    }

    #[test]
    fn 근무일_판정() {
        let off: OffDates = [date("2026-11-06")].into_iter().collect();
        assert!(off.is_workday(date("2026-11-05")));
        assert!(!off.is_workday(date("2026-11-06")));
        assert!(!off.is_workday(date("2026-11-07")));
        assert_eq!(off.next_workday(date("2026-11-05")), date("2026-11-09"));
        assert_eq!(off.previous_workday(date("2026-11-09")), date("2026-11-05"));
    }

    #[test]
    fn 달력_덧셈() {
        assert_eq!(date("2026-12-31").add_days(1).to_string(), "2027-01-01");
        assert_eq!(date("2026-03-01").add_days(-1).to_string(), "2026-02-28");
        assert_eq!(date("2026-11-05").days_until(date("2026-11-07")), 2);
    }
}
