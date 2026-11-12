//! 표시 게이트, 예산, 묶음, 밀어내기.
#![allow(clippy::unwrap_used)]

mod common;
mod next_support;

use common::*;
use kairos_agenda::{ActionContext, CardKind, DisplayMode, GateReason, HoldReason, ItemTier, KairosControl, Presenter};
use kairos_core::model::SourceName;
use kairos_store::Database;
use next_support::*;

fn later(db: &Database, ids: &[i64], now: &str) {
    let none = none();
    actions(db).later(ids, &ActionContext { now: at(now), off_dates: &none }).unwrap();
}

fn suggestion_text(db: &Database, column: &str) -> String {
    db.connection().query_row(&format!("SELECT {column} FROM suggestions"), [], |row| row.get(0)).unwrap()
}

#[test]
fn 표시_게이트_근무일_1일이_지나기_전에는_카드를_주지_않고_사유를_준다_항목_엔진은_계속_돈다() {
    let db = seeded();
    let auto = Over::control(KairosControl::default());
    p1(&db, T0, &["C1"]);
    assert_eq!(count(&db, "items"), 1);
    let closed = peek_over(&db, T1, auto.clone());
    assert!(closed.ok);
    assert_eq!(closed.card, None);
    assert_eq!(
        (closed.display.open, closed.display.mode, closed.display.reason),
        (false, DisplayMode::Auto, Some(GateReason::Recording))
    );
    assert_eq!(closed.display.starts_at.unwrap().to_iso(), kst(MON, "20:00"));
    assert_eq!(closed.hold.unwrap().reason, HoldReason::Recording);
    assert_eq!(see_over(&db, T1, auto.clone()).card, None);
    assert_eq!(count(&db, "suggestions"), 0);
    let ready = peek_over(&db, &kst(TUE, "10:30"), auto);
    assert!(ready.display.open);
    assert!(ready.card.is_some());
}

#[test]
fn 표시_게이트_수집_전_off_pause는_카드를_주지_않고_on은_auto를_넘는다() {
    let db = seeded();
    p1(&db, T0, &["C1"]);
    let none = none();
    let base = KairosControl::default();
    let gate = |control: &KairosControl, start: Option<&str>| gate_at(start, control, T1, &none);
    assert_eq!(gate(&base, None).reason, Some(GateReason::NoCollection));
    let off = base.clone().with_display_mode(DisplayMode::Off);
    assert_eq!(gate(&off, Some(BEFORE)).reason, Some(GateReason::DisplayOff));
    let paused = base.clone().with_paused_until(Some("indefinite")).with_display_mode(DisplayMode::On);
    assert_eq!(gate(&paused, Some(BEFORE)).reason, Some(GateReason::Paused));
    assert!(gate(&base.with_display_mode(DisplayMode::On), Some(BEFORE)).open);
    assert_eq!(peek_over(&db, T1, Over::control(off)).card, None);
}

#[test]
fn 설치는_표시_모드를_고른_적이_없을_때만_표시를_켠다() {
    let none = none();
    let gate = |control: &KairosControl, start: Option<&str>| gate_at(start, control, T1, &none);
    // 고른 적이 없으면 켠다. 기본값이면 닫혀 있을 수집 전과 기록 1근무일 안에도 열린다.
    let base = KairosControl::default();
    assert_eq!(gate(&base, None).reason, Some(GateReason::NoCollection));
    assert_eq!(gate(&base, Some(T0)).reason, Some(GateReason::Recording));
    let shown = base.clone().shown_from_install().unwrap();
    assert_eq!(shown.display_mode(), DisplayMode::On);
    assert!(gate(&shown, None).open && gate(&shown, Some(T0)).open);
    // 사용자가 고른 값은 지킨다. 직접 고른 auto는 기록 1근무일을 원한다는 뜻이다.
    for mode in [DisplayMode::On, DisplayMode::Off, DisplayMode::Auto] {
        assert!(base.clone().with_display_mode(mode).shown_from_install().is_none());
    }
    // 이미 열린 표시도 건드리지 않는다.
    assert!(base.with_display_opened_at(T1).shown_from_install().is_none());
}

