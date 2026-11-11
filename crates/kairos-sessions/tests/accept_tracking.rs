//! 추적과 카드, 이동.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_agenda::{
    CardAction, DisplayMode, ItemActions, ItemState, ItemView, KairosControl, NextOptions, Presenter, ReadOnlyRunRepo,
    ReadyKind, RunView,
};
use kairos_core::Instant;
use kairos_sessions::{RunState, SESSION_VISIBLE_MS, SessionStatus, SyncedRun};
use kairos_store::{Database, params};
use serde_json::json;

fn no_agents() -> Canned {
    ok(json!({ "worktrees": [], "truncated": false }))
}

#[test]
fn 추적_ps_상태가_run_상태가_된다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    orca.set("worktree ps", ps_with(&["working"]));
    let report = sync(&db, &orca, 5000);
    let synced = SyncedRun { id: RUN.into(), item_id: 1, state: RunState::Started, ready: None };
    assert_eq!(report.runs, [synced]);
    orca.set("worktree ps", ps_with(&["blocked"]));
    sync(&db, &orca, 6000);
    assert_eq!((run(&db).state, run(&db).ready_kind), (RunState::WaitingUser, Some(ReadyKind::Question)));
    orca.set("worktree ps", ps_with(&["working"]));
    sync(&db, &orca, 7000);
    assert_eq!(run(&db).state, RunState::Started);
    orca.set("worktree ps", ps_with(&["waiting"]));
    sync(&db, &orca, 8000);
    assert_eq!(run(&db).state, RunState::WaitingUser);
    assert_eq!(run(&db).ready_at, Some(later_iso(6000)), "준비됨은 run마다 1회다");
}

#[test]
fn 추적_턴이_끝난_세션은_대기이고_준비됨을_남긴다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    orca.set("worktree ps", ps_with(&["done"]));
    sync(&db, &orca, 5000);
    let row = run(&db);
    assert_eq!((row.state, row.ready_kind), (RunState::WaitingUser, Some(ReadyKind::Done)));
    assert_eq!(row.seen_at, Some(later_iso(5000)));
    assert_eq!(item(&db, 1).state, ItemState::Accepted, "세션 상태는 항목을 닫지 않는다");
}

#[test]
fn 추적_30초_안에_안_보이면_확인_필요다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    sync(&db, &orca, SESSION_VISIBLE_MS - 1);
    assert_eq!(run(&db).state, RunState::Claimed);
    sync(&db, &orca, SESSION_VISIBLE_MS);
    assert_eq!((run(&db).state, run(&db).ready_kind), (RunState::NeedsAttention, Some(ReadyKind::Attention)));
    orca.set("worktree ps", ps_with(&["done"]));
    sync(&db, &orca, SESSION_VISIBLE_MS + 1000);
    let row = run(&db);
    assert_eq!(
        (row.state, row.ready_kind),
        (RunState::WaitingUser, Some(ReadyKind::Done)),
        "확인 카드는 진짜 준비됨을 막지 않는다"
    );
}

#[test]
fn 추적_ps를_읽지_못하거나_잘린_응답이면_상태를_바꾸지_않는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    orca.set("worktree ps", Canned::Lost);
    let report = sync(&db, &orca, SESSION_VISIBLE_MS * 2);
    assert!(!report.reachable);
    assert_eq!(run(&db).state, RunState::Claimed);
    orca.set("worktree ps", ok(json!({ "worktrees": [], "truncated": true })));
    sync(&db, &orca, SESSION_VISIBLE_MS * 3);
    assert_eq!(run(&db).state, RunState::Claimed);
}

#[test]
fn 추적_본_세션이_사라지고_터미널도_없으면_끝이다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    orca.set("worktree ps", ps_with(&["working"]));
    sync(&db, &orca, 1000);
    orca.set("worktree ps", no_agents());
    sync(&db, &orca, 2000);
    assert_eq!(run(&db).state, RunState::Started, "터미널이 살아 있으면 판단을 미룬다");
    orca.set("terminal list", ok(json!({ "terminals": [pane_with("connected", json!(false))] })));
    sync(&db, &orca, 3000);
    let row = run(&db);
    assert_eq!((row.state, row.ready_kind), (RunState::Ended, Some(ReadyKind::Ended)));
    assert_eq!(row.ended_at, Some(later_iso(3000)));
    assert_eq!(item(&db, 1).state, ItemState::Accepted, "세션 종료는 항목 종료가 아니다");
}

