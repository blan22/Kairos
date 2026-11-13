//! `[hosts.orca.repos]`가 없을 때 작업 저장소의 Orca ID를 `repo list`로 찾는다.
//! 프로필은 프로세스에 하나라서 이 파일은 Orca ID가 빠진 합성 프로필을 따로 정한다.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_agenda::ItemState;
use kairos_core::profile::{self, Profile};
use kairos_sessions::{RunState, SessionStatus};
use serde_json::{Value, json};

const FOUND: &str = "0000c0de-0000-4000-8000-0000000000aa";
const OTHER: &str = "0000c0de-0000-4000-8000-0000000000bb";

fn without_orca_repo() {
    let mut value = Profile::fixture();
    for target in &mut value.playbooks.targets {
        target.orca_repo = None;
    }
    profile::install(value).unwrap();
}

fn repo(id: &str, path: &str, key: &str) -> Value {
    json!({ "id": id, "path": path, "kind": "git", "gitRemoteIdentity": { "canonicalKey": key } })
}

#[test]
fn 설정이_없으면_원격_주소가_같은_orca_저장소로_만든다() {
    without_orca_repo();
    let (db, orca) = seeded();
    orca.set(
        "repo list",
        ok(json!({ "repos": [
            repo(OTHER, "/synthetic/atlas-specs", "github.com/contoso/atlas-specs"),
            repo(FOUND, "/synthetic/atlas", "github.com/contoso/atlas"),
        ] })),
    );
    let value = accept(&db, &orca);
    assert_eq!(value.result.status, SessionStatus::Changed);
    assert_eq!(orca.count("repo list"), 1);
    let args = orca.args("worktree create");
    let at = args.iter().position(|arg| arg == "--repo").unwrap();
    assert_eq!(args[at + 1], format!("id:{FOUND}"));
}

#[test]
fn orca_저장소를_고르지_못하면_worktree를_만들지_않고_까닭을_알린다() {
    without_orca_repo();
    let replies = [
        ok(json!({ "repos": [repo(OTHER, "/synthetic/atlas-specs", "github.com/contoso/atlas-specs")] })),
        ok(json!({ "repos": [
            repo(FOUND, "/synthetic/atlas-2", "github.com/contoso/atlas"),
            repo(OTHER, "/synthetic/atlas-3", "github.com/contoso/atlas"),
        ] })),
        Canned::Refused("unknown_command"),
    ];
    for reply in replies {
        let db = test_db();
        seed_issue(&db);
        let orca = FakeOrca::with("repo list", reply);
        let value = accept(&db, &orca);
        assert_eq!((value.result.status, value.result.state), (SessionStatus::RepoUnresolved, Some(ItemState::Open)));
        assert!(value.result.message.contains("[hosts.orca.repos]"), "{}", value.result.message);
        assert_eq!(run(&db).state, RunState::Failed);
        assert_eq!(orca.count("worktree create"), 0);
    }
}

#[test]
fn repo_list가_응답하지_않으면_orca_없음이다() {
    without_orca_repo();
    for reply in [Canned::Lost, Canned::Missing] {
        let db = test_db();
        seed_issue(&db);
        let orca = FakeOrca::with("repo list", reply);
        assert_eq!(accept(&db, &orca).result.status, SessionStatus::OrcaUnavailable);
        assert_eq!(orca.count("worktree create"), 0);
    }
}
