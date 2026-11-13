//! 응답을 잃거나 단계가 실패했을 때의 복구.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use kairos_agenda::{ItemState, ReadyKind};
use kairos_sessions::{CLAIM_STALE_MS, RunState, SESSION_LOST_MS, SessionStatus};
use serde_json::json;

fn empty_terminals() -> Canned {
    ok(json!({ "terminals": [] }))
}

#[test]
fn terminal_create_응답을_잃으면_다시_부르지_않고_제목으로_찾는다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Lost);
    orca.set(
        "terminal list",
        ok(json!({ "terminals": [], "visualLayouts": [layout("kairos-99999999"), layout("kairos-11111111")] })),
    );
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(orca.count("terminal create"), 1);
    assert_eq!(
        orca.args("terminal list"),
        ["terminal", "list", "--worktree", SELECTOR, "--include-visual-layouts", "--json"]
    );
    let row = run(&db);
    assert_eq!((row.pane_key.as_deref(), row.terminal_handle.as_deref()), (Some(PANE), Some("term_found")));
}

#[test]
fn 프로그램이_바꾼_terminals_title은_대조에_쓰지_않는다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Lost);
    orca.set(
        "terminal list",
        ok(json!({ "terminals": [pane_with("title", json!("kairos-11111111"))], "visualLayouts": [] })),
    );
    assert_eq!(accept(&db, &orca).run.unwrap().state, "needs_attention");
}

#[test]
fn 응답을_잃었지만_ps에_에이전트가_있으면_그_pane을_쓴다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Lost);
    orca.set("terminal list", empty_terminals());
    orca.set("worktree ps", ps_with(&["working"]));
    accept(&db, &orca);
    assert_eq!(orca.count("terminal create"), 1);
    let row = run(&db);
    assert_eq!((row.pane_key.as_deref(), row.terminal_handle), (Some(PANE), None));
}

#[test]
fn 응답을_잃고_아무것도_못_찾으면_재실행_없이_확인_필요로_두고_늦게_뜨면_started가_된다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Lost);
    orca.set("terminal list", empty_terminals());
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(value.run.unwrap().state, "needs_attention");
    assert_eq!(item(&db, 1).state, ItemState::Accepted);
    assert_eq!(orca.count("terminal create"), 1);

    orca.set("terminal list", ok(json!({ "terminals": [], "visualLayouts": [layout("kairos-11111111")] })));
    orca.set("worktree ps", ps_with(&["working"]));
    sync(&db, &orca, 10_000);
    assert_eq!(run(&db).state, RunState::Started);
    assert_eq!(run(&db).ready_at, None, "늦게 떴으면 확인 카드를 거둔다");
    assert_eq!(orca.count("terminal create"), 1);
}

#[test]
fn 응답을_잃은_채_kairos_run도_시작하지_않으면_실패로_내리고_항목을_연다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Lost);
    orca.set("terminal list", empty_terminals());
    accept(&db, &orca);
    sync(&db, &orca, SESSION_LOST_MS - 1);
    assert_eq!(run(&db).state, RunState::NeedsAttention);
    sync(&db, &orca, SESSION_LOST_MS);
    assert_eq!((run(&db).state, item(&db, 1).state), (RunState::Failed, ItemState::Open));
    assert_eq!(orca.count("terminal create"), 1);
    assert_eq!(run(&db).fail_code.as_deref(), Some("session_lost"));
    assert!(item(&db, 1).bundle_queued, "다시 제안되도록 다음 묶음에 싣는다");
}

#[test]
fn orca가_terminal_create를_거절하고_찾지도_못하면_실패다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Refused("runtime_unavailable"));
    orca.set("terminal list", empty_terminals());
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::TerminalFailed);
    assert_eq!((run(&db).state, item(&db, 1).state), (RunState::Failed, ItemState::Open));
    assert_eq!(orca.count("terminal create"), 1);
}