#[test]
fn 추적_worktree가_지워져도_끝으로_본다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    orca.set("worktree ps", ps_with(&["working"]));
    sync(&db, &orca, 1000);
    orca.set("worktree ps", no_agents());
    orca.set("terminal list", Canned::Refused("selector_not_found"));
    sync(&db, &orca, 2000);
    assert_eq!(run(&db).state, RunState::Ended);
}

#[test]
fn 추적_활성_run이_없으면_orca를_부르지_않는다() {
    let (db, orca) = seeded();
    let report = sync(&db, &orca, 0);
    assert!(report.reachable);
    assert!(report.runs.is_empty());
    assert!(orca.calls().is_empty());
}

fn next(db: &Database, now: Instant, control: &KairosControl, seen: bool) -> kairos_agenda::NextResponse {
    Presenter::new(db).next_response(&context(now, control), NextOptions { seen, ack: false }).unwrap()
}

#[test]
fn 준비됨_카드_이동_버튼만_있고_그리면_다시_나오지_않으며_예산을_쓰지_않는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    orca.set("worktree ps", ps_with(&["done"]));
    sync(&db, &orca, 1000);
    let now = later(2000);
    let on = control_on();
    let peeked = next(&db, now, &on, false);
    assert_eq!(peeked.ready.len(), 1);
    let card = &peeked.ready[0];
    assert_eq!(
        (card.run_id.as_str(), card.item_id, card.kind, card.headline.as_deref()),
        (RUN, 1, Some(ReadyKind::Done), Some("준비됨"))
    );
    let action =
        CardAction {
            id: "move".into(), label: "이동".into(), argv: vec!["move".into(), "1".into(), "--json".into()]
        };
    assert_eq!(card.actions, [action]);
    assert_eq!(next(&db, now, &on, false).ready.len(), 1, "next만으로는 쓰지 않는다");
    let seen = next(&db, now, &on, true);
    assert_eq!(seen.ready.len(), 1);
    assert_eq!(next(&db, now, &on, false).ready.len(), 0);
    let budget = &seen.budget;
    assert_eq!((budget.p1_hour_used, budget.p1_day_used, budget.bundles_today, budget.shown_today), (0, 0, 0, 0));
}

#[test]
fn 준비됨_카드_게이트가_닫히면_안_나오고_세션이_살아_있으면_항목이_닫혀도_나온다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    orca.set("worktree ps", ps_with(&["blocked"]));
    sync(&db, &orca, 1000);
    let on = control_on();
    let off = KairosControl::default().with_display_mode(DisplayMode::Off);
    assert_eq!(next(&db, later(2000), &off, true).ready.len(), 0);
    assert_eq!(next(&db, later(2000), &on, false).ready.len(), 1, "게이트가 닫혀 있는 동안 소비되지 않는다");
    ItemActions::new(&db).done(&[1], later(3000), false).unwrap();
    assert_eq!(next(&db, later(4000), &on, false).ready.len(), 1, "세션으로 갈 길이 사라지면 안 된다");
    orca.set("worktree ps", no_agents());
    orca.set("terminal list", ok(json!({ "terminals": [] })));
    sync(&db, &orca, 5000);
    assert_eq!(next(&db, later(6000), &on, false).ready.len(), 0, "세션이 끝나면 닫힌 항목의 카드는 없다");
}

