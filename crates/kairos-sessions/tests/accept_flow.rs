//! 수락의 순서, 재수락, 동시 수락, 기존 세션, 취소.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::thread;

use common::*;
use kairos_agenda::{ItemActions, ItemState, Playbook, RunView};
use kairos_core::KairosHome;
use kairos_core::ports::FixedClock;
use kairos_sessions::{
    ORCA_READ, ORCA_TERMINAL, ORCA_WORKTREE, RunState, SessionLauncher, SessionRefusal, SessionStatus,
};
use kairos_store::Database;
use serde_json::json;

/// 시험 프로필(`Profile::fixture`)의 플레이북 대상.
const PLAYBOOK_ORCA_REPO: &str = "0000c0de-0000-4000-8000-000000000001";
const PLAYBOOK_BASE_BRANCH: &str = "origin/fe-stage";

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

const LAUNCH: &str = "'/synthetic/bin/kairos-run' 11111111-2222-4333-8444-555555555555";

#[test]
fn 수락_순서와_플래그는_설계_7_2_그대로다() {
    let (db, orca) = seeded();
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(value.run, Some(RunView { id: RUN.into(), state: "claimed".into() }));
    let url = issue_url(1);
    let repo = format!("id:{PLAYBOOK_ORCA_REPO}");
    assert_eq!(
        orca.calls(),
        vec![
            strings(&["reference", "find", &url, "--json"]),
            strings(&[
                "worktree",
                "create",
                "--repo",
                &repo,
                "--name",
                "t-11111111",
                "--no-parent",
                "--setup",
                "skip",
                "--base-branch",
                PLAYBOOK_BASE_BRANCH,
                "--reference",
                &url,
                "--json"
            ]),
            strings(&["worktree", "show", "--worktree", SELECTOR, "--json"]),
            strings(&[
                "terminal",
                "create",
                "--worktree",
                SELECTOR,
                "--title",
                "kairos-11111111",
                "--command",
                LAUNCH,
                "--focus",
                "--json"
            ]),
            strings(&["worktree", "ps", "--json"]),
        ]
    );
    assert_eq!(item(&db, 1).state, ItemState::Accepted);
    let row = run(&db);
    assert_eq!(row.state, RunState::Claimed);
    assert_eq!(row.worktree_identity.as_deref(), Some(SELECTOR));
    assert_eq!(row.worktree_id.as_deref(), Some(WORKTREE_ID));
    assert_eq!(row.pane_key.as_deref(), Some(PANE));
    assert_eq!(row.terminal_handle.as_deref(), Some("term_made"));
    assert_eq!(row.launched_at.as_deref(), Some(AT));
    assert_eq!(row.playbook, Playbook::QaIssue);
}

#[test]
fn 금지한_플래그와_명령은_어떤_호출에도_없다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    sync(&db, &orca, 1000);
    move_to(&db, &orca, 1);
    let flat = orca.flat();
    for banned in ["--pr", "--agent", "--prompt", "--activate", "send", "launch", "agent"] {
        assert!(!flat.iter().any(|argument| argument == banned), "{banned}");
    }
}

#[test]
fn 세션_창은_수락의_터미널_생성에서만_앞으로_온다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    sync(&db, &orca, 1000);
    move_to(&db, &orca, 1);
    let focused: Vec<_> = orca.calls().into_iter().filter(|call| call.iter().any(|arg| arg == "--focus")).collect();
    assert_eq!(focused.len(), 1);
    assert_eq!(focused[0][..2], ["terminal".to_owned(), "create".to_owned()]);
}

#[test]
fn 이동은_수락과_추적에서_부르지_않고_move에서만_부른다() {
    let (db, orca) = seeded();
    orca.set("worktree ps", ps_with(&["done"]));
    accept(&db, &orca);
    sync(&db, &orca, 1000);
    assert_eq!(orca.count("terminal switch"), 0);
    move_to(&db, &orca, 1);
    assert_eq!(orca.count("terminal switch"), 1);
}

