//! 기본 일정(언제나)에서의 날짜 규칙을 시험한다. 시험 프로필은 근무 일정이라 이 파일만 기본 일정을 쓴다.
#![allow(clippy::unwrap_used)]

use kairos_core::profile::{self, Profile, Schedule};
use kairos_core::{Instant, LocalDate, OffDates};
use kairos_policy::{DeployLinker, DeployStage, WorkSchedule, morning_minutes};

fn always() {
    profile::install(Profile { schedule: Schedule::default(), ..Profile::fixture() }).unwrap();
}

fn date(text: &str) -> LocalDate {
    LocalDate::parse(text).unwrap()
}

#[test]
fn 모든_요일이_근무일이어도_배포_전_근무일은_주말을_건너뛴다() {
    always();
    let off = OffDates::new();
    // 2026-11-23은 월요일이다.
    assert_eq!(off.previous_workday(date("2026-11-23")), date("2026-11-20"));
    assert!(off.is_workday(date("2026-11-22")), "기본 일정에서는 일요일도 근무일이다");
    let holiday: OffDates = [date("2026-11-20")].into_iter().collect();
    assert_eq!(holiday.previous_workday(date("2026-11-23")), date("2026-11-19"));
}

#[test]
fn 월요일_배포의_d1은_금요일부터다() {
    always();
    let stage = |today: &str| DeployLinker::stage(date("2026-11-23"), date(today), &OffDates::new());
    assert_eq!(stage("2026-11-19"), None);
    assert_eq!(stage("2026-11-20"), Some(DeployStage::D1));
    assert_eq!(stage("2026-11-22"), Some(DeployStage::D1));
    assert_eq!(stage("2026-11-23"), Some(DeployStage::D0));
}

#[test]
fn 근무가_자정에_시작하면_아침은_9시다() {
    always();
    assert_eq!(morning_minutes(), 9 * 60);
}

#[test]
fn 밤과_주말에도_근무_시간이고_점심_보류가_없다() {
    always();
    let off = OffDates::new();
    let schedule = WorkSchedule::new(&off);
    // 2026-11-22(일) 03:00, 12:30, 23:59 KST.
    for at in ["2026-11-21T18:00:00Z", "2026-11-22T03:30:00Z", "2026-11-22T14:59:00Z"] {
        let now = Instant::parse(at).unwrap();
        assert!(schedule.is_work_hours(now), "{at}");
        assert!(!schedule.is_lunch(now), "{at}");
    }
}
