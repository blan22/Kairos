//! 표시와 항목 엔진 시험(R10~R12).
//! 번호는 .codex/runs/2026-11-04-stage2.md의 표와 같다. 고정값은 모두 합성이다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

#[path = "../../kairos-agenda/tests/common/mod.rs"]
mod common;

use std::collections::BTreeSet;

use common::*;
use kairos_agenda::{
    ControlFile, DisplayMode, DisplayStart, HoldReason, ItemEngine, KairosControl, NextOptions, NextResponse,
    RefreshInput,
};
use kairos_core::model::{NormalizedObject, Source, SourceName};
use kairos_core::{KairosHome, OffDates};
use kairos_policy::ItemReason;
use kairos_store::Database;

/// 원본의 control. 표시가 켜져 있고 세션도 열렸다.
fn control() -> KairosControl {
    control_on().with_sessions(Some(T0))
}

fn ctx(now: &str) -> kairos_agenda::NextContext {
    context(now, &control(), Over::default())
}

fn ctx_with(now: &str, used: KairosControl) -> kairos_agenda::NextContext {
    context(now, &control(), Over::control(used))
}

fn next(db: &Database, now: &str) -> NextResponse {
    next_with(db, &ctx(now), NextOptions::default())
}

fn seen_card(when: &str) -> Database {
    let db = test_db();
    prime_slack(&db);
    slack(when).events(vec![mention("1", when)]).run(&db);
    refresh(&db, when);
    let response = next_with(&db, &ctx(when), NextOptions { seen: true, ack: false });
    assert!(response.card.is_some());
    db
}

fn response_of(db: &Database) -> Option<String> {
    db.connection().query_row("SELECT response FROM suggestions WHERE item_id = 1", [], |row| row.get(0)).unwrap()
}

#[test]
fn r10_점심_보류로_가려진_카드는_밀어내기로_세지_않는다() {
    let db = seen_card(&kst(MON, "11:55"));
    assert_eq!(next(&db, &kst(MON, "12:00")).hold.unwrap().reason, HoldReason::Lunch);
    assert_eq!(next(&db, &kst(MON, "12:56")).hold.unwrap().reason, HoldReason::Lunch);
    assert_eq!(item(&db, 1).push_count, 0);
    assert_eq!(response_of(&db).as_deref(), Some("superseded"));
    assert_eq!(next(&db, &kst(MON, "13:31")).card.unwrap().item_ids, [1], "보류가 끝나면 다시 낸다");
}

#[test]
fn r10_일시정지와_포커스도_같고_수집_틱의_만료도_보류_중에는_세지_않는다() {
    let db = seen_card(&kst(MON, "10:00"));
    let paused = control().with_paused_until(Some("indefinite"));
    next_with(&db, &ctx_with(&kst(MON, "11:30"), paused), NextOptions::default());
    assert_eq!(item(&db, 1).push_count, 0);
    let focusing = control().with_focus(&kst(MON, "10:05"), &kst(MON, "11:30"));
    let other = seen_card(&kst(MON, "10:00"));
    next_with(&other, &ctx_with(&kst(MON, "11:10"), focusing), NextOptions::default());
    assert_eq!(item(&other, 1).push_count, 0);
    let tick = seen_card(&kst(MON, "11:55"));
    let (off_dates, ok_sources) = (OffDates::new(), BTreeSet::new());
    let input =
        RefreshInput { now: at(&kst(MON, "12:56")), off_dates: &off_dates, ok_sources: &ok_sources, cards_held: true };
    ItemEngine::new(&tick).refresh(&input).unwrap();
    assert_eq!(item(&tick, 1).push_count, 0);
}

#[test]
fn r10_대조_보이는_동안_응답이_없으면_그대로_밀어내기_1회다() {
    let db = seen_card(&kst(MON, "10:00"));
    next(&db, &kst(MON, "11:05"));
    assert_eq!(item(&db, 1).push_count, 1);
    assert_eq!(response_of(&db).as_deref(), Some("expired"));
}

/// 연결된 Slack 하나만 있는 소스 목록. 게이트는 그 T0만 본다.
fn slack_only() -> FakeSource {
    FakeSource { name: SourceName::Slack, queries: Vec::new(), reason: None }
}