#[test]
fn 다시_눌러도_세션은_하나이고_기존_run을_돌려준다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    let calls = orca.calls().len();
    let again = accept_as(&db, &orca, 1, OTHER_RUN);
    assert_eq!(again.result.status, SessionStatus::Unchanged);
    assert_eq!(again.run, Some(RunView { id: RUN.into(), state: "claimed".into() }));
    assert_eq!(orca.calls().len(), calls, "진 쪽은 Orca를 부르지 않는다");
    assert_eq!(count(&db, "runs"), 1);
}

/// 원본의 Promise.all은 한 스레드에서 번갈아 돌았다. 여기서는 실제 스레드와 연결을 쓴다.
fn race(connections: usize) -> (Vec<kairos_sessions::SessionOutcome>, std::sync::Arc<FakeOrca>, tempfile::TempDir) {
    let (dir, home) = temp_home();
    let databases: Vec<Database> = (0..connections).map(|_| Database::open(&home).unwrap()).collect();
    seed_issue(&databases[0]);
    let orca = FakeOrca::new();
    let outcomes = thread::scope(|scope| {
        let workers: Vec<_> = databases
            .into_iter()
            .enumerate()
            .map(|(index, db)| {
                let orca = &orca;
                scope.spawn(move || accept_as(&db, orca, 1, &format!("{index}1111111-2222-4333-8444-555555555555")))
            })
            .collect();
        workers.into_iter().map(|worker| worker.join().unwrap()).collect()
    });
    (outcomes, orca, dir)
}

fn sorted_statuses(outcomes: &[kairos_sessions::SessionOutcome]) -> Vec<&'static str> {
    let mut statuses: Vec<_> = outcomes.iter().map(|outcome| outcome.result.status.as_str()).collect();
    statuses.sort_unstable();
    statuses
}

#[test]
fn 두_연결이_동시에_수락해도_run과_worktree는_하나다() {
    let (outcomes, orca, dir) = race(2);
    assert_eq!(sorted_statuses(&outcomes), ["changed", "unchanged"]);
    assert_eq!(outcomes[0].run.as_ref().map(|run| &run.id), outcomes[1].run.as_ref().map(|run| &run.id));
    assert_eq!(count(&Database::open(&KairosHome::at(dir.path())).unwrap(), "runs"), 1);
    assert_eq!(orca.count("worktree create"), 1);
    assert_eq!(orca.count("terminal create"), 1);
}

/// 원본: CLI: 여러 프로세스가 동시에 수락해도 worktree와 run은 하나다. 프로세스 대신 연결 다섯으로 같은 경합을 만든다.
#[test]
fn 여러_연결이_동시에_수락해도_worktree와_run은_하나다() {
    let (outcomes, orca, dir) = race(5);
    assert_eq!(sorted_statuses(&outcomes), ["changed", "unchanged", "unchanged", "unchanged", "unchanged"]);
    let ids: std::collections::BTreeSet<_> = outcomes.iter().map(|outcome| outcome.run.clone().unwrap().id).collect();
    assert_eq!(ids.len(), 1, "진 쪽도 같은 run을 받는다");
    assert_eq!(orca.count("worktree create"), 1);
    assert_eq!(orca.count("terminal create"), 1);
    assert_eq!(count(&Database::open(&KairosHome::at(dir.path())).unwrap(), "runs"), 1);
}

