//! 종일 일정의 날짜. 원문 모양만 믿고 달력에 있는지는 보지 않는다. 그래야 `2026-02-30`이 TS 때처럼 저장된다.

use kairos_core::LocalDate;
use kairos_core::js;
use kairos_core::local_date::is_date_only;

use crate::shared::ParseFailure;

/// 종일 일정의 date 값에서 앞 10자만 현지 날짜로 취한다. 순간으로 바꾸지 않는다.
/// `2026-11-05T00:00:00Z`를 순간으로 바꿔 KST로 읽으면 11/5 09:00 시작이 된다.
pub(super) fn all_day_date(value: &str) -> Result<String, ParseFailure> {
    let date = js::slice_utf16(value, 10);
    if is_date_only(&date) { Ok(date) } else { Err(ParseFailure::new("calendar_date")) }
}

fn number(text: &str) -> i64 {
    text.parse().unwrap_or(0)
}

/// `new Date(Date.UTC(y, m - 1, d + days)).toISOString().slice(0, 10)`.
/// 달과 날이 넘치면 다음으로 넘기고, 0~99년은 19xx년으로 읽는다. date는 `all_day_date`를 거친 값이다.
pub(super) fn add_days(date: &str, days: i64) -> Option<String> {
    let (year, month, day) = (number(date.get(0..4)?), number(date.get(5..7)?), number(date.get(8..10)?));
    let full_year = if (0..=99).contains(&year) { 1900 + year } else { year };
    let months = full_year * 12 + month - 1;
    let first = format!("{:04}-{:02}-01", months.div_euclid(12), months.rem_euclid(12) + 1);
    let moved = LocalDate::parse(&first)?.add_days(day - 1 + days).to_string();
    // 10000년부터 `toISOString`은 `+010000-01-30...`이다. 잘라도 그 모양이 남는다.
    Some(if moved.len() > 10 { js::slice_utf16(&format!("+0{moved}"), 10) } else { moved })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 달력에_없는_날짜도_date_utc처럼_넘긴다() {
        let cases = [
            ("2026-03-05", 1, "2026-03-06"),
            ("2026-12-31", 1, "2027-01-01"),
            ("2026-02-30", 1, "2026-03-03"),
            ("2026-13-45", 1, "2027-02-15"),
            ("2026-00-00", 1, "2025-12-01"),
            ("0099-01-01", 0, "1999-01-01"),
            ("9999-12-31", 31, "+010000-01"),
        ];
        for (date, days, expected) in cases {
            assert_eq!(add_days(date, days).as_deref(), Some(expected), "{date}");
        }
    }

    #[test]
    fn 날짜는_앞_10자의_모양만_본다() {
        assert_eq!(all_day_date("2026-03-05T00:00:00Z").as_deref(), Ok("2026-03-05"));
        assert_eq!(all_day_date("2026-02-30").as_deref(), Ok("2026-02-30"));
        assert_eq!(all_day_date("not-a-date"), Err(ParseFailure::new("calendar_date")));
        assert_eq!(all_day_date("2026"), Err(ParseFailure::new("calendar_date")));
    }
}
