//! 사용자 조작, 상태 기계, triage.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_agenda::{
    ActionContext, ActionStatus, ItemCloseReason, ItemRepo, ItemState, ItemTier, ItemTransitions, ParkTrigger,
    TriageDecision,
};
use kairos_core::OffDates;
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, NormalizedObject, SourceName};
use kairos_ledger::check_invariants;
use kairos_policy::ItemReason;
use kairos_store::Database;

fn acting<'a>(now: &str, off_dates: &'a OffDates) -> ActionContext<'a> {
    ActionContext { now: at(now), off_dates }
}

fn seeded_mention(db: &Database) {
    prime_slack(db);
    slack(T0).events(vec![mention("1", T0)]).run(db);
    refresh(db, T0);
}

fn triage_issue(db: &Database, object: NormalizedObject, decide: Option<TriageDecision>) -> i64 {
    github(T0).objects(vec![object]).run(db);
    let id = actions(db).triage_batch(at(T1)).unwrap().rows[0].id;
    if let Some(decision) = decide {
        actions(db).triage_decide(&[id], decision, at(T1)).unwrap();
    }
    id
}

#[test]
fn 사용자_입력으로_p0를_만들_수_없다() {
    let db = test_db();
    let added = actions(&db).add("합성 할 일", at(T0)).unwrap();
    assert_eq!(added.status, ActionStatus::Changed);
    let row = item(&db, added.id);
    assert_eq!((row.tier, row.reason), (ItemTier::P2, ItemReason::Manual));
}

#[test]
fn 직접_추가한_할_일은_제안_건수에_들어가지_않는다() {
    let db = test_db();
    actions(&db).add("합성 할 일", at(T0)).unwrap();
    let counts = ItemRepo::new(db.connection()).counts(at(T0)).unwrap();
    assert_eq!((counts.open.p2, counts.manual), (0, 1));
}

#[test]
fn 상태_기계_수락과_되돌리기() {
    let db = test_db();
    seeded_mention(&db);
    let transitions = ItemTransitions::new(db.connection());
    assert_eq!(transitions.accept(1, at(T0)).unwrap().status, ActionStatus::Changed);
    assert_eq!(item(&db, 1).state, ItemState::Accepted);
    assert_eq!(
        transitions.accept(1, at(T0)).unwrap().status,
        ActionStatus::InvalidState,
        "수락한 항목은 다시 수락되지 않는다"
    );
    assert_eq!(transitions.release(1, at(T0)).unwrap().status, ActionStatus::Changed);
    assert_eq!(item(&db, 1).state, ItemState::Open);
    actions(&db).done(&[1], at(T0), false).unwrap();
    assert_eq!(transitions.accept(1, at(T0)).unwrap().status, ActionStatus::AlreadyClosed);
    assert_eq!(transitions.release(1, at(T0)).unwrap().status, ActionStatus::AlreadyClosed);
    assert_eq!(item(&db, 1).state, ItemState::Closed);
}

#[test]
fn 상태_기계_대기와_수락_상태의_항목도_증거로_닫힌다() {
    let db = test_db();
    prime_slack(&db);
    let (a, b) = (format!("slack:{CHANNEL}:1"), format!("slack:{CHANNEL}:2"));
    slack(T0).events(vec![mention("1", T0), mention("2", T0)]).run(&db);
    refresh(&db, T0);
    let none = none();
    actions(&db).later(&[1], &acting(T0, &none)).unwrap();
    ItemTransitions::new(db.connection()).accept(2, at(T0)).unwrap();
    assert_eq!(items(&db).iter().map(|row| row.state).collect::<Vec<_>>(), [ItemState::Parked, ItemState::Accepted]);
    slack(T1).events(vec![my_message("3", T1, &a), my_message("4", T1, &b)]).run(&db);
    refresh(&db, T1);
    assert_eq!(items(&db).iter().map(|row| row.state).collect::<Vec<_>>(), [ItemState::Closed, ItemState::Closed]);
    assert_eq!(item(&db, 1).park_trigger, None);
}

