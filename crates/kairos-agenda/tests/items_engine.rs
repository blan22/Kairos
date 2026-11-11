//! 항목 엔진: 열기, 모으기, 닫기, 회의, 배포.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_agenda::{ItemCloseReason, ItemState, ItemTier};
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, NormalizedObject, SourceName};
use kairos_ledger::check_invariants;
use kairos_policy::ItemReason;
use kairos_store::Database;

fn key(ts: &str) -> String {
    format!("slack:{CHANNEL}:{ts}")
}

fn states(db: &Database) -> Vec<ItemState> {
    items(db).iter().map(|row| row.state).collect()
}

#[test]
fn baseline_이벤트는_항목을_열지_않는다() {
    let db = test_db();
    // 첫 실행이라 모두 baseline이다.
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    assert_eq!(event_flag(&db, &key("1"), "baseline"), 1);
    assert_eq!(refresh(&db, T0).opened, 0);
    assert_eq!(count(&db, "items"), 0);
}

#[test]
fn 기록만인_이벤트와_접힌_이벤트는_항목을_열지_않는다() {
    let db = test_db();
    prime(&db, SourceName::Github, "tick");
    github(T0).objects(vec![issue(1)]).run(&db);
    let bot =
        NormalizedEvent { actor_kind: ActorKind::Bot, actor_id: Some("synthetic[bot]".into()), ..comment("b", T1) };
    github(T2).events(vec![bot, comment("1", T1), comment("2", T2)]).run(&db);
    assert_eq!(event_text(&db, "gh:comment:2", "disposition").as_deref(), Some("folded"));
    assert_eq!(refresh(&db, T2).opened, 1);
    assert_eq!(count(&db, "items"), 1);
    let row = item(&db, 1);
    assert_eq!(
        (row.reason, row.tier, row.state, row.opened_by_event.as_deref()),
        (ItemReason::ReplyOwed, ItemTier::P2, ItemState::Open, Some("gh:comment:1"))
    );
}

#[test]
fn 규칙의_등급과_이유로_연다_slack처럼_객체가_없으면_스레드가_키다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    let row = item(&db, 1);
    assert_eq!(row.object_key, key("1"));
    assert_eq!((row.reason, row.tier), (ItemReason::ReplyOwed, ItemTier::P1));
    assert_eq!((row.source.as_deref(), row.channel_id.as_deref()), (Some("slack"), Some(CHANNEL)));
    assert_eq!(row.title, None, "외부 제목을 항목에 복사하지 않는다");
    assert_eq!(row.due_at, Some(plus_minutes(T0, 90)), "P1은 최대 보류 90분이 due_at이다");
}

#[test]
fn 같은_스레드의_이벤트는_한_항목으로_모이고_높은_등급으로_올린다() {
    let db = test_db();
    prime_slack(&db);
    let thread = key("1");
    slack(T0).events(vec![NormalizedEvent { mention_count: Some(4), ..mention("1", T0) }]).run(&db);
    assert_eq!(refresh(&db, T0).opened, 1);
    assert_eq!(item(&db, 1).tier, ItemTier::P2);
    slack(T1).events(vec![NormalizedEvent { thread_key: Some(thread), ..mention("2", T1) }]).run(&db);
    let second = refresh(&db, T1);
    assert_eq!((second.opened, second.folded), (0, 1));
    assert_eq!(count(&db, "items"), 1);
    assert_eq!(item(&db, 1).tier, ItemTier::P1);
}

#[test]
fn 다시_돌려도_같은_결과다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    let again = refresh(&db, T1);
    assert_eq!((again.opened, again.folded, again.closed), (0, 0, 0));
    assert_eq!(count(&db, "items"), 1);
}

#[test]
fn 양성_증거로_닫고_근거_이벤트를_남긴다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    slack(T1).events(vec![my_message("2", T1, &key("1"))]).run(&db);
    assert_eq!(refresh(&db, T1).closed, 1);
    let row = item(&db, 1);
    assert_eq!(row.state, ItemState::Closed);
    assert_eq!(row.close_reason, Some(ItemCloseReason::DoneEvidence));
    assert_eq!(row.close_evidence_event, Some(key("2")));
    assert_eq!(check_invariants(db.connection()).unwrap().item_closures_without_evidence, 0);
}

#[test]
fn 결과에_없다는_사실만으로는_닫지_않는다() {
    let db = test_db();
    prime(&db, SourceName::Github, "tick");
    github(T0).objects(vec![issue(1)]).run(&db);
    github(T1).events(vec![comment("1", T1)]).run(&db);
    refresh(&db, T1);
    // 객체가 스냅샷에서 빠지고 ok 실행이 이어져도 증거는 없다.
    for when in [T2, T3] {
        github(when).snapshot().run(&db);
        assert_eq!(refresh(&db, when).closed, 0);
    }
    assert_eq!(item(&db, 1).state, ItemState::Open);
}