#[test]
fn next는_읽기만_하고_seen이_노출과_예산을_기록한다() {
    let db = seeded();
    p1(&db, T0, &["C1"]);
    let first = peek(&db, T1);
    assert_eq!(first.card.as_ref().unwrap().kind, CardKind::Item);
    assert!(!first.card.unwrap().pending);
    assert_eq!(count(&db, "suggestions"), 0);
    assert_eq!(peek(&db, T1).budget.p1_hour_used, 0);

    let shown = see(&db, T1);
    assert!(shown.card.as_ref().unwrap().pending);
    assert_eq!(shown.budget.p1_hour_used, 1);
    assert_eq!(count(&db, "suggestions"), 1);
    assert_eq!(item(&db, 1).first_surfaced_at.as_deref(), Some(T1));

    let again = see(&db, T2);
    assert_eq!(again.card.unwrap().id, shown.card.unwrap().id);
    assert_eq!(again.budget.p1_hour_used, 1, "같은 카드를 다시 그려도 예산을 쓰지 않는다");
    assert_eq!(count(&db, "suggestions"), 1);
}

#[test]
fn p1은_시간당_1건이고_응답해도_시간_예산은_돌아오지_않는다() {
    let db = seeded();
    p1(&db, T0, &["C1", "C2"]);
    see(&db, T0);
    actions(&db).done(&[2], at(T1), false).unwrap();
    let held = peek(&db, T2);
    assert_eq!(held.card, None);
    let hold = held.hold.unwrap();
    assert_eq!(hold.reason, HoldReason::BudgetHour);
    assert_eq!(hold.until, Some(plus_minutes(T0, 60)));
    assert_eq!(held.counts.open.p1, 1, "상태 표시줄 건수와 목록은 예산과 무관하다");
    assert_eq!(list(&db, T2).items.len(), 1);
    assert_eq!(card_ids(&peek(&db, &plus_minutes(T0, 60)))[0], 1);
}

#[test]
fn p1은_하루_4건이고_초과분은_다음_묶음으로_간다() {
    let db = seeded();
    p1(&db, T0, &["C1", "C2", "C3", "C4", "C5"]);
    // 13:30 묶음은 P2가 없고 P1이 예산 안이면 P1 카드가 먼저다.
    for (index, time) in ["10:00", "11:01", "14:00", "15:01"].iter().enumerate() {
        let shown = see(&db, &kst(MON, time));
        assert_eq!(shown.card.unwrap().kind, CardKind::Item, "{time}");
        actions(&db).done(&[5 - index as i64], at(&kst(MON, time)), false).unwrap();
    }
    let over = peek(&db, &kst(MON, "16:10"));
    assert_eq!(over.budget.p1_day_used, 4);
    assert_eq!(over.card.as_ref().unwrap().kind, CardKind::Bundle, "이번 슬롯이 남아 있으면 초과분을 묶음으로 낸다");
    assert_eq!(card_ids(&over), [1]);
    see(&db, &kst(MON, "16:10"));
    assert_eq!(item(&db, 1).first_surfaced_at, Some(kst(MON, "16:10")));
}

#[test]
fn 같은_등급의_카드는_최신이_먼저다() {
    let db = seeded();
    let minute = |minutes| plus_minutes(&kst(MON, "10:30"), minutes);
    slack(&minute(0)).events(vec![mention_in("old", &minute(0), "C1")]).run(&db);
    refresh(&db, &minute(0));
    slack(&minute(60)).events(vec![mention_in("new", &minute(60), "C2")]).run(&db);
    refresh(&db, &minute(60));
    let newest = items(&db).iter().find(|row| row.object_key == "slack:C2:new").unwrap().id;
    assert_eq!(card_ids(&peek(&db, &minute(60)))[0], newest);
}

#[test]
fn 같은_채널의_30분_구간은_카드_1장이고_예산_1건이다() {
    let db = seeded();
    let minute = |minutes| plus_minutes(&kst(MON, "10:30"), minutes);
    let events = vec![
        mention("1", &minute(0)),
        mention("2", &minute(10)),
        mention("3", &minute(25)),
        mention_in("4", &minute(5), "C9"),
    ];
    slack(&minute(25)).events(events).run(&db);
    slack(&minute(40)).events(vec![mention("5", &minute(40))]).run(&db);
    refresh(&db, &minute(40));
    let shown = see(&db, &minute(40));
    let id_of = |ts: i64| items(&db).iter().find(|row| row.object_key == format!("slack:{CHANNEL}:{ts}")).unwrap().id;
    let card = shown.card.as_ref().unwrap();
    assert_eq!(card.item_ids, [id_of(5), id_of(3), id_of(2)]);
    assert_eq!(shown.budget.p1_hour_used, 1);
    assert_eq!(count(&db, "suggestions"), 3);
    let ids: Vec<&str> = card.actions.iter().map(|action| action.id.as_str()).collect();
    assert_eq!(ids, ["later", "done"], "합친 카드에는 시작과 남김이 없다");
    let mut argv = vec!["later".to_owned()];
    argv.extend(card.item_ids.iter().map(ToString::to_string));
    argv.push("--json".to_owned());
    assert_eq!(card.actions[0].argv, argv);
}

