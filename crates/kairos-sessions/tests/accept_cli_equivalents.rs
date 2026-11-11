//! 시험. 프로세스 없이 같은 동작을 라이브러리로 본다
//! 응답 틀과 종료 코드, state.json은 kairos-cli의 몫이다.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use common::*;
use kairos_agenda::{ItemState, ReadOnlyRunRepo};
use kairos_core::KairosHome;
use kairos_core::ports::FixedClock;
use kairos_core::process::SystemProcessRunner;
use kairos_sessions::{OrcaClient, RUN_ID, RunSynchronizer, SessionLauncher, SessionOutcome, SessionStatus};
use kairos_store::Database;
use serde_json::json;

const CALLER: [&str; 5] =
    ["ORCA_TERMINAL_HANDLE", "ORCA_PANE_KEY", "ORCA_WORKSPACE_ID", "ORCA_WORKTREE_ID", "ORCA_CLI_CWD"];

/// 호출을 파일에 남기는 가짜 실행 파일. 원본의 fakeOrcaBin과 같은 응답을 낸다.
fn fake_orca_bin(root: &Path) -> (String, std::path::PathBuf) {
    let bin = root.join("fake-orca");
    let log = root.join("orca-calls.log");
    let worktree = worktree();
    let script = format!(
        r#"#!/bin/sh
printf '%s|%s|%s\n' "$(pwd -P)" "${{ORCA_TERMINAL_HANDLE-}}" "$*" >> '{log}'
case "$1 $2" in
'reference find') result='{{"matches":[],"truncated":false}}' ;;
'worktree create'|'worktree show') result='{{"worktree":{worktree}}}' ;;
'terminal create') result='{{"terminal":{{"handle":"term_made","tabId":"tab-synth","paneKey":"{PANE}"}}}}' ;;
'terminal list') result='{{"terminals":[{{"handle":"term_now","tabId":"tab-synth","leafId":"leaf-synth","connected":true}}]}}' ;;
'worktree ps') result='{{"worktrees":[],"truncated":false}}' ;;
'terminal switch') result='{{"focus":{{"handle":"term_now","tabId":"tab-synth"}}}}' ;;
*) printf '{{"ok":false,"error":{{"code":"unexpected"}}}}\n'; exit 1 ;;
esac
printf '{{"id":"req","ok":true,"result":%s,"_meta":{{}}}}\n' "$result"
"#,
        log = log.display(),
    );
    fs::write(&bin, script).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o700)).unwrap();
    (bin.to_str().unwrap().to_owned(), log)
}

fn system_client(bin: Option<String>, home: &KairosHome) -> OrcaClient {
    OrcaClient::new(Arc::new(SystemProcessRunner), bin, home.root())
}

fn accept_with(db: &Database, orca: &OrcaClient) -> SessionOutcome {
    let (clock, ids) = (FixedClock(at(AT)), FixedId(RUN.to_owned()));
    SessionLauncher::new(db, orca, &clock, &admit_all, RUN_BINARY).with_ids(&ids).accept(1).unwrap()
}

/// 원본: CLI accept: 가짜 orca로 끝까지 돌고 형식을 지킨다.
#[test]
fn 가짜_orca_파일로_끝까지_돌고_형식을_지킨다() {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    let (bin, log) = fake_orca_bin(dir.path());
    let value = accept_with(&db, &system_client(Some(bin), &home));
    assert_eq!(
        serde_json::to_value(&value.result).unwrap(),
        json!({ "id": 1, "status": "changed", "state": "accepted", "message": "세션을 여는 중입니다" })
    );
    let view = value.run.unwrap();
    assert!(RUN_ID.is_match(&view.id));
    assert_eq!(view.state, "claimed");
    let calls = fs::read_to_string(log).unwrap();
    let cwd = dir.path().canonicalize().unwrap();
    assert!(calls.lines().all(|line| line.starts_with(&format!("{}|", cwd.display()))), "고정 cwd에서 부른다");
    let create = calls.lines().find(|line| line.contains("|terminal create ")).unwrap();
    assert!(create.contains(&format!("--command '{RUN_BINARY}' {RUN} --focus --json")));
    assert_eq!(ReadOnlyRunRepo::new(db.connection()).active_count().unwrap(), 1);
}

