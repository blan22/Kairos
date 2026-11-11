//! budget: 묶음 시각, 점심, 접기, 합치기는 순수 함수다.
//! 나머지는 근무 시간 판단을 고정한다.
#![allow(clippy::unwrap_used)]

use kairos_core::{Instant, LocalDate, OffDates};
use kairos_policy::{FoldedLines, Mergeable, WorkSchedule, fold_lines, merge_group};

const MON: &str = "2026-03-02";
const TUE: &str = "2026-03-03";

fn kst(date: &str, clock: &str) -> Instant {
    Instant::parse(&format!("{date}T{clock}:00+09:00")).unwrap()
}

#[derive(Debug)]
struct Row {
    id: i64,
    channel: Option<&'static str>,
    opened_at: Instant,
}

impl Mergeable for Row {
    fn channel_id(&self) -> Option<&str> {
        self.channel
    }

    fn opened_at(&self) -> Option<Instant> {
        Some(self.opened_at)
    }
}

fn row(id: i64, channel: Option<&'static str>, minutes: i64) -> Row {
    Row { id, channel, opened_at: kst(MON, "10:00").plus_ms(minutes * 60_000) }
}

fn ids(rows: &[&Row]) -> Vec<i64> {
    rows.iter().map(|row| row.id).collect()
}

#[test]
fn budget_묶음_시각_점심_접기_합치기는_순수_함수다() {
    let none = OffDates::new();
    let schedule = WorkSchedule::new(&none);
    assert_eq!(schedule.next_bundle_at(kst(MON, "10:00")), kst(MON, "13:30"));
    assert_eq!(schedule.next_bundle_at(kst(MON, "13:30")), kst(MON, "18:00"));
    assert_eq!(schedule.next_bundle_at(kst(MON, "18:00")), kst(TUE, "13:30"));
    assert_eq!(schedule.next_bundle_at(kst("2026-03-06", "18:30")), kst("2026-03-09", "13:30"), "주말을 건너뛴다");
    let off: OffDates = [LocalDate::parse(TUE).unwrap()].into_iter().collect();
    let with_off = WorkSchedule::new(&off);
    assert_eq!(with_off.next_bundle_at(kst(MON, "18:30")), kst("2026-03-04", "13:30"), "쉬는 날을 건너뛴다");
    assert_eq!(schedule.bundle_slot(kst(MON, "13:29")), None);
    assert_eq!(schedule.bundle_slot(kst(MON, "13:30")), Some(kst(MON, "13:30")));
    assert_eq!(schedule.bundle_slot(kst(MON, "18:10")), Some(kst(MON, "18:00")));
    assert!(schedule.is_lunch(kst(MON, "12:00")));
    assert!(!schedule.is_lunch(kst(MON, "13:30")));
    let lines = [1, 2, 3, 4, 5, 6, 7, 8, 9];
    assert_eq!(fold_lines(&lines, 0), FoldedLines { shown: vec![1, 2, 3, 4, 5, 6, 7], more: 2 });
    assert_eq!(fold_lines(&lines, 8), FoldedLines { shown: vec![1, 2, 3, 4, 5, 6, 7, 8], more: 1 });
    let rows = [row(1, Some("A"), 0), row(2, Some("B"), 1), row(3, Some("A"), 30), row(4, Some("A"), 31)];
    assert_eq!(ids(&merge_group(&rows)), [1, 3]);
    let no_channel = [row(1, None, 0), row(2, None, 1)];
    assert_eq!(ids(&merge_group(&no_channel)), [1]);
}

#[test]
fn 묶음이_없는_빈_입력은_빈_결과다() {
    let rows: [Row; 0] = [];
    assert!(merge_group(&rows).is_empty());
    let empty: [i32; 0] = [];
    assert_eq!(fold_lines(&empty, 0), FoldedLines { shown: vec![], more: 0 });
}

#[test]
fn 근무_시간은_10시_포함_19시_30분_제외다() {
    let none = OffDates::new();
    let schedule = WorkSchedule::new(&none);
    assert!(!schedule.is_work_hours(kst(MON, "09:59")));
    assert!(schedule.is_work_hours(kst(MON, "10:00")));
    assert!(schedule.is_work_hours(kst(MON, "19:29")));
    assert!(!schedule.is_work_hours(kst(MON, "19:30")));
}

#[test]
fn 다음_근무_시작은_오늘_시작_전이면_오늘이고_아니면_다음_근무일이다() {
    let none = OffDates::new();
    let schedule = WorkSchedule::new(&none);
    assert_eq!(schedule.next_work_start(kst(MON, "09:00")), kst(MON, "10:00"));
    assert_eq!(schedule.next_work_start(kst(MON, "10:00")), kst(TUE, "10:00"));
    assert_eq!(schedule.next_work_start(kst("2026-03-07", "09:00")), kst("2026-03-09", "10:00"), "토요일");
    let off: OffDates = [LocalDate::parse(MON).unwrap()].into_iter().collect();
    assert_eq!(WorkSchedule::new(&off).next_work_start(kst(MON, "09:00")), kst(TUE, "10:00"), "쉬는 날");
}