#[test]
fn 밀어내기는_2회까지이고_무응답_p1은_다음_근무일_첫_카드_맨_앞에_온다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    see(&db, &kst(MON, "10:00"));
    assert!(peek(&db, &kst(MON, "10:30")).card.unwrap().pending, "응답을 기다리는 동안 같은 카드를 유지한다");
    see(&db, &kst(MON, "11:01"));
    assert_eq!(item(&db, 1).push_count, 1, "응답 없이 넘어간 노출이 밀어내기다");
    let third = see(&db, &kst(MON, "14:02"));
    assert_eq!(item(&db, 1).push_count, 2);
    assert_eq!(third.card, None, "2회 뒤에는 오늘 다시 밀지 않는다");
    assert_eq!(third.counts.open.p1, 1, "일은 사라지지 않고 건수에 남는다");
    assert_eq!(list(&db, &kst(MON, "14:02")).items.len(), 1);

    p1(&db, &kst(TUE, "10:00"), &["C2"]);
    let morning = peek(&db, &kst(TUE, "10:05"));
    assert_eq!(morning.card.as_ref().unwrap().kind, CardKind::Carryover);
    assert_eq!(card_ids(&morning), [1]);
    assert!(morning.card.as_ref().unwrap().items[0].carried);
    assert_eq!(morning.counts.carried_p1, 1);
    see(&db, &kst(TUE, "10:05"));
    let later_card = peek(&db, &kst(TUE, "11:06"));
    assert_eq!(later_card.card.as_ref().unwrap().kind, CardKind::Item);
    assert_eq!(card_ids(&later_card), [2], "넘어온 카드는 하루 한 번이다");
}

#[test]
fn 근무일을_넘긴_p1은_새_p1보다_먼저이고_묶음의_n에_접히지_않는다() {
    let db = seeded();
    let channels: Vec<String> = (0..9).map(|index| format!("C{index}")).collect();
    let names: Vec<&str> = channels.iter().map(String::as_str).collect();
    p1(&db, &kst(MON, "19:00"), &names);
    let carry = peek(&db, &kst(TUE, "10:01")).card.unwrap();
    assert_eq!(carry.kind, CardKind::Carryover);
    assert_eq!(carry.items.len(), 9);
    assert_eq!(carry.more, 0);
}

#[test]
fn 넘어온_p1이_시간_예산에_걸리면_사유를_budget_hour로_낸다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1", "C2"]);
    db.connection().execute_batch("UPDATE items SET push_count = 2 WHERE id = 1").unwrap();
    actions(&db).done(&[2], at(&kst(MON, "10:01")), false).unwrap();
    p1(&db, &kst(TUE, "10:00"), &["C3"]);
    see(&db, &kst(TUE, "10:00"));
    actions(&db).done(&[1], at(&kst(TUE, "10:01")), false).unwrap();
    let hold = peek(&db, &kst(TUE, "10:02")).hold.unwrap();
    assert_eq!(hold.reason, HoldReason::BudgetHour);
    assert_eq!(hold.until, Some(kst(TUE, "11:00")));
}

#[test]
fn 나중에는_다음_묶음으로_보낸다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    see(&db, &kst(MON, "10:00"));
    later(&db, &[1], &kst(MON, "10:01"));
    assert_eq!(suggestion_text(&db, "response"), "later");
    assert_eq!(peek(&db, &kst(MON, "11:30")).card, None);
    let bundle = peek(&db, &kst(MON, "13:31"));
    assert_eq!(bundle.card.as_ref().unwrap().kind, CardKind::Bundle);
    assert_eq!(card_ids(&bundle), [1]);
}