#[test]
fn 나중에는_다음_묶음_시각까지_대기하고_그_시각에_열린다() {
    let db = test_db();
    seeded_mention(&db);
    let none = none();
    let outcome = actions(&db).later(&[1], &acting(T0, &none)).unwrap();
    assert_eq!(outcome[0].status, ActionStatus::Changed);
    let row = item(&db, 1);
    assert_eq!((row.state, row.park_trigger), (ItemState::Parked, Some(ParkTrigger::NextBundle)));
    assert_eq!((row.parked_until, row.push_count), (Some(kst(MON, "13:30")), 1));
    assert_eq!(refresh(&db, &kst(MON, "13:29")).woken, 0);
    assert_eq!(refresh(&db, &kst(MON, "13:30")).woken, 1);
    let woken = item(&db, 1);
    assert_eq!((woken.state, woken.park_trigger, woken.parked_until), (ItemState::Open, None, None));
    assert!(woken.bundle_queued);
}

#[test]
fn 오래된_카드의_조작은_이미_처리됨으로_끝나고_없는_id는_not_found다() {
    let db = test_db();
    let id = actions(&db).add("합성 할 일", at(T0)).unwrap().id;
    actions(&db).done(&[id], at(T0), false).unwrap();
    let none = none();
    let outcome = actions(&db).later(&[id, 999], &acting(T0, &none)).unwrap();
    assert_eq!((outcome[0].status, outcome[0].message.as_str()), (ActionStatus::AlreadyClosed, "이미 처리됨"));
    assert_eq!(outcome[1].status, ActionStatus::NotFound);
    assert_eq!(item(&db, id).close_reason, Some(ItemCloseReason::UserDone));
}

#[test]
fn 버리기는_user_dropped로_닫는다() {
    let db = test_db();
    let id = actions(&db).add("합성 할 일", at(T0)).unwrap().id;
    actions(&db).done(&[id], at(T0), true).unwrap();
    assert_eq!(item(&db, id).close_reason, Some(ItemCloseReason::UserDropped));
}

#[test]
fn 사용자_조작은_events_행으로_이력을_남기고_종료_근거나_처분을_흐리지_않는다() {
    let db = test_db();
    let id = actions(&db).add("합성 할 일", at(T0)).unwrap().id;
    actions(&db).done(&[id], at(T1), false).unwrap();
    let mut statement = db
        .connection()
        .prepare("SELECT disposition, text_head FROM events WHERE type = 'item_changed' ORDER BY id")
        .unwrap();
    let history: Vec<(String, Option<String>)> =
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
    assert_eq!(history, [("noise".to_owned(), None), ("noise".to_owned(), None)], "제목을 이력에 남기지 않는다");
    let invariants = check_invariants(db.connection()).unwrap();
    assert_eq!((invariants.events_without_disposition, invariants.unclassified), (0, 0));
}

#[test]
fn keep은_답으로_끝나지_않는_일을_work_owed로_남기고_원래_항목은_조용히_증거를_기다린다() {
    let db = test_db();
    seeded_mention(&db);
    let none = none();
    let outcome = actions(&db).keep(&[1], &acting(T0, &none), false).unwrap();
    assert_eq!(outcome[0].status, ActionStatus::Changed);
    let rows: Vec<_> = items(&db).iter().map(|row| (row.reason, row.state, row.park_trigger)).collect();
    assert_eq!(
        rows,
        [
            (ItemReason::ReplyOwed, ItemState::Parked, Some(ParkTrigger::Kept)),
            (ItemReason::WorkOwed, ItemState::Open, None)
        ]
    );
    assert_eq!(item(&db, 2).tier, ItemTier::P2);
    assert!(item(&db, 2).first_surfaced_at.is_some(), "남긴 일은 다시 제안하지 않는다");
    // 짧은 답장으로 reply_owed가 닫혀도 work_owed는 남는다.
    slack(T1).events(vec![my_message("2", T1, &format!("slack:{CHANNEL}:1"))]).run(&db);
    refresh(&db, T1);
    assert_eq!(items(&db).iter().map(|row| row.state).collect::<Vec<_>>(), [ItemState::Closed, ItemState::Open]);
}