#[test]
fn reference에_살아_있는_에이전트가_있으면_새로_열지_않고_이동을_준다() {
    let (db, orca) = seeded();
    orca.set(
        "reference find",
        ok(json!({
            "matches": [{
                "workspace": { "id": WORKTREE_ID, "name": "synthetic" },
                "agents": [
                    { "agent": "claude", "liveness": "exited", "paneKey": "tab-old:leaf-old", "linked": true },
                    { "agent": "claude", "liveness": "live", "paneKey": PANE, "terminal": "term_live", "linked": true },
                ],
            }],
        })),
    );
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(value.run.unwrap().state, "started");
    assert_eq!(orca.count("worktree create"), 0);
    assert_eq!(orca.count("terminal create"), 0);
    let row = run(&db);
    assert!(row.adopted);
    assert_eq!(row.pane_key.as_deref(), Some(PANE));
    assert_eq!(row.worktree_identity, Some(format!("id:{WORKTREE_ID}")));
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::Changed);
    assert_eq!(orca.args("terminal switch"), strings(&["terminal", "switch", "--terminal", "term_now", "--json"]));
}

#[test]
fn 죽은_에이전트만_있는_reference는_새로_연다() {
    let (db, orca) = seeded();
    orca.set(
        "reference find",
        ok(json!({ "matches": [{ "workspace": { "id": WORKTREE_ID, "name": "x" }, "agents": [{ "liveness": "exited", "paneKey": PANE }] }] })),
    );
    accept(&db, &orca);
    assert_eq!(orca.count("worktree create"), 1);
}

#[test]
fn 닫힌_항목_없는_항목_대기_항목() {
    let (db, orca) = seeded();
    assert_eq!(accept_as(&db, &orca, 9, RUN).result.status, SessionStatus::NotFound);
    db.connection().execute("UPDATE items SET state = 'parked', park_trigger = 'kept' WHERE id = 1", []).unwrap();
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::InvalidState);
    db.connection().execute("UPDATE items SET state = 'open', park_trigger = NULL WHERE id = 1", []).unwrap();
    ItemActions::new(&db).done(&[1], at(AT), false).unwrap();
    let closed = accept(&db, &orca);
    assert_eq!(closed.result.status, SessionStatus::AlreadyClosed);
    assert_eq!(closed.result.message, "이미 처리됨");
    assert!(orca.calls().is_empty());
}

#[test]
fn 일하는_세션이_4개_이상이면_경고_한_줄만_내고_막지_않는다() {
    let (db, orca) = seeded();
    orca.set("worktree ps", ps_with(&["working", "working", "working", "working", "done"]));
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(value.warning.as_deref(), Some("일하는 세션이 4개입니다"));
    let quiet = test_db();
    seed_issue(&quiet);
    let few = FakeOrca::with("worktree ps", ps_with(&["working", "working", "working"]));
    assert_eq!(accept(&quiet, &few).warning, None);
}

#[test]
fn 취소_항목을_되돌리고_늦게_뜬_run은_살아나지_않는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    assert_eq!(cancel(&db, 1, 1).result.status, SessionStatus::Changed);
    assert_eq!((run(&db).state, item(&db, 1).state), (RunState::Failed, ItemState::Open));
    assert_eq!(cancel(&db, 1, 2).result.status, SessionStatus::InvalidState);
    assert_eq!(cancel(&db, 9, 2).result.status, SessionStatus::NotFound);
}

// 이하 원본에 없는 시험. 지시받은 불변식을 고정한다.

#[test]
fn 시간_제한의_최악_합은_113초다() {
    let (db, orca) = seeded();
    orca.set("worktree create", Canned::Lost);
    orca.script("reference find", |_| {
        ok(json!({ "matches": [{ "workspace": { "id": WORKTREE_ID, "name": "t-11111111" }, "agents": [] }] }))
    });
    orca.set("terminal create", Canned::Lost);
    orca.set("terminal list", ok(json!({ "terminals": [] })));
    orca.set("worktree ps", ps_with(&["working"]));
    accept(&db, &orca);
    let waits: Vec<_> = orca.requests().iter().map(|call| call.timeout).collect();
    assert_eq!(
        waits,
        [ORCA_READ, ORCA_WORKTREE, ORCA_READ, ORCA_READ, ORCA_TERMINAL, ORCA_READ, ORCA_READ, ORCA_READ],
        "find, create, find, show, terminal, list, ps, 경고 ps"
    );
    assert_eq!(waits.iter().sum::<std::time::Duration>().as_secs(), 113);
}

