//! 서머타임이 있는 시간대. 프로필은 프로세스마다 하나라 이 시험 파일이 따로 정한다.
#![allow(clippy::unwrap_used)]

use kairos_core::profile::{self, Profile};
use kairos_core::{Instant, LocalDate};

fn new_york() {
    let mut profile = Profile::empty();
    profile.schedule.zone = chrono_tz::America::New_York;
    profile::install(profile).unwrap();
}

#[test]
fn 서머타임_전후의_현지_날짜와_분() {
    new_york();
    // 2026-03-08 02:00 EST → 03:00 EDT.
    let before = Instant::parse("2026-03-08T06:59:00Z").unwrap();
    assert_eq!((before.local_date().to_string(), before.local_minutes()), ("2026-03-08".to_owned(), 60 + 59));
    let after = Instant::parse("2026-03-08T07:00:00Z").unwrap();
    assert_eq!(after.local_minutes(), 3 * 60);
    let day = LocalDate::parse("2026-03-08").unwrap();
    assert_eq!(Instant::at_local(day, 10 * 60).to_iso(), "2026-03-08T14:00:00.000Z");
    assert_eq!(Instant::at_local(day.add_days(-1), 10 * 60).to_iso(), "2026-03-07T15:00:00.000Z");
    // 건너뛴 02:30은 앞선 오프셋(EST)으로 읽는다.
    assert_eq!(Instant::at_local(day, 2 * 60 + 30).to_iso(), "2026-03-08T07:30:00.000Z");
    // 오프셋 없는 시각 표기도 프로필 시간대다.
    assert_eq!(Instant::parse("2026-07-01T09:00:00").unwrap().to_iso(), "2026-07-01T13:00:00.000Z");
    // 두 번 있는 01:30(11월 1일)은 앞의 것(EDT)이다.
    let fall = LocalDate::parse("2026-11-01").unwrap();
    assert_eq!(Instant::at_local(fall, 90).to_iso(), "2026-11-01T05:30:00.000Z");
}
