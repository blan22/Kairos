//! kairos-run의 띄우기 직전 재확인. 수집이 저장하지 못한 글을 가짜 gh로 준다. 실제 gh와 claude를 부르지 않는다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};

use common::*;
use kairos_agenda::ItemState;
use kairos_core::KairosHome;
use kairos_core::ports::FixedClock;
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_ledger::EventRepo;
use kairos_sessions::{RunState, SessionCommand, SessionProfile, SessionRunner, SessionSpawner, SessionWorkspace};
use kairos_source_github::{GithubConfig, LiveAuthorReader};
use kairos_store::Database;
use serde_json::{Value, json};
use tempfile::TempDir;

const GH: &str = "/synthetic/gh";

/// 토큰과 정한 GraphQL 답을 준다. 받은 요청을 적는다.
struct FakeGh {
    graphql: ProcessOutput,
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl ProcessRunner for FakeGh {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        self.calls.lock().unwrap().push((request.program.clone(), request.args.clone()));
        match request.args[0].as_str() {
            "auth" => ProcessOutput::exited(0, "synthetic-token\n"),
            "api" if request.args[1] == "graphql" => self.graphql.clone(),
            _ => panic!("시험에 없는 gh 호출: {:?}", request.args),
        }
    }
}

/// 띄우면 센다.
#[derive(Default)]
struct Counting(Mutex<usize>);

impl SessionSpawner for Counting {
    fn spawn(&self, _command: &SessionCommand) -> i32 {
        *self.0.lock().unwrap() += 1;
        0
    }
}

fn writing(login: &str, association: &str) -> Value {
    json!({ "authorAssociation": association, "author": { "__typename": "User", "login": login } })
}

/// 조직의 사람이 연 이슈와 그 댓글 전체.
fn live_issue(comments: Vec<Value>) -> ProcessOutput {
    let total = comments.len();
    let node = json!({
        "__typename": "Issue", "id": "I_synth", "number": 1,
        "authorAssociation": "MEMBER", "author": { "__typename": "User", "login": "synthetic-lead" },
        "comments": { "totalCount": total, "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": comments },
    });
    ProcessOutput::exited(0, json!({ "data": { "repository": { "issueOrPullRequest": node } } }).to_string())
}

fn accepted_home() -> (TempDir, KairosHome) {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    accept(&db, &FakeOrca::new());
    (dir, home)
}

fn config() -> GithubConfig {
    GithubConfig::from_vars(|key| match key {
        "KAIROS_GH" => Some(GH.to_owned()),
        "PATH" => Some("/synthetic/bin".to_owned()),
        "HOME" => Some("/synthetic/home".to_owned()),
        _ => None,
    })
}

/// 실제 `LiveAuthorReader`에 가짜 gh를 붙여 kairos-run을 돈다.
fn run_with_gh(home: &KairosHome, graphql: ProcessOutput) -> (i32, usize, Arc<FakeGh>) {
    let gh = Arc::new(FakeGh { graphql, calls: Mutex::new(Vec::new()) });
    let live = LiveAuthorReader::new(gh.clone(), config());
    write_session_token(home, SYNTHETIC_TOKEN, 0o600);
    let profile = SessionProfile::new(
        Path::new("/synthetic/kairos"),
        Path::new("/synthetic/kairos-home"),
        Path::new("/synthetic/home"),
    )
    .with_workspace(
        SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap(),
    );
    let (spawner, clock) = (Counting::default(), FixedClock(at(AT)));
    let runner =
        SessionRunner::new(home.clone(), Some("/synthetic/claude".to_owned()), &profile, &spawner, &live, &clock);
    let code = runner.run(&[RUN.to_owned()]).unwrap();
    let spawned = *spawner.0.lock().unwrap();
    (code, spawned, gh)
}

fn inspect<T>(home: &KairosHome, read: impl FnOnce(&Database) -> T) -> T {
    read(&Database::open(home).unwrap())
}