#[test]
fn 단계별_실패는_run을_실패로_내리고_항목을_되돌리며_다시_수락할_수_있다() {
    let cases = [
        ("reference find", SessionStatus::OrcaUnavailable, Canned::Lost),
        ("reference find", SessionStatus::OrcaUnavailable, Canned::Missing),
        ("worktree create", SessionStatus::WorktreeFailed, Canned::Refused("selector_not_found")),
        ("worktree show", SessionStatus::CheckoutFailed, Canned::Refused("selector_not_found")),
        (
            "worktree show",
            SessionStatus::CheckoutFailed,
            ok(json!({ "worktree": { "id": WORKTREE_ID, "path": "/synthetic" } })),
        ),
    ];
    for (command, status, reply) in cases {
        let db = test_db();
        seed_issue(&db);
        let orca = FakeOrca::with(command, reply);
        let value = accept(&db, &orca);
        assert_eq!(
            (value.result.status, value.result.state, value.run),
            (status, Some(ItemState::Open), None),
            "{command}"
        );
        assert_eq!(run(&db).state, RunState::Failed);
        assert_eq!(orca.count("terminal create"), 0);
        let retry = FakeOrca::new();
        let second = accept_as(&db, &retry, 1, OTHER_RUN);
        assert_eq!(second.result.status, SessionStatus::Changed);
        assert_eq!(count(&db, "runs"), 2);
    }
}

#[test]
fn worktree_create_응답을_잃으면_이름으로_찾고_없으면_실패다() {
    let (db, orca) = seeded();
    let finds = Arc::new(AtomicUsize::new(0));
    orca.set("worktree create", Canned::Lost);
    orca.script("reference find", move |_| {
        if finds.fetch_add(1, Ordering::SeqCst) == 0 {
            ok(json!({ "matches": [] }))
        } else {
            ok(json!({ "matches": [{ "workspace": { "id": WORKTREE_ID, "name": "t-11111111" }, "agents": [] }] }))
        }
    });
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(orca.count("worktree create"), 1);
    assert_eq!(run(&db).worktree_identity.as_deref(), Some("identity:wt-synth-key"), "show의 응답으로 ID를 채운다");

    let other = test_db();
    seed_issue(&other);
    let lost = FakeOrca::with("worktree create", Canned::Lost);
    assert_eq!(accept(&other, &lost).result.status, SessionStatus::WorktreeFailed);
    assert_eq!(lost.count("worktree create"), 1);
}

#[test]
fn 추적_선점만_남은_run은_시간이_지나면_실패로_내린다() {
    let (db, orca) = seeded();
    orca.script("reference find", |_| panic!("프로세스가 여기서 죽은 경우"));
    let died = catch_unwind(AssertUnwindSafe(|| accept(&db, &orca)));
    assert!(died.is_err());
    assert_eq!(run(&db).state, RunState::Claimed);
    orca.clear();
    sync(&db, &orca, CLAIM_STALE_MS - 1);
    assert_eq!(run(&db).state, RunState::Claimed);
    sync(&db, &orca, CLAIM_STALE_MS);
    assert_eq!((run(&db).state, item(&db, 1).state), (RunState::Failed, ItemState::Open));
    assert!(orca.calls().is_empty(), "띄운 적 없는 run에는 ps도 부르지 않는다");
    assert_eq!(run(&db).fail_code.as_deref(), Some("claim_stale"));
}

// 이하 원본에 없는 시험.

#[test]
fn 응답을_잃은_확인_필요는_준비됨_attention을_남긴다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Lost);
    orca.set("terminal list", empty_terminals());
    accept(&db, &orca);
    let row = run(&db);
    assert_eq!((row.ready_kind, row.ready_at.as_deref()), (Some(ReadyKind::Attention), Some(AT)));
}

#[test]
fn terminal_create가_missing이어도_찾으면_성공이고_못_찾으면_실패다() {
    let (db, orca) = seeded();
    orca.set("terminal create", Canned::Missing);
    orca.set("terminal list", ok(json!({ "visualLayouts": [layout("kairos-11111111")] })));
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::Changed);

    let other = test_db();
    seed_issue(&other);
    let gone = FakeOrca::with("terminal create", Canned::Missing);
    gone.set("terminal list", Canned::Missing);
    gone.set("worktree ps", Canned::Missing);
    assert_eq!(accept(&other, &gone).result.status, SessionStatus::TerminalFailed);
}

#[test]
fn worktree_create가_missing이면_orca_없음이고_되찾기를_하지_않는다() {
    let (db, _) = seeded();
    let orca = FakeOrca::with("worktree create", Canned::Missing);
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::OrcaUnavailable);
    assert_eq!(orca.count("reference find"), 1);
}

#[test]
fn 설정한_orca_저장소_id가_있으면_repo_list를_부르지_않는다() {
    let (db, orca) = seeded();
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::Changed);
    assert_eq!(orca.count("repo list"), 0);
    let args = orca.args("worktree create");
    assert!(args.contains(&"id:0000c0de-0000-4000-8000-000000000001".to_owned()), "{args:?}");
}