#[test]
fn p2_묶음은_하루_2회이고_7줄을_넘으면_n으로_접는다() {
    let db = seeded();
    p2(&db, &kst(MON, "10:00"), &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(peek(&db, &kst(MON, "11:00")).hold.unwrap().reason, HoldReason::Nothing, "13:30 전에는 묶음이 없다");
    let first = see(&db, &kst(MON, "13:31"));
    let card = first.card.as_ref().unwrap();
    assert_eq!(card.kind, CardKind::Bundle);
    assert_eq!((card.items.len(), card.more, card.item_ids.len()), (7, 2, 9));
    assert_eq!(first.budget.bundles_today, 1);
    ack(&db, &kst(MON, "13:40"));
    p2(&db, &kst(MON, "14:00"), &[10]);
    assert_eq!(peek(&db, &kst(MON, "14:10")).card, None, "같은 슬롯에는 다시 내지 않는다");
    let second = see(&db, &kst(MON, "18:01"));
    assert_eq!(second.card.unwrap().items.len(), 1);
    ack(&db, &kst(MON, "18:02"));
    p2(&db, &kst(MON, "18:30"), &[11]);
    assert_eq!(peek(&db, &kst(MON, "19:00")).card, None, "하루 2회다");
}

#[test]
fn 묶음의_접힌_줄은_표시하지_않아도_표면에_올린_것으로_친다() {
    let db = seeded();
    p2(&db, &kst(MON, "10:00"), &[1, 2, 3, 4, 5, 6, 7, 8]);
    see(&db, &kst(MON, "13:31"));
    assert!(items(&db).iter().all(|row| row.first_surfaced_at.is_some()));
    assert_eq!(count(&db, "suggestions"), 8);
}

#[test]
fn p0는_예산을_쓰지_않고_기다리는_p1_카드를_밀어내되_밀어내기로_세지_않는다() {
    let db = seeded();
    prime(&db, SourceName::Calendar, "mine");
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    see(&db, &kst(MON, "10:00"));
    let object = meeting_at("p0", &format!("{MON}T10:45:00+09:00"), &format!("{MON}T11:00:00+09:00"));
    calendar("mine", &kst(MON, "10:10")).objects(vec![object]).run(&db);
    refresh(&db, &kst(MON, "10:10"));
    let urgent = see(&db, &kst(MON, "10:11"));
    assert_eq!(urgent.card.unwrap().tier, ItemTier::P0);
    assert_eq!(urgent.budget.p1_hour_used, 1);
    assert_eq!(item(&db, 1).push_count, 0);
    let response: String =
        db.connection().query_row("SELECT response FROM suggestions WHERE item_id = 1", [], |row| row.get(0)).unwrap();
    assert_eq!(response, "superseded");
    assert_eq!(see(&db, &kst(MON, "12:00")).card.unwrap().tier, ItemTier::P0, "P0 카드는 응답 전까지 만료되지 않는다");
}

#[test]
fn 항목이_자동으로_닫히면_기다리던_카드는_내려간다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    see(&db, &kst(MON, "10:00"));
    actions(&db).done(&[1], at(&kst(MON, "10:01")), false).unwrap();
    assert_eq!(peek(&db, &kst(MON, "10:02")).card, None);
    assert_eq!(suggestion_text(&db, "response"), "done");
}

#[test]
fn 공백_복구_항목은_카드가_아니라_묶음으로_간다() {
    let db = seeded();
    let when = kst(MON, "10:40");
    slack(&when).events(vec![mention("1", &kst(MON, "10:00"))]).run(&db);
    refresh(&db, &when);
    assert_eq!(peek(&db, &when).hold.unwrap().reason, HoldReason::Nothing);
    assert_eq!(peek(&db, &kst(MON, "13:31")).card.unwrap().kind, CardKind::Bundle);
}

#[test]
fn 총_표시_건수를_낸다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    let shown = see(&db, &kst(MON, "10:00"));
    assert_eq!(shown.budget.shown_today, 1);
    assert_eq!(shown.budget.day_target, 6);
    assert_eq!(shown.budget.p1_day_limit, 4);
}

#[test]
fn 패널에서_본_카드는_panel_표면으로_예산을_쓴다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1", "C2"]);
    see(&db, &kst(MON, "10:00"));
    assert_eq!(peek(&db, &kst(MON, "10:00")).budget.p1_hour_used, 1);
    assert_eq!(suggestion_text(&db, "surface"), "panel");
}

#[test]
fn 묶음과_넘어온_카드는_보류로_내려가면_다음_묶음에_다시_실린다() {
    // 원본에 없는 시험이다. holdCards의 kind 분기를 고정한다.
    let db = seeded();
    p2(&db, &kst(MON, "10:00"), &[1]);
    see(&db, &kst(MON, "13:31"));
    let hidden = Presenter::new(&db)
        .next_response(&ctx_over(&kst(MON, "19:40"), Over::default()), kairos_agenda::NextOptions::default())
        .unwrap();
    assert_eq!(hidden.hold.unwrap().reason, HoldReason::OffHours);
    assert_eq!(suggestion_text(&db, "response"), "superseded");
    assert!(item(&db, 1).bundle_queued);
}