#[test]
fn 되돌린_뒤_다시_시작해도_앞_run의_카드는_살아나지_않는다() {
    for (kind, state) in [("attention", "needs_attention"), ("question", "waiting_user"), ("ended", "ended")] {
        let (db, orca) = seeded();
        accept(&db, &orca);
        let ended_at = (state == "ended").then(|| later_iso(500));
        db.connection()
            .execute(
                "UPDATE runs SET state = ?, ended_at = ?, ready_kind = ?, ready_at = ?",
                params![state, ended_at, kind, later_iso(500)],
            )
            .unwrap();
        let on = control_on();
        assert_eq!(next(&db, later(600), &on, false).ready.len(), 1, "{kind}");
        assert_eq!(cancel(&db, 1, 700).result.status, SessionStatus::Changed);
        let retry = FakeOrca::new();
        assert_eq!(accept_as(&db, &retry, 1, OTHER_RUN).result.status, SessionStatus::Changed);
        assert_eq!(item(&db, 1).state, ItemState::Accepted);
        assert_eq!(next(&db, later(800), &on, false).ready.len(), 0, "{kind}");
        let runs = ReadOnlyRunRepo::new(db.connection());
        assert_eq!(runs.unseen_ready_count(true).unwrap(), 0, "건수도 같은 조건이다");

        db.connection()
            .execute(
                "UPDATE runs SET ready_kind = 'done', ready_at = ? WHERE id = ?",
                params![later_iso(900), OTHER_RUN],
            )
            .unwrap();
        let cards = next(&db, later(1000), &on, false).ready;
        let found: Vec<_> = cards.iter().map(|card| (card.run_id.as_str(), card.kind)).collect();
        assert_eq!(found, [(OTHER_RUN, Some(ReadyKind::Done))], "새 run의 카드는 나온다");
    }
}

fn view(db: &Database) -> ItemView {
    let listed = Presenter::new(db).list_response(&context(later(1000), &control_on()), false).unwrap();
    listed.items.into_iter().find(|row| row.id == 1).unwrap()
}

#[test]
fn item_view_run_활성_세션과_수락_상태의_끝난_세션을_보여_주고_실패는_안_보여_준다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    assert_eq!(view(&db).run, Some(RunView { id: RUN.into(), state: "claimed".into() }));
    let ids: Vec<_> = view(&db).actions.into_iter().map(|action| action.id).collect();
    assert_eq!(ids, ["done"], "수락 항목의 버튼은 끝뿐이다");
    db.connection().execute("UPDATE runs SET state = 'ended', ended_at = ?", params![later_iso(500)]).unwrap();
    assert_eq!(view(&db).run, Some(RunView { id: RUN.into(), state: "ended".into() }));
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::Changed);
    assert_eq!(cancel(&db, 1, 600).result.status, SessionStatus::Changed);
    assert_eq!(view(&db).run, None);
}

#[test]
fn 이동_handle은_pane_key로_다시_찾는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    let mut other = pane_with("tabId", json!("tab-x"));
    other["leafId"] = json!("leaf-x");
    other["handle"] = json!("term_other");
    orca.set("terminal list", ok(json!({ "terminals": [other, pane_with("handle", json!("term_fresh"))] })));
    let value = move_to(&db, &orca, 1);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(orca.args("terminal list"), ["terminal", "list", "--worktree", SELECTOR, "--json"]);
    assert_eq!(orca.args("terminal switch"), ["terminal", "switch", "--terminal", "term_fresh", "--json"]);
}

#[test]
fn 이동_실패_경로() {
    let (db, orca) = seeded();
    assert_eq!(move_to(&db, &orca, 9).result.status, SessionStatus::NotFound);
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::NoSession);
    accept(&db, &orca);
    orca.set("terminal list", ok(json!({ "terminals": [] })));
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::SessionGone);
    orca.set("terminal list", ok(json!({ "terminals": [pane_with("handle", json!("--evil handle"))] })));
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::SessionGone, "형식이 다른 handle은 넘기지 않는다");
    assert_eq!(orca.count("terminal switch"), 0);
    orca.set("terminal list", ok(json!({ "terminals": [pane_row()] })));
    orca.set("terminal switch", ok(json!({ "focus": { "handle": "term_now", "navigated": false } })));
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::MoveFailed);
    orca.set("terminal list", Canned::Lost);
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::OrcaUnavailable);
}