#[test]
fn keep_until_deploy_배포_연결_항목은_d_1까지_침묵하고_트리거_때_p1로_연다() {
    let db = test_db();
    prime(&db, SourceName::Github, "tick");
    prime(&db, SourceName::Calendar, "team");
    let linked = NormalizedObject { milestone_title: Some("합성 기능".into()), ..issue(7) };
    let event =
        NormalizedObject { start_date: Some("2026-03-10".into()), end_date: Some("2026-03-11".into()), ..deploy("d2") };
    github(T0).objects(vec![linked.clone()]).run(&db);
    calendar("team", T0).objects(vec![event]).run(&db);
    let owned = NormalizedEvent { object_key: Some(linked.object_key.clone()), ..comment("1", T1) };
    github(T1).events(vec![owned]).run(&db);
    refresh(&db, T1);
    let id = items(&db).iter().find(|row| row.object_key == linked.object_key).unwrap().id;
    let none = none();
    let outcome = actions(&db).keep(&[id], &acting(T1, &none), true).unwrap();
    assert_eq!(outcome[0].status, ActionStatus::Changed);
    assert_eq!(item(&db, id).park_trigger, Some(ParkTrigger::Deploy));
    assert_eq!(item(&db, id).parked_until, Some(kst("2026-03-09", "10:00")));
    refresh(&db, &kst("2026-03-09", "09:59"));
    assert_eq!(item(&db, id).state, ItemState::Parked);
    refresh(&db, &kst("2026-03-09", "10:00"));
    assert_eq!((item(&db, id).state, item(&db, id).tier), (ItemState::Open, ItemTier::P1));
}

#[test]
fn keep_until_deploy_연결된_배포가_없으면_바꾸지_않는다() {
    let db = test_db();
    seeded_mention(&db);
    let none = none();
    let outcome = actions(&db).keep(&[1], &acting(T0, &none), true).unwrap();
    assert_eq!(outcome[0].status, ActionStatus::NoDeploy);
    assert_eq!(item(&db, 1).state, ItemState::Open);
}

#[test]
fn triage_기준선_객체를_10건씩_내고_유지와_버리기를_고른다() {
    let db = test_db();
    let mut objects: Vec<_> = (1..=12).map(issue).collect();
    objects.push(pull_request(50));
    objects.push(NormalizedObject { state: Some("closed".into()), ..issue(60) });
    objects.push(NormalizedObject { assigned_to_me: Some(false), ..issue(61) });
    github(T0).objects(objects).run(&db);
    let first = actions(&db).triage_batch(at(T1)).unwrap();
    assert_eq!(first.rows.len(), 10);
    assert_eq!(first.remaining, 3, "남은 건은 이슈 2건과 내 PR 1건이다");
    assert_eq!(actions(&db).triage_batch(at(T1)).unwrap().rows.len(), 10, "결정 전에는 같은 10건이다");
    assert_eq!(count(&db, "items"), 10);
    assert!(first.rows.iter().all(|row| {
        row.baseline && row.state == ItemState::Parked && row.park_trigger == Some(ParkTrigger::Triage)
    }));

    let (keep, drop) = (first.rows[0].id, first.rows[1].id);
    assert_eq!(
        actions(&db).triage_decide(&[keep], TriageDecision::Keep, at(T1)).unwrap()[0].status,
        ActionStatus::Changed
    );
    assert_eq!(
        actions(&db).triage_decide(&[drop], TriageDecision::Drop, at(T1)).unwrap()[0].status,
        ActionStatus::Changed
    );
    assert_eq!((item(&db, keep).state, item(&db, keep).baseline), (ItemState::Open, true));
    assert_eq!(
        (item(&db, drop).state, item(&db, drop).close_reason),
        (ItemState::Closed, Some(ItemCloseReason::UserDropped))
    );

    let second = actions(&db).triage_batch(at(T1)).unwrap();
    assert_eq!(second.rows.len(), 10);
    assert_eq!(second.remaining, 1);
    let dropped_key = item(&db, drop).object_key;
    assert!(!second.rows.iter().any(|row| row.object_key == dropped_key), "버린 건은 돌아오지 않는다");
}