/// 원본: CLI: Orca 터미널 안에서 부른 이동도 호출자 값을 넘기지 않는다.
#[test]
fn orca_터미널_안에서_부른_이동도_호출자_값을_넘기지_않는다() {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    let (bin, log) = fake_orca_bin(dir.path());
    let inside: BTreeMap<String, String> =
        CALLER.iter().map(|key| ((*key).to_owned(), "term_caller".to_owned())).collect();
    let orca = system_client(Some(bin), &home).with_env(inside);
    accept_with(&db, &orca);
    let clock = FixedClock(at(AT));
    let moved = SessionLauncher::new(&db, &orca, &clock, &admit_all, RUN_BINARY).move_to(1).unwrap();
    assert_eq!(moved.result.status, SessionStatus::Changed);
    let calls = fs::read_to_string(log).unwrap();
    assert!(calls.lines().all(|line| line.split('|').nth(1) == Some("")), "ORCA_TERMINAL_HANDLE을 지운다");
}

#[test]
fn 호출자_값은_환경에서_빠지고_나머지는_남는다() {
    let (db, orca) = seeded();
    let mut base: BTreeMap<String, String> = CALLER.iter().map(|key| ((*key).to_owned(), "합성".to_owned())).collect();
    base.insert("PATH".to_owned(), "/usr/bin".to_owned());
    base.insert("ORCA_AGENT_HOOK_PORT".to_owned(), "1".to_owned());
    let client = client(&orca).with_env(base);
    accept_with(&db, &client);
    for call in orca.requests() {
        let env = call.env.unwrap();
        assert!(CALLER.iter().all(|key| !env.contains_key(*key)));
        assert_eq!(env.len(), 2);
        assert_eq!(call.cwd.as_deref(), Some(Path::new("/synthetic/home")));
        assert_eq!(call.program, "/synthetic/orca");
    }
}

/// 원본: CLI runs와 move: 형식과 state.json.
#[test]
fn runs와_move의_형식() {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    let (bin, log) = fake_orca_bin(dir.path());
    let orca = system_client(Some(bin), &home);
    let accepted = accept_with(&db, &orca);
    let clock = FixedClock(at(AT));
    let report = RunSynchronizer::new(&db, &orca, &clock).sync().unwrap();
    assert_eq!(
        serde_json::to_string(&report).unwrap(),
        json!({ "reachable": true, "runs": [{ "id": accepted.run.unwrap().id, "itemId": 1, "state": "claimed", "ready": null }] })
            .to_string()
    );
    let moved = SessionLauncher::new(&db, &orca, &clock, &admit_all, RUN_BINARY).move_to(1).unwrap();
    assert_eq!(
        serde_json::to_value(&moved.result).unwrap(),
        json!({ "id": 1, "status": "changed", "state": "accepted", "message": "이동했습니다" })
    );
    let calls = fs::read_to_string(log).unwrap();
    assert!(calls.lines().last().unwrap().ends_with("|terminal switch --terminal term_now --json"));
}

/// 원본: CLI: orca 실행 파일이 없으면 항목을 되돌리고 실패로 답한다.
#[test]
fn orca_실행_파일이_없으면_항목을_되돌리고_실패로_답한다() {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    let missing = dir.path().join("no-such-orca").to_str().unwrap().to_owned();
    for bin in [Some(missing), None] {
        let value = accept_with(&db, &system_client(bin, &home));
        assert_eq!(value.result.status, SessionStatus::OrcaUnavailable);
        assert_eq!(item(&db, 1).state, ItemState::Open);
        db.connection().execute("DELETE FROM runs", []).unwrap();
    }
}

/// 원본: CLI accept --cancel.
#[test]
fn accept_cancel() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    let value = cancel(&db, 1, 0);
    assert_eq!((value.result.status, value.result.state), (SessionStatus::Changed, Some(ItemState::Open)));
    assert_eq!(ReadOnlyRunRepo::new(db.connection()).active_count().unwrap(), 0);
}

#[test]
fn 결과_json은_키_순서와_null을_지킨다() {
    let (db, orca) = seeded();
    let value = accept(&db, &orca);
    assert_eq!(
        serde_json::to_string(&value).unwrap(),
        format!(
            r#"{{"result":{{"id":1,"status":"changed","state":"accepted","message":"세션을 여는 중입니다"}},"run":{{"id":"{RUN}","state":"claimed"}},"url":null,"warning":null}}"#
        )
    );
}
