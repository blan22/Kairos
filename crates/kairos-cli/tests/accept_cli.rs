//! 시험 7건. 나머지는 kairos-sessions에 있다
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;
use std::process::{Command, Stdio};
use std::thread;

use common::*;
use kairos_sessions::RUN_ID;
use serde_json::{Value, json};

#[test]
fn cli_accept_가짜_orca로_끝까지_돌고_형식을_지킨다() {
    let mut cli = cli_with_issue();
    let orca = cli.fake_orca();
    let done = cli.run(&["accept", "1", "--json"]);
    let body = done.body();
    assert_eq!(done.status, 0, "{}", done.err);
    assert_eq!(body["schema"], 1);
    assert_eq!(body["ok"], true);
    assert_eq!(
        body["results"][0],
        json!({ "id": 1, "status": "changed", "state": "accepted", "message": "세션을 여는 중입니다" })
    );
    let run_id = body["run"]["id"].as_str().unwrap();
    assert!(RUN_ID.is_match(run_id));
    assert_eq!(body["run"]["state"], "claimed");
    let calls = orca.calls();
    let home = cli.root.to_str().unwrap();
    assert!(calls.iter().all(|call| call.cwd == home), "고정 cwd에서 부른다");
    let create = calls.iter().find(|call| call.args[..2] == ["terminal", "create"]).unwrap();
    let index = create.args.iter().position(|arg| arg == "--command").unwrap();
    // 개발 빌드의 설치 구조 기준 경로다. `<target>/debug/kairos`의 `<target>/bin/kairos-run`.
    let binary = std::fs::canonicalize(BIN).unwrap();
    let run_binary = binary.parent().unwrap().parent().unwrap().join("bin").join("kairos-run");
    assert_eq!(create.args[index + 1], format!("'{}' {run_id}", run_binary.display()));
    assert_eq!(cli.state()["runs"], json!({ "active": 1, "ready": 0 }));
}

#[test]
fn cli_orca_터미널_안에서_부른_이동도_호출자_값을_넘기지_않는다() {
    let mut cli = cli_with_issue();
    let orca = cli.fake_orca();
    cli.run(&["accept", "1", "--json"]);
    assert_eq!(cli.run_with(&[("ORCA_TERMINAL_HANDLE", "term_caller")], &["move", "1", "--json"]).status, 0);
    assert!(orca.calls().iter().all(|call| call.handle.is_empty()), "ORCA_TERMINAL_HANDLE을 지운다");
}

#[test]
fn cli_여러_프로세스가_동시에_수락해도_worktree와_run은_하나다() {
    let mut cli = cli_with_issue();
    let orca = cli.fake_orca();
    let children: Vec<_> = (0..5)
        .map(|_| {
            Command::new(BIN)
                .args(["accept", "1", "--json"])
                .env_clear()
                .envs(&cli.env)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    let results: Vec<_> = children
        .into_iter()
        .map(|child| thread::spawn(move || child.wait_with_output().unwrap()))
        .map(|waiting| waiting.join().unwrap())
        .collect();
    assert!(results.iter().all(|result| result.status.code() == Some(0)));
    let bodies: Vec<Value> = results.iter().map(|result| serde_json::from_slice(&result.stdout).unwrap()).collect();
    let mut statuses: Vec<&str> = bodies.iter().map(|body| body["results"][0]["status"].as_str().unwrap()).collect();
    statuses.sort_unstable();
    assert_eq!(statuses, ["changed", "unchanged", "unchanged", "unchanged", "unchanged"]);
    let runs: BTreeSet<&str> = bodies.iter().map(|body| body["run"]["id"].as_str().unwrap()).collect();
    assert_eq!(runs.len(), 1, "진 쪽도 같은 run을 받는다");
    assert_eq!(orca.count("worktree", "create"), 1);
    assert_eq!(orca.count("terminal", "create"), 1);
    assert_eq!(count(&cli.db(), "runs"), 1);
}

#[test]
fn cli_runs와_move_형식과_state_json() {
    let mut cli = cli_with_issue();
    let orca = cli.fake_orca();
    let accepted = cli.run(&["accept", "1", "--json"]).body();
    let runs = cli.run(&["runs", "--json"]);
    let body = runs.body();
    assert_eq!(runs.status, 0);
    assert_eq!((&body["schema"], &body["ok"], &body["orca"]), (&json!(1), &json!(true), &json!(true)));
    assert_eq!(body["runs"], json!([{ "id": accepted["run"]["id"], "itemId": 1, "state": "claimed", "ready": null }]));
    let moved = cli.run(&["move", "1", "--json"]);
    assert_eq!(moved.status, 0);
    assert_eq!(
        moved.body()["results"][0],
        json!({ "id": 1, "status": "changed", "state": "accepted", "message": "이동했습니다" })
    );
    assert_eq!(orca.calls().last().unwrap().args, ["terminal", "switch", "--terminal", "term_now", "--json"]);
}

#[test]
fn cli_잘못된_입력은_사용법_오류이고_실패는_종료_코드_1이다() {
    let mut cli = cli_with_issue();
    let orca = cli.fake_orca();
    for (args, status, code) in [
        (&["accept", "--json"][..], 2, "usage"),
        (&["accept", "1;rm", "--json"], 2, "bad_id"),
        (&["accept", "1", "2", "--json"], 2, "usage"),
        (&["move", "--bogus", "--json"], 2, "usage"),
    ] {
        let done = cli.run(args);
        assert_eq!((done.status, &done.body()["error"]["code"]), (status, &json!(code)), "{args:?}");
    }
    let missing = cli.run(&["accept", "99", "--json"]);
    let body = missing.body();
    assert_eq!((missing.status, &body["ok"], &body["error"]["code"]), (1, &json!(false), &json!("not_found")));
    assert!(!orca.called(), "여기까지 Orca를 부르지 않았다");
}

#[test]
fn cli_orca_실행_파일이_없으면_항목을_되돌리고_실패로_답한다() {
    let cli = cli_with_issue();
    let done = cli.run(&["accept", "1", "--json"]);
    let body = done.body();
    assert_eq!((done.status, &body["ok"], &body["error"]["code"]), (1, &json!(false), &json!("orca_unavailable")));
    assert_eq!(item(&cli.db(), 1).state.to_string(), "open");
}

#[test]
fn cli_accept_cancel() {
    let mut cli = cli_with_issue();
    cli.fake_orca();
    cli.run(&["accept", "1", "--json"]);
    let done = cli.run(&["accept", "1", "--cancel", "--json"]);
    let body = done.body();
    assert_eq!(done.status, 0);
    assert_eq!((&body["results"][0]["status"], &body["results"][0]["state"]), (&json!("changed"), &json!("open")));
    assert_eq!(cli.state()["runs"], json!({ "active": 0, "ready": 0 }));
}