#[test]
fn ok가_아닌_실행의_증거로는_닫지_않고_다음_ok에서_닫는다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    slack(T1).events(vec![my_message("2", T1, &key("1"))]).partial().run(&db);
    assert_eq!(refresh(&db, T1).closed, 0);
    assert_eq!(item(&db, 1).state, ItemState::Open);
    slack(T2).run(&db);
    assert_eq!(refresh(&db, T2).closed, 1);
}

#[test]
fn 모인_이벤트가_모두_닫혀야_항목이_닫힌다() {
    let db = test_db();
    prime_slack(&db);
    let thread = key("1");
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    // 내 답 뒤에 새 멘션이 왔다. 첫 이벤트만 닫히고 항목은 남는다.
    let reply = NormalizedEvent { thread_key: Some(thread.clone()), ..mention("3", T2) };
    slack(T2).events(vec![my_message("2", T1, &thread), reply]).run(&db);
    assert_eq!(event_text(&db, &key("1"), "close_reason").as_deref(), Some("done_evidence"));
    assert_eq!(refresh(&db, T2).closed, 0);
    assert_eq!(states(&db), [ItemState::Open]);
    slack(T3).events(vec![my_message("4", T3, &thread)]).run(&db);
    assert_eq!(refresh(&db, T3).closed, 1);
    assert_eq!(item(&db, 1).close_evidence_event, Some(key("4")));
}

#[test]
fn 닫힌_항목은_되살리지_않고_같은_일은_새_행이다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    actions(&db).done(&[1], at(T0), false).unwrap();
    slack(T1).events(vec![NormalizedEvent { thread_key: Some(key("1")), ..mention("2", T1) }]).run(&db);
    refresh(&db, T1);
    assert_eq!(states(&db), [ItemState::Closed, ItemState::Open]);
    assert_eq!(item(&db, 1).close_reason, Some(ItemCloseReason::UserDone));
}

#[test]
fn 사용자가_끝낸_항목의_열린_이벤트가_다음_틱에_새_항목을_만들지_않는다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    actions(&db).done(&[1], at(T0), false).unwrap();
    assert_eq!(refresh(&db, T1).opened, 0);
    assert_eq!(count(&db, "items"), 1);
}

#[test]
fn work_owed는_사용자_조작으로만_닫힌다() {
    let db = test_db();
    prime(&db, SourceName::Gmail, "recent");
    let object_key = "gmail:t1";
    let mail = NormalizedEvent {
        actor_id: Some("alerts@sentry.io".into()),
        object_key: Some(object_key.into()),
        text_head: Some("합성 경보".into()),
        ..NormalizedEvent::new(object_key, SourceName::Gmail, EventType::MailThread, T0, ActorKind::Human)
    };
    Push::from(SourceName::Gmail, "recent", T0).events(vec![mail]).run(&db);
    refresh(&db, T0);
    assert_eq!((item(&db, 1).reason, item(&db, 1).tier), (ItemReason::WorkOwed, ItemTier::P1));
    Push::from(SourceName::Gmail, "recent", T3).run(&db);
    assert_eq!(refresh(&db, T3).closed, 0);
    actions(&db).done(&[1], at(T3), false).unwrap();
    assert_eq!(item(&db, 1).close_reason, Some(ItemCloseReason::UserDone));
}

#[test]
fn 회의_종료_시각이_지나면_expired로_닫는다() {
    let db = test_db();
    prime(&db, SourceName::Calendar, "mine");
    let object = meeting_at("m1", &format!("{MON}T11:30:00+09:00"), &format!("{MON}T12:00:00+09:00"));
    calendar("mine", T0).objects(vec![object.clone()]).run(&db);
    refresh(&db, T0);
    assert_eq!(item(&db, 1).reason, ItemReason::MeetingNew);
    let after = kst(MON, "12:05");
    calendar("mine", &after).objects(vec![object]).run(&db);
    refresh(&db, &after);
    let row = item(&db, 1);
    assert_eq!((row.state, row.close_reason), (ItemState::Closed, Some(ItemCloseReason::Expired)));
    assert_eq!(row.close_evidence_event, None);
}

#[test]
fn 회의_ok_스냅샷_2회_연속_부재면_withdrawn으로_닫고_철회가_새_항목을_만들지_않는다() {
    let db = test_db();
    prime(&db, SourceName::Calendar, "mine");
    let object = meeting_at("m2", &format!("{TUE}T15:00:00+09:00"), &format!("{TUE}T16:00:00+09:00"));
    calendar("mine", T0).objects(vec![object]).snapshot().run(&db);
    refresh(&db, T0);
    for when in [T1, T2] {
        calendar("mine", when).snapshot().run(&db);
        refresh(&db, when);
    }
    assert_eq!(count(&db, "items"), 1);
    let row = item(&db, 1);
    assert_eq!((row.state, row.close_reason), (ItemState::Closed, Some(ItemCloseReason::Withdrawn)));
    assert!(row.close_evidence_event.unwrap_or_default().contains("#withdrawn:"));
    assert_eq!(refresh(&db, T3).opened, 0);
}