fn home_db() -> (tempfile::TempDir, KairosHome, Database) {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    (dir, home, db)
}

#[test]
fn r11_한_번_열린_표시_게이트는_첫_기록일을_사후에_쉬는_날로_넣어도_닫히지_않는다() {
    let (_dir, home, db) = home_db();
    let t0 = kst(MON, "09:30");
    prime_at(&db, SourceName::Slack, "tick", &t0);
    let source = slack_only();
    let sources: [&dyn Source; 1] = [&source];
    let now = kst(TUE, "11:00");
    let auto = KairosControl::default();
    let file = ControlFile::new(&home);
    file.write(&auto).unwrap();
    let latched = file.latch_display(db.connection(), &sources, at(&now), auto).unwrap();
    assert!(gate_at(Some(&t0), &latched, &now, &none()).open);
    let with_off = file.read().with_off_dates(&[MON.to_owned()]);
    let off_dates = off(&[MON]);
    let start = DisplayStart::compute(Some(&t0), at(&now), &off_dates).unwrap();
    assert!(!start.ready, "판정 재료는 다시 계산돼도");
    assert!(gate_at(Some(&t0), &with_off, &now, &off_dates).open, "게이트는 되돌아가지 않는다");
}

#[test]
fn r11_대조_아직_열린_적_없으면_쉬는_날을_반영하고_일시정지와_끄기는_그대로_닫는다() {
    let (_dir, home, db) = home_db();
    let t0 = kst(MON, "09:30");
    prime_at(&db, SourceName::Slack, "tick", &t0);
    let source = slack_only();
    let sources: [&dyn Source; 1] = [&source];
    let before = kst(MON, "15:00");
    let plain = KairosControl::default();
    let latched = ControlFile::new(&home).latch_display(db.connection(), &sources, at(&before), plain.clone()).unwrap();
    assert!(!latched.display_opened());
    let opened = plain.with_display_opened_at(&before);
    let now = kst(TUE, "11:00");
    let paused = opened.clone().with_paused_until(Some("indefinite"));
    assert!(!gate_at(Some(&t0), &paused, &now, &none()).open);
    let off_mode = opened.with_display_mode(DisplayMode::Off);
    assert!(!gate_at(Some(&t0), &off_mode, &now, &none()).open);
}

#[test]
fn r12_달력에_없는_날짜의_일정도_항목을_막지_않는다() {
    let db = test_db();
    prime(&db, SourceName::Calendar, "mine");
    prime_slack(&db);
    let bad = NormalizedObject {
        all_day: Some(true),
        start_date: Some("2026-13-45".into()),
        end_date: Some("2026-13-46".into()),
        start_at: None,
        end_at: None,
        ..meeting("bad")
    };
    calendar("mine", T0).objects(vec![bad]).run(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    let report = refresh(&db, T0);
    assert_eq!(report.skipped, 0);
    assert_eq!(count(&db, "items"), 2);
    let new_meeting = items(&db).into_iter().find(|row| row.reason == ItemReason::MeetingNew).unwrap();
    assert_eq!(new_meeting.due_at, None);
}

#[test]
fn r12_이벤트_한_건의_예외는_그_건만_건너뛰고_코드를_남긴다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0), mention_in("2", T1, "C0SYNTH2")]).run(&db);
    db.connection().execute_batch(&format!(
        "CREATE TRIGGER boom BEFORE INSERT ON items WHEN NEW.channel_id = '{CHANNEL}' BEGIN SELECT RAISE(ABORT, 'synthetic'); END"
    ))
    .unwrap();
    let report = refresh(&db, T1);
    assert_eq!(report.skipped, 1);
    let channels: Vec<_> = items(&db).into_iter().map(|row| row.channel_id).collect();
    assert_eq!(channels, [Some("C0SYNTH2".to_owned())]);
    db.connection().execute_batch("DROP TRIGGER boom").unwrap();
    assert_eq!(refresh(&db, T2).skipped, 0);
    assert_eq!(count(&db, "items"), 2, "풀리면 다음 틱에 연다");
}