#[test]
fn 잠금이_닫혀_있으면_선점하지_않고_url만_준다() {
    let (db, orca) = seeded();
    let (orca_client, clock) = (client(&orca), FixedClock(at(AT)));
    for (refusal, status) in [
        (SessionRefusal::not_eligible("차단 시험 전"), SessionStatus::NotEligible),
        (SessionRefusal::gate_closed("표시 꺼짐"), SessionStatus::GateClosed),
    ] {
        let admission = || Some(refusal.clone());
        let launcher = SessionLauncher::new(&db, &orca_client, &clock, &admission, RUN_BINARY);
        let value = launcher.accept(1).unwrap();
        assert_eq!(value.result.status, status);
        assert_eq!(value.result.state, Some(ItemState::Open));
        assert_eq!(value.url, Some(issue_url(1)));
        assert_eq!(value.run, None);
        assert_eq!(launcher.accept(9).unwrap().result.state, None, "없는 항목은 상태가 없다");
    }
    assert!(orca.calls().is_empty());
    assert_eq!(count(&db, "runs"), 0);
    assert_eq!(item(&db, 1).state, ItemState::Open);
}

#[test]
fn 수락_도중_취소하면_외부_쓰기를_멈춘다() {
    for (command, expected_create, expected_terminal) in [("reference find", 0, 0), ("worktree show", 1, 0)] {
        let (dir, home) = temp_home();
        let db = Database::open(&home).unwrap();
        seed_issue(&db);
        let orca = FakeOrca::new();
        let path = dir.path().to_owned();
        let reply = match command {
            "reference find" => json!({ "matches": [] }),
            _ => json!({ "worktree": worktree() }),
        };
        // 사용자가 Orca 호출 사이에 다른 프로세스에서 취소를 누른 경우다.
        orca.script(command, move |_| {
            let other = Database::open(&KairosHome::at(&path)).unwrap();
            cancel(&other, 1, 1);
            ok(reply.clone())
        });
        let value = accept(&db, &orca);
        assert_eq!(value.result.status, SessionStatus::Aborted, "{command}");
        assert_eq!(value.result.state, Some(ItemState::Open));
        assert_eq!(orca.count("worktree create"), expected_create, "{command}");
        assert_eq!(orca.count("terminal create"), expected_terminal, "{command}");
        assert_eq!(run(&db).state, RunState::Failed);
        assert_eq!(run(&db).launched_at, None, "취소한 run에 launched_at을 쓰지 않는다");
    }
}

#[test]
fn 수락_도중_항목이_닫히면_이미_처리됨을_돌려준다() {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    let orca = FakeOrca::new();
    let path = dir.path().to_owned();
    orca.script("worktree show", move |_| {
        let other = Database::open(&KairosHome::at(&path)).unwrap();
        ItemActions::new(&other).done(&[1], at(AT), false).unwrap();
        ok(json!({ "worktree": worktree() }))
    });
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::AlreadyClosed);
    assert_eq!(orca.count("terminal create"), 0);
}

#[test]
fn launched_at은_터미널을_만들기_전에_적는다() {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    let orca = FakeOrca::new();
    let path = dir.path().to_owned();
    orca.script("terminal create", move |_| {
        let other = Database::open(&KairosHome::at(&path)).unwrap();
        let row = run(&other);
        assert_eq!(row.launched_at.as_deref(), Some(AT));
        assert_eq!(row.worktree_identity.as_deref(), Some(SELECTOR));
        assert_eq!(row.pane_key, None);
        ok(json!({ "terminal": { "handle": "term_made", "paneKey": PANE } }))
    });
    accept(&db, &orca);
    assert_eq!(run(&db).pane_key.as_deref(), Some(PANE));
}