#[test]
fn 회의_취소는_열린_항목을_withdrawn으로_닫는다() {
    let db = test_db();
    prime(&db, SourceName::Calendar, "mine");
    let object = meeting_at("m3", &format!("{TUE}T15:00:00+09:00"), &format!("{TUE}T16:00:00+09:00"));
    calendar("mine", T0).objects(vec![object.clone()]).run(&db);
    refresh(&db, T0);
    let cancelled = NormalizedObject { state: Some("cancelled".into()), ..object };
    calendar("mine", T1).objects(vec![cancelled]).run(&db);
    refresh(&db, T1);
    assert_eq!(count(&db, "items"), 1);
    assert_eq!((item(&db, 1).state, item(&db, 1).close_reason), (ItemState::Closed, Some(ItemCloseReason::Withdrawn)));
}

#[test]
fn 회의가_60분_안으로_옮겨지면_같은_항목이_p0가_된다() {
    let db = test_db();
    prime(&db, SourceName::Calendar, "mine");
    let object = meeting_at("m4", &format!("{TUE}T15:00:00+09:00"), &format!("{TUE}T16:00:00+09:00"));
    calendar("mine", T0).objects(vec![object]).run(&db);
    refresh(&db, T0);
    assert_eq!(item(&db, 1).tier, ItemTier::P2);
    let moved = meeting_at("m4", &format!("{MON}T10:30:00+09:00"), &format!("{MON}T11:00:00+09:00"));
    calendar("mine", T1).objects(vec![moved]).run(&db);
    refresh(&db, T1);
    assert_eq!(count(&db, "items"), 1);
    assert_eq!(item(&db, 1).tier, ItemTier::P0);
}

fn seed_deploy(db: &Database, when: &str) -> (NormalizedObject, NormalizedObject) {
    prime(db, SourceName::Github, "tick");
    prime(db, SourceName::Calendar, "team");
    let linked = NormalizedObject { milestone_title: Some("합성 기능".into()), ..issue(7) };
    let event =
        NormalizedObject { start_date: Some("2026-03-03".into()), end_date: Some("2026-03-04".into()), ..deploy("d1") };
    github(when).objects(vec![linked.clone()]).snapshot().run(db);
    calendar("team", when).objects(vec![event.clone()]).run(db);
    (linked, event)
}

#[test]
fn 배포_d_1은_p1_당일은_같은_항목을_p0로_올리고_연결_0건이면_닫는다() {
    let db = test_db();
    let (linked, event) = seed_deploy(&db, T0);
    refresh(&db, T0);
    assert_eq!(
        items(&db).iter().map(|row| (row.reason, row.tier)).collect::<Vec<_>>(),
        [(ItemReason::DeployRisk, ItemTier::P1)]
    );
    let day = kst(TUE, "10:00");
    calendar("team", &day).objects(vec![event]).run(&db);
    refresh(&db, &day);
    let rows: Vec<_> = items(&db).iter().map(|row| (row.reason, row.tier, row.state)).collect();
    assert_eq!(rows, [(ItemReason::DeployRisk, ItemTier::P0, ItemState::Open)]);
    let later = kst(TUE, "10:03");
    let closed = NormalizedObject { state: Some("closed".into()), ..linked };
    github(&later).objects(vec![closed]).snapshot().run(&db);
    refresh(&db, &later);
    let row = item(&db, 1);
    assert_eq!((row.state, row.close_reason), (ItemState::Closed, Some(ItemCloseReason::Withdrawn)));
    assert!(row.close_evidence_event.unwrap_or_default().contains("cleared"));
}

#[test]
fn 배포_일정_종료가_지나면_expired로_닫는다() {
    let db = test_db();
    let (_, event) = seed_deploy(&db, T0);
    refresh(&db, T0);
    let after = kst("2026-03-04", "10:00");
    calendar("team", &after).objects(vec![event]).run(&db);
    refresh(&db, &after);
    assert_eq!((item(&db, 1).state, item(&db, 1).close_reason), (ItemState::Closed, Some(ItemCloseReason::Expired)));
}