#[test]
fn triage_판단은_기준선_대기_항목에만_된다() {
    let db = test_db();
    let id = actions(&db).add("합성 할 일", at(T0)).unwrap().id;
    assert_eq!(
        actions(&db).triage_decide(&[id], TriageDecision::Keep, at(T0)).unwrap()[0].status,
        ActionStatus::InvalidState
    );
    assert_eq!(
        actions(&db).triage_decide(&[999], TriageDecision::Keep, at(T0)).unwrap()[0].status,
        ActionStatus::NotFound
    );
}

#[test]
fn 기준선_항목은_사람의_새_사건이_생기면_올라온다() {
    let db = test_db();
    let id = triage_issue(&db, issue(1), Some(TriageDecision::Keep));
    assert_eq!(ItemRepo::new(db.connection()).counts(at(T1)).unwrap().open.p2, 0, "기준선은 건수에 넣지 않는다");
    github(T2).events(vec![comment("1", T2)]).run(&db);
    refresh(&db, T2);
    assert!(!item(&db, id).baseline);
    assert!(item(&db, id).bundle_queued);
}

#[test]
fn 기준선_항목은_배포_연결이_생기면_올라온다() {
    let db = test_db();
    let object = NormalizedObject { milestone_title: Some("합성 기능".into()), ..issue(7) };
    let id = triage_issue(&db, object, None);
    prime_at(&db, SourceName::Calendar, "team", T1);
    let event =
        NormalizedObject { start_date: Some("2026-03-03".into()), end_date: Some("2026-03-04".into()), ..deploy("d3") };
    calendar("team", T2).objects(vec![event]).run(&db);
    refresh(&db, T2);
    assert_eq!((item(&db, id).baseline, item(&db, id).state), (false, ItemState::Open));
}

#[test]
fn 기준선_항목도_양성_증거로_닫히되_그_소스가_이번_틱에_ok일_때만이다() {
    let db = test_db();
    let object = issue(1);
    let key = object.object_key.clone();
    let id = triage_issue(&db, object, Some(TriageDecision::Keep));
    let closed = NormalizedEvent {
        object_key: Some(key),
        ..NormalizedEvent::new("gh:closed:1", SourceName::Github, EventType::GhClosed, T2, ActorKind::Human)
    };
    github(T2).events(vec![closed]).partial().run(&db);
    assert_eq!(refresh_with(&db, T2, &none(), &[]).closed, 0);
    assert_eq!(refresh_with(&db, T3, &none(), &["github"]).closed, 1);
    assert_eq!(item(&db, id).close_reason, Some(ItemCloseReason::DoneEvidence));
    assert_eq!(item(&db, id).close_evidence_event.as_deref(), Some("gh:closed:1"));
}

#[test]
fn 세션이_열린_항목을_끝내면_세션은_열려_있다고_알린다() {
    // 원본에 없는 시험이다. doneItems의 메시지 분기를 고정한다.
    let db = test_db();
    let id = actions(&db).add("합성 할 일", at(T0)).unwrap().id;
    db.connection()
        .execute(
            "INSERT INTO runs (id, item_id, state, playbook, created_at) VALUES ('r1', ?, 'started', 'qa_issue', ?)",
            kairos_store::params![id, T0],
        )
        .unwrap();
    assert_eq!(actions(&db).done(&[id], at(T1), false).unwrap()[0].message, "끝냈습니다. 세션은 열려 있습니다");
}