#[test]
fn db에_없는_t3_댓글을_띄우기_직전에_찾으면_claude를_띄우지_않는다() {
    let (_dir, home) = accepted_home();
    let stored = |db: &Database| EventRepo::new(db.connection()).has_outsider_on_object("gh:acme/widget#1").unwrap();
    assert!(!inspect(&home, stored), "DB 게이트는 통과한 상태다");
    let comments = vec![writing("synthetic-teammate", "MEMBER"), writing("synthetic-outsider", "NONE")];
    let (code, spawned, gh) = run_with_gh(&home, live_issue(comments));
    assert_eq!((code, spawned), (1, 0));
    let calls = gh.calls.lock().unwrap().clone();
    assert!(calls.iter().all(|(program, _)| program == GH), "설정한 gh만 부른다");
    assert_eq!(calls.len(), 2, "토큰과 질의 하나");
    let row = inspect(&home, run);
    assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some("outsider_writing")));
    assert_eq!(inspect(&home, |db| item(db, 1).state), ItemState::Open);
    assert!(inspect(&home, stored), "본 외부 글은 DB 게이트에도 남는다");
}

/// 수집은 연관 정보가 없는 글을 조직의 사람으로 본다. 재확인은 믿지 않고 막지만 지우지 않는 외부 표시는 남기지 않는다.
#[test]
fn 작성자를_알_수_없는_글은_띄우지_않고_db_게이트에_외부_표시를_남기지_않는다() {
    let ghost = json!({ "authorAssociation": "NONE", "author": null });
    let no_association = json!({ "author": { "__typename": "User", "login": "synthetic-member" } });
    for (name, comment) in [("지운 계정", ghost), ("연관 정보 없음", no_association)] {
        let (_dir, home) = accepted_home();
        let stored =
            |db: &Database| EventRepo::new(db.connection()).has_outsider_on_object("gh:acme/widget#1").unwrap();
        let (code, spawned, _gh) =
            run_with_gh(&home, live_issue(vec![writing("synthetic-teammate", "MEMBER"), comment]));
        assert_eq!((code, spawned), (1, 0), "{name}");
        let row = inspect(&home, run);
        assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some("live_unverified")), "{name}");
        assert_eq!(inspect(&home, |db| item(db, 1).state), ItemState::Open, "{name}");
        assert!(!inspect(&home, stored), "{name}: 외부 글로 단정하지 않는다");
    }
}

#[test]
fn github을_읽지_못하면_claude를_띄우지_않는다() {
    for (name, graphql) in [
        ("시간 초과", ProcessOutput::ended(ProcessEnd::TimedOut)),
        ("gh 실패", ProcessOutput { end: ProcessEnd::Exited(1), stdout: String::new(), stderr: "HTTP 502".to_owned() }),
        ("부분 오류", ProcessOutput::exited(0, json!({ "data": null, "errors": [{ "type": "X" }] }).to_string())),
    ] {
        let (_dir, home) = accepted_home();
        let (code, spawned, _gh) = run_with_gh(&home, graphql);
        assert_eq!((code, spawned), (1, 0), "{name}");
        let row = inspect(&home, run);
        assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some("live_unverified")), "{name}");
        assert_eq!(inspect(&home, |db| item(db, 1).state), ItemState::Open, "{name}");
    }
}

#[test]
fn 대조_조직의_사람과_봇의_글뿐이면_띄운다() {
    let (_dir, home) = accepted_home();
    let bot = json!({ "authorAssociation": "NONE", "author": { "__typename": "Bot", "login": "synthetic[bot]" } });
    let (code, spawned, _gh) = run_with_gh(&home, live_issue(vec![writing("synthetic-teammate", "COLLABORATOR"), bot]));
    assert_eq!((code, spawned), (0, 1));
    assert_eq!(inspect(&home, run).state, RunState::Ended);
}