#[test]
fn 규칙에_없는_rule_id의_이벤트로는_항목이_생기지_않는다() {
    let db = test_db();
    db.connection().execute_batch(&format!(
        "INSERT INTO events (dedupe_key, source, thread_key, type, actor_kind, occurred_at, first_seen_at, run_id, disposition, rule_id)
         VALUES ('x', 'slack', 'th', 'slack_mention', 'human', '{T0}', '{T0}', 1, 'opened', 'p0.from_model')"
    ))
    .unwrap();
    assert_eq!(refresh(&db, T0).opened, 0);
    assert_eq!(count(&db, "items"), 0);
}

#[test]
fn 공백_복구로_들어온_이벤트는_묶음으로_보낸다() {
    let db = test_db();
    prime_slack(&db);
    slack(T3).events(vec![mention("1", &plus_minutes(T3, -40))]).run(&db);
    refresh(&db, T3);
    assert!(item(&db, 1).bundle_queued);
}

#[test]
fn 엔진은_수집_저장소를_바꾸지_않는다_이벤트_처분과_종료는_그대로다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    let read = |column| event_text(&db, &key("1"), column);
    let before = (read("disposition"), read("rule_id"), read("close_reason"));
    refresh(&db, T0);
    assert_eq!((read("disposition"), read("rule_id"), read("close_reason")), before);
    slack(T1).run(&db);
    assert_eq!(count(&db, "items"), 1);
}

#[test]
fn 해석할_수_없는_사건_시각은_그_이벤트만_건너뛰고_다음_틱에_다시_본다() {
    // 원본에 없는 시험이다. `new Date(NaN).toISOString()`이 던지던 경로의 격리를 고정한다.
    let db = test_db();
    db.connection().execute_batch(
        "INSERT INTO events (dedupe_key, source, thread_key, type, actor_kind, occurred_at, first_seen_at, run_id, disposition, rule_id, channel_id)
         VALUES ('bad', 'slack', 'th', 'slack_mention', 'human', 'not-a-date', 'x', 1, 'opened', 'p1.direct_mention', 'C1')",
    )
    .unwrap();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    let report = refresh(&db, T0);
    assert_eq!((report.opened, report.skipped), (1, 1));
    assert_eq!(refresh(&db, T1).skipped, 1);
    assert_eq!(count(&db, "items"), 1);
}

fn postponed(event: NormalizedObject, start: &str, end: &str) -> NormalizedObject {
    NormalizedObject { start_date: Some(start.into()), end_date: Some(end.into()), ..event }
}

#[test]
fn 배포가_밀려_d1이_다시_오면_p1로_다시_알릴_차례가_된다() {
    let db = test_db();
    let (_linked, event) = seed_deploy(&db, T0);
    refresh(&db, T0);
    assert_eq!(item(&db, 1).tier, ItemTier::P1);
    db.connection().execute("UPDATE items SET p1_notified_at = ?1", [T0]).unwrap();

    let moved = postponed(event, "2026-03-05", "2026-03-06");
    let d1 = kst("2026-03-04", "10:00");
    calendar("team", &d1).objects(vec![moved.clone()]).run(&db);
    refresh(&db, &d1);
    let row = item(&db, 1);
    assert_eq!((row.state, row.tier, row.p1_notified_at), (ItemState::Open, ItemTier::P1, None));

    db.connection().execute("UPDATE items SET p1_notified_at = ?1", [T0]).unwrap();
    let again = kst("2026-03-04", "10:03");
    calendar("team", &again).objects(vec![moved]).run(&db);
    refresh(&db, &again);
    assert_eq!(item(&db, 1).p1_notified_at.as_deref(), Some(T0), "같은 단계는 다시 알리지 않는다");
}

#[test]
fn 당일에_밀린_배포의_d1은_당일_알림이_아니고_새_당일에_p0로_다시_알린다() {
    let db = test_db();
    let (_linked, event) = seed_deploy(&db, T0);
    let day = kst(TUE, "10:00");
    calendar("team", &day).objects(vec![event.clone()]).run(&db);
    refresh(&db, &day);
    assert_eq!(item(&db, 1).tier, ItemTier::P0);
    db.connection().execute("UPDATE items SET p0_notified_at = ?1", [T0]).unwrap();

    let moved = postponed(event, "2026-03-04", "2026-03-05");
    let later = kst(TUE, "10:03");
    calendar("team", &later).objects(vec![moved.clone()]).run(&db);
    refresh(&db, &later);
    let row = item(&db, 1);
    assert_eq!((row.tier, row.p1_notified_at), (ItemTier::P1, None));
    assert_eq!(row.p0_notified_at.as_deref(), Some(T0), "D-1은 P0로 알리지 않는다");

    let next = kst("2026-03-04", "10:00");
    calendar("team", &next).objects(vec![moved]).run(&db);
    refresh(&db, &next);
    let row = item(&db, 1);
    assert_eq!((row.tier, row.p0_notified_at), (ItemTier::P0, None));
}
