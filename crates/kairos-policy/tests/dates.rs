//! kairos-core의 `LocalDate`, `Instant`, `OffDates`를 시험한다
#![allow(clippy::unwrap_used)]

use kairos_core::{Instant, LocalDate, OffDates};

fn date(text: &str) -> LocalDate {
    LocalDate::parse(text).unwrap()
}

fn instant(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

#[test]
fn 종일_일정_날짜_함정_z가_붙어도_앞_10자가_그_날짜다() {
    assert_eq!(LocalDate::from_all_day("2026-11-05T00:00:00Z"), Some(date("2026-11-05")));
    assert_eq!(LocalDate::from_all_day("2026-11-05"), Some(date("2026-11-05")));
    // 순간으로 바꿔 KST로 읽으면 같은 날 09:00이 되고, 음의 오프셋이면 전날이 된다.
    assert_eq!(LocalDate::from_all_day("2026-11-05T00:00:00-08:00"), Some(date("2026-11-05")));
}

#[test]
fn 날짜가_아니면_거부한다() {
    assert_eq!(LocalDate::from_all_day("11/05/2026"), None);
}

#[test]
fn kst_날짜_경계() {
    assert_eq!(instant("2026-11-04T14:59:00Z").local_date(), date("2026-11-04"));
    assert_eq!(instant("2026-11-04T15:00:00Z").local_date(), date("2026-11-05"));
    assert_eq!(instant("2026-11-05T00:30:00+09:00").local_date(), date("2026-11-05"));
}

#[test]
fn kst_분() {
    assert_eq!(instant("2026-11-04T00:00:00Z").local_minutes(), 9 * 60);
    assert_eq!(instant("2026-11-04T15:00:00Z").local_minutes(), 0);
}

#[test]
fn 달력_덧셈은_월_경계를_넘는다() {
    assert_eq!(date("2026-02-28").add_days(1), date("2026-03-01"));
    assert_eq!(date("2026-03-01").add_days(-1), date("2026-02-28"));
}

#[test]
fn 주말() {
    assert!(date("2026-11-07").is_weekend());
    assert!(date("2026-11-08").is_weekend());
    assert!(!date("2026-11-09").is_weekend());
}

#[test]
fn 직전_근무일은_주말과_쉬는_날을_건너뛴다() {
    assert_eq!(OffDates::new().previous_workday(date("2026-11-09")), date("2026-11-06"));
    let off: OffDates = [date("2026-11-06")].into_iter().collect();
    assert_eq!(off.previous_workday(date("2026-11-09")), date("2026-11-05"));
}
