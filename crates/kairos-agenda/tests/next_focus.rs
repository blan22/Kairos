//! 포커스와 보류.
#![allow(clippy::unwrap_used)]

mod common;
mod next_support;

use common::*;
use kairos_agenda::{FocusTracker, HoldReason, ItemTier};
use kairos_core::model::{NormalizedObject, SourceName};
use next_support::*;

fn with_calendar() -> kairos_store::Database {
    let db = seeded();
    prime(&db, SourceName::Calendar, "mine");
    db
}

#[test]
fn 포커스_중_p1을_보류하고_p0는_무시한다_끝나면_쌓인_건수를_한_줄로_낸다() {
    let db = with_calendar();
    let focused = FocusTracker::start(control(), 50, at(&kst(MON, "10:00")));
    assert_eq!(focused.focus().unwrap().unwrap().until, kst(MON, "10:50"));
    p1(&db, &kst(MON, "10:05"), &["C1", "C2"]);
    let held = peek_over(&db, &kst(MON, "10:06"), Over::control(focused.clone()));
    assert_eq!(held.card, None);
    let hold = held.hold.unwrap();
    assert_eq!((hold.reason, hold.until), (HoldReason::Focus, Some(kst(MON, "10:50"))));
    assert!(held.focus.active);

    let object = meeting_at("p0", &format!("{MON}T11:00:00+09:00"), &format!("{MON}T11:30:00+09:00"));
    calendar("mine", &kst(MON, "10:10")).objects(vec![object]).run(&db);
    refresh(&db, &kst(MON, "10:10"));
    let urgent = peek_over(&db, &kst(MON, "10:11"), Over::control(focused.clone()));
    assert_eq!(urgent.card.unwrap().tier, ItemTier::P0);
    actions(&db).done(&[3], at(&kst(MON, "10:12")), false).unwrap();

    let after = peek_over(&db, &kst(MON, "10:51"), Over::control(focused));
    assert!(!after.focus.active);
    assert_eq!(after.focus.held_line.as_deref(), Some("포커스 중 쌓인 P1 2건"));
    assert_eq!(after.card.unwrap().tier, ItemTier::P1);
}

#[test]
fn 포커스는_최대_90분이고_회의_5분_전이나_발견_시각에_끝난다() {
    let db = with_calendar();
    let tracker = FocusTracker::new(db.connection());
    let start = at(&kst(MON, "10:00"));
    assert_eq!(FocusTracker::start(control(), 300, start).focus().unwrap().unwrap().until, kst(MON, "11:30"));
    let started = FocusTracker::start(control(), 90, start);
    let object = meeting_at("m", &format!("{MON}T10:40:00+09:00"), &format!("{MON}T11:00:00+09:00"));
    calendar("mine", &kst(MON, "10:20")).objects(vec![object]).run(&db);
    assert_eq!(tracker.view(&started, at(&kst(MON, "10:21"))).unwrap().until, Some(kst(MON, "10:35")));
    assert!(!tracker.view(&started, at(&kst(MON, "10:36"))).unwrap().active);
    // 회의가 5분 안에 시작할 때 발견되면 발견 시각에 끝난다.
    let soon = meeting_at("n", &format!("{MON}T10:22:00+09:00"), &format!("{MON}T10:30:00+09:00"));
    calendar("mine", &kst(MON, "10:21")).objects(vec![soon]).run(&db);
    assert_eq!(tracker.view(&started, at(&kst(MON, "10:21"))).unwrap().until, Some(kst(MON, "10:21")));
}

#[test]
fn 거절한_회의와_취소된_회의는_포커스를_끝내지_않는다() {
    let db = with_calendar();
    let started = FocusTracker::start(control(), 90, at(&kst(MON, "10:00")));
    let end = format!("{MON}T11:00:00+09:00");
    let declined = NormalizedObject {
        my_response: Some("declined".into()),
        ..meeting_at("d", &format!("{MON}T10:40:00+09:00"), &end)
    };
    let cancelled =
        NormalizedObject { state: Some("cancelled".into()), ..meeting_at("c", &format!("{MON}T10:41:00+09:00"), &end) };
    calendar("mine", &kst(MON, "10:20")).objects(vec![declined, cancelled]).run(&db);
    let view = FocusTracker::new(db.connection()).view(&started, at(&kst(MON, "10:21"))).unwrap();
    assert_eq!(view.until, Some(kst(MON, "11:30")));
}

#[test]
fn 포커스_end는_바로_끝내고_쌓인_건수를_알려_준다() {
    let db = seeded();
    let started = FocusTracker::start(control(), 50, at(&kst(MON, "10:00")));
    p1(&db, &kst(MON, "10:05"), &["C1"]);
    let ended = FocusTracker::end(started, at(&kst(MON, "10:10")));
    let tracker = FocusTracker::new(db.connection());
    let view = tracker.view(&ended, at(&kst(MON, "10:10"))).unwrap();
    assert_eq!((view.active, view.held_line.as_deref()), (false, Some("포커스 중 쌓인 P1 1건")));
    assert_eq!(tracker.view(&ended, at(&kst(MON, "11:11"))).unwrap().held_line, None, "한 시간이 지나면 내린다");
}

#[test]
fn 점심시간과_근무_시간_밖_쉬는_날에는_p1을_보류하고_p0는_낸다() {
    let db = with_calendar();
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    let lunch = peek(&db, &kst(MON, "12:30")).hold.unwrap();
    assert_eq!((lunch.reason, lunch.until), (HoldReason::Lunch, Some(kst(MON, "13:30"))));
    assert_eq!(peek(&db, &kst(MON, "09:30")).hold.unwrap().reason, HoldReason::OffHours);
    assert_eq!(peek(&db, &kst(MON, "19:30")).hold.unwrap().until, Some(kst(TUE, "10:00")));
    let holiday = peek_over(&db, &kst(MON, "11:00"), Over::off_dates(off(&[MON])));
    let hold = holiday.hold.unwrap();
    assert_eq!((hold.reason, hold.until), (HoldReason::DayOff, Some(kst(TUE, "10:00"))));
    assert_eq!(holiday.counts.open.p1, 1, "복귀일 건수는 보인다");

    let object = meeting_at("p0", &format!("{MON}T13:00:00+09:00"), &format!("{MON}T13:30:00+09:00"));
    calendar("mine", &kst(MON, "12:20")).objects(vec![object]).run(&db);
    refresh(&db, &kst(MON, "12:20"));
    assert_eq!(peek(&db, &kst(MON, "12:30")).card.unwrap().tier, ItemTier::P0);
    let on_holiday = peek_over(&db, &kst(MON, "12:30"), Over::off_dates(off(&[MON])));
    assert_eq!(on_holiday.card.unwrap().tier, ItemTier::P0);
}
