//! kairos-run의 선점과 끝.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::path::Path;
use std::sync::Mutex;

use common::*;
use kairos_agenda::{ItemState, Playbook, ReadyKind};
use kairos_core::KairosHome;
use kairos_core::ports::FixedClock;
use kairos_sessions::{
    ClaimRefusal, LiveAuthorCheck, RunState, SESSION_GH_TOKEN_FILE, SessionClaim, SessionCommand, SessionProfile,
    SessionRunner, SessionSpawner, SessionStatus, SessionWorkspace,
};
use kairos_source_github::LiveAuthors;
use kairos_store::{Database, params};
use tempfile::TempDir;

fn accepted_home() -> (TempDir, KairosHome) {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    accept(&db, &FakeOrca::new());
    write_session_token(&home, &format!("{SYNTHETIC_TOKEN}\n"), 0o600);
    (dir, home)
}

fn inspect<T>(home: &KairosHome, read: impl FnOnce(&Database) -> T) -> T {
    read(&Database::open(home).unwrap())
}

/// 받은 인자를 적고 정한 종료 코드를 돌려준다.
struct Recording<F> {
    seen: Mutex<Vec<SessionCommand>>,
    during: F,
}

impl<F: Fn() -> i32> Recording<F> {
    fn new(during: F) -> Self {
        Self { seen: Mutex::new(Vec::new()), during }
    }

    fn count(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}

impl<F: Fn() -> i32> SessionSpawner for Recording<F> {
    fn spawn(&self, command: &SessionCommand) -> i32 {
        self.seen.lock().unwrap().push(command.clone());
        (self.during)()
    }
}

fn no_spawn() -> Recording<fn() -> i32> {
    Recording::new(|| panic!("claude를 띄우면 안 된다"))
}

fn run_session(home: &KairosHome, argv: &[&str], claude: Option<&str>, spawner: &dyn SessionSpawner) -> i32 {
    run_session_live(home, argv, claude, spawner, &FakeLive::insiders())
}

fn run_session_live(
    home: &KairosHome,
    argv: &[&str],
    claude: Option<&str>,
    spawner: &dyn SessionSpawner,
    live: &dyn LiveAuthorCheck,
) -> i32 {
    let profile = SessionProfile::new(
        Path::new("/synthetic/kairos"),
        Path::new("/synthetic/kairos-home"),
        Path::new("/synthetic/home"),
    )
    .with_workspace(
        SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap(),
    );
    let clock = FixedClock(at(AT));
    let runner = SessionRunner::new(home.clone(), claude.map(str::to_owned), &profile, spawner, live, &clock);
    let argv: Vec<String> = argv.iter().map(|arg| (*arg).to_owned()).collect();
    runner.run(&argv).unwrap()
}

#[test]
fn kairos_run_uuid가_아니면_db를_열기_전에_멈춘다() {
    let (dir, _) = temp_home();
    let home = KairosHome::at(dir.path().join("not-created"));
    let spawner = no_spawn();
    let injected = format!("{RUN}; rm -rf ~");
    let cases: [&[&str]; 5] = [&[], &["bad"], &[&injected], &[RUN, "extra"], &["ABCDEF11-2222-4333-8444-555555555555"]];
    for argv in cases {
        assert_eq!(run_session(&home, argv, Some("/c"), &spawner), 2, "{argv:?}");
    }
    assert!(!home.root().exists());
}

#[test]
fn kairos_run_같은_run은_한_번만_뜨고_끝나면_ended다() {
    let (_dir, home) = accepted_home();
    let spawner = Recording::new(|| 0);
    assert_eq!(run_session(&home, &[RUN], Some("/synthetic/claude"), &spawner), 0);
    assert_eq!(spawner.count(), 1);
    let (state, ready, item_state) = inspect(&home, |db| (run(db).state, run(db).ready_kind, item(db, 1).state));
    assert_eq!((state, ready, item_state), (RunState::Ended, Some(ReadyKind::Ended), ItemState::Accepted));
    assert_eq!(run_session(&home, &[RUN], Some("/synthetic/claude"), &spawner), 1);
    assert_eq!(spawner.count(), 1, "두 번째 실행은 claude를 띄우지 않는다");
}

#[test]
fn kairos_run_시작_표시는_세션이_도는_동안_두_번째_실행을_막는다() {
    let (_dir, home) = accepted_home();
    let db = Database::open(&home).unwrap();
    assert!(matches!(SessionRunner::claim(&db, RUN, at(AT)).unwrap(), SessionClaim::Won { .. }));
    assert_eq!(SessionRunner::claim(&db, RUN, at(AT)).unwrap(), SessionClaim::Refused(ClaimRefusal::Used));
}

#[test]
fn kairos_run_없는_run_취소한_run_닫힌_항목_위조_url은_띄우지_않는다() {
    let spawner = no_spawn();
    let (_dir, home) = accepted_home();
    assert_eq!(run_session(&home, &[OTHER_RUN], Some("/c"), &spawner), 1);

    let forged = format!("{}; rm -rf ~", issue_url(1));
    let tamper: [(&str, &[&str]); 7] = [
        ("UPDATE runs SET state = 'failed', ended_at = 'x'", &[]),
        ("UPDATE items SET state = 'closed', closed_at = 'x', close_reason = 'user_done'", &[]),
        ("UPDATE objects SET permalink = ?", &[&forged]),
        ("UPDATE objects SET permalink = 'https://github.com/acme/widget/issues/1'", &[]),
        ("UPDATE objects SET trust_tier = 'T3'", &[]),
        ("UPDATE objects SET state = 'closed'", &[]),
        ("UPDATE runs SET adopted = 1", &[]),
    ];
    for (change, values) in tamper {
        let (_other_dir, other) = accepted_home();
        inspect(&other, |db| db.connection().execute(change, kairos_store::params_from_iter(values.iter())).unwrap());
        assert_eq!(run_session(&other, &[RUN], Some("/c"), &spawner), 1, "{change}");
    }
}

#[test]
fn kairos_run_보이기_전에_실패로_끝나면_항목을_되돌리고_본_뒤의_실패는_ended다() {
    let (_dir, home) = accepted_home();
    assert_eq!(run_session(&home, &[RUN], Some("/c"), &Recording::new(|| 1)), 1);
    assert_eq!(inspect(&home, |db| (run(db).state, item(db, 1).state)), (RunState::Failed, ItemState::Open));

    let (_seen_dir, seen) = accepted_home();
    let spawner = Recording::new(|| {
        // 세션이 도는 동안 추적이 ps에서 본 경우다.
        inspect(&seen, |db| {
            db.connection().execute("UPDATE runs SET seen_at = ?, state = 'started'", params![AT]).unwrap()
        });
        1
    });
    run_session(&seen, &[RUN], Some("/c"), &spawner);
    assert_eq!(inspect(&seen, |db| (run(db).state, item(db, 1).state)), (RunState::Ended, ItemState::Accepted));
}

#[test]
fn kairos_run_claude가_없으면_실패로_내린다() {
    let (_dir, home) = accepted_home();
    assert_eq!(run_session(&home, &[RUN], None, &no_spawn()), 1);
    assert_eq!(inspect(&home, |db| (run(db).state, item(db, 1).state)), (RunState::Failed, ItemState::Open));
}

#[test]
fn 추적과_kairos_run의_끝이_겹쳐도_상태가_되돌아가지_않는다() {
    let (_dir, home) = accepted_home();
    run_session(&home, &[RUN], Some("/c"), &Recording::new(|| 0));
    let orca = FakeOrca::with("worktree ps", ps_with(&["working"]));
    let report = inspect(&home, |db| sync(db, &orca, 0));
    assert!(report.runs.is_empty());
    assert_eq!(inspect(&home, |db| run(db).state), RunState::Ended);
}

// 이하 원본에 없는 시험.

#[test]
fn 실패_코드는_띄우지_못함과_세션_실패를_나눈다() {
    for (exit, code) in [(127, "no_claude"), (1, "session_failed"), (7, "session_failed")] {
        let (_dir, home) = accepted_home();
        run_session(&home, &[RUN], Some("/c"), &Recording::new(move || exit));
        let row = inspect(&home, run);
        assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some(code)), "{exit}");
        assert_eq!(row.ready_at.as_deref(), Some(AT));
        assert_eq!(row.ready_kind, None);
    }
}

#[test]
fn claude_경로가_빈_문자열이면_없는_것이다() {
    let (_dir, home) = accepted_home();
    assert_eq!(run_session(&home, &[RUN], Some(""), &no_spawn()), 1);
    assert_eq!(inspect(&home, run).fail_code.as_deref(), Some("no_claude"));
}

#[test]
fn 세션의_인자는_프로필의_값이고_url은_db에서_읽는다() {
    let (_dir, home) = accepted_home();
    let spawner = Recording::new(|| 0);
    run_session(&home, &[RUN], Some("/synthetic/claude"), &spawner);
    let expected = SessionProfile::new(
        Path::new("/synthetic/kairos"),
        Path::new("/synthetic/kairos-home"),
        Path::new("/synthetic/home"),
    )
    .with_workspace(
        SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap(),
    )
    .command("/synthetic/claude", RUN, Playbook::QaIssue, &issue_url(1), session_token())
    .unwrap();
    assert_eq!(spawner.seen.lock().unwrap()[0], expected);
}

#[test]
fn 세션의_프롬프트는_run에_적힌_플레이북을_따른다() {
    let (_dir, home) = accepted_home();
    inspect(&home, |db| db.connection().execute("UPDATE runs SET playbook = 'pr_review'", []).unwrap());
    let spawner = Recording::new(|| 0);
    run_session(&home, &[RUN], Some("/synthetic/claude"), &spawner);
    let prompt = SessionProfile::prompt(Playbook::PrReview, &issue_url(1)).unwrap();
    assert_eq!(spawner.seen.lock().unwrap()[0].args.last(), Some(&prompt));
}

#[test]
fn 토큰_파일이_있으면_자식_환경과_settings_파일로만_가고_인자에는_없다() {
    let (_dir, home) = accepted_home();
    let spawner = Recording::new(|| 0);
    assert_eq!(run_session(&home, &[RUN], Some("/synthetic/claude"), &spawner), 0);
    let seen = spawner.seen.lock().unwrap();
    assert!(seen[0].has_gh_token());
    assert_eq!(seen[0].child_env().collect::<Vec<_>>(), [("GH_TOKEN", SYNTHETIC_TOKEN)]);
    assert!(!seen[0].args.iter().any(|arg| arg.contains("SYNTHETIC0123")), "토큰은 인자에 없다");
    assert!(!format!("{:?}", seen[0]).contains("SYNTHETIC0123"), "Debug로도 새지 않는다");
    // --settings는 파일 경로이고, 그 파일의 env가 GH_TOKEN을 읽기 전용 토큰으로 고정한다.
    let index = seen[0].args.iter().position(|arg| arg == "--settings").unwrap();
    assert_eq!(Path::new(&seen[0].args[index + 1]), seen[0].settings_path().unwrap());
    let settings: serde_json::Value = serde_json::from_str(seen[0].settings_content().unwrap()).unwrap();
    assert_eq!(settings["env"]["GH_TOKEN"], SYNTHETIC_TOKEN);
}

#[test]
fn 남이_읽을_수_있는_토큰_파일이면_run을_시작으로_적기_전에_멈춘다() {
    let (_dir, home) = accepted_home();
    for mode in [0o644, 0o640, 0o604] {
        write_session_token(&home, SYNTHETIC_TOKEN, mode);
        assert_eq!(run_session(&home, &[RUN], Some("/synthetic/claude"), &no_spawn()), 1, "{mode:o}");
        assert_eq!(inspect(&home, |db| run(db).session_started_at), None, "{mode:o}");
    }
}

/// 토큰이 없을 때 소유자의 키체인 gh 로그인으로 내려가면 세션이 소유자 권한으로 GitHub에 쓸 수 있다. 시작하지 않는다.
#[test]
fn 토큰_파일이_없으면_키체인으로_내려가지_않고_run을_시작으로_적기_전에_멈춘다() {
    let (_dir, home) = accepted_home();
    std::fs::remove_file(home.file(SESSION_GH_TOKEN_FILE)).unwrap();
    let before = inspect(&home, |db| run(db).state);
    assert_eq!(run_session(&home, &[RUN], Some("/synthetic/claude"), &no_spawn()), 1);
    assert_eq!(inspect(&home, |db| run(db).session_started_at), None);
    assert_eq!(inspect(&home, |db| run(db).state), before, "run을 선점하지 않는다");
}

/// 쓰기 범위를 함께 가진 classic 토큰과 gh 로그인 토큰은 받지 않는다.
#[test]
fn fine_grained가_아닌_토큰이면_띄우지_않는다() {
    let (_dir, home) = accepted_home();
    for content in ["ghp_SYNTHETIC0123456789abcdefABCDEF", "gho_SYNTHETIC0123456789abcdefABCDEF"] {
        write_session_token(&home, content, 0o600);
        assert_eq!(run_session(&home, &[RUN], Some("/synthetic/claude"), &no_spawn()), 1, "{content}");
        assert_eq!(inspect(&home, |db| run(db).session_started_at), None, "{content}");
    }
}

#[test]
fn 시작_표시는_claude를_띄우기_전에_남는다() {
    let (_dir, home) = accepted_home();
    let spawner = Recording::new(|| {
        assert_eq!(inspect(&home, run).session_started_at.as_deref(), Some(AT));
        0
    });
    run_session(&home, &[RUN], Some("/c"), &spawner);
    assert_eq!(spawner.count(), 1);
}

#[test]
fn kairos_run_띄우기_직전에_외부_글을_보면_띄우지_않고_항목을_되돌린_뒤_계속_막는다() {
    let (_dir, home) = accepted_home();
    let live = FakeLive::new(Ok(LiveAuthors::Outsider));
    assert_eq!(run_session_live(&home, &[RUN], Some("/c"), &no_spawn(), &live), 1);
    assert_eq!(*live.urls.lock().unwrap(), [issue_url(1)], "DB에서 읽은 URL로 본다");
    let row = inspect(&home, run);
    assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some("outsider_writing")));
    assert_eq!(row.session_started_at.as_deref(), Some(AT), "같은 run은 다시 뜨지 않는다");
    assert_eq!(inspect(&home, |db| item(db, 1).state), ItemState::Open);
    assert_eq!(run_session(&home, &[RUN], Some("/c"), &no_spawn()), 1);

    // 본 외부 글은 객체에 남아 DB 게이트도 막는다. 되돌린 항목을 다시 눌러도 Orca를 부르지 않는다.
    let orca = FakeOrca::new();
    let again = inspect(&home, |db| accept_as(db, &orca, 1, OTHER_RUN));
    assert_eq!(again.result.status, SessionStatus::NotEligible);
    assert!(orca.calls().is_empty());
}

#[test]
fn kairos_run_글을_다_읽지_못하면_띄우지_않고_다음_시작은_막지_않는다() {
    for code in ["live_gh_timeout", "live_incomplete", "live_gh_auth"] {
        let (_dir, home) = accepted_home();
        let live = FakeLive::new(Err(code));
        assert_eq!(run_session_live(&home, &[RUN], Some("/c"), &no_spawn(), &live), 1, "{code}");
        let row = inspect(&home, run);
        assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some("live_unverified")), "{code}");
        assert_eq!(inspect(&home, |db| (item(db, 1).state, count(db, "outsider_marks"))), (ItemState::Open, 0));
        let orca = FakeOrca::new();
        let again = inspect(&home, |db| accept_as(db, &orca, 1, OTHER_RUN));
        assert_eq!(again.result.status, SessionStatus::Changed, "일시적인 실패라 다시 시작하면 또 읽는다");
    }
}

/// 지운 계정이나 연관 정보가 빠진 글은 띄우지 않지만 외부 글로 단정하지 않는다. 지우지 않는 외부 표시를 남기지 않는다.
#[test]
fn kairos_run_작성자를_알_수_없는_글이면_띄우지_않고_외부_표시는_남기지_않는다() {
    let (_dir, home) = accepted_home();
    let live = FakeLive::new(Ok(LiveAuthors::Unknown));
    assert_eq!(run_session_live(&home, &[RUN], Some("/c"), &no_spawn(), &live), 1);
    let row = inspect(&home, run);
    assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some("live_unverified")));
    assert_eq!(row.session_started_at.as_deref(), Some(AT), "같은 run은 다시 뜨지 않는다");
    assert_eq!(inspect(&home, |db| (item(db, 1).state, count(db, "outsider_marks"))), (ItemState::Open, 0));
    let orca = FakeOrca::new();
    let again = inspect(&home, |db| accept_as(db, &orca, 1, OTHER_RUN));
    assert_eq!(again.result.status, SessionStatus::Changed, "작성자가 확인되면 다시 시작할 수 있다");
}

/// 글을 읽는 사이에 사용자가 수락을 취소한다.
struct CancelsWhileReading<'a>(&'a KairosHome);

impl LiveAuthorCheck for CancelsWhileReading<'_> {
    fn live_authors(&self, _url: &str) -> kairos_core::KairosResult<LiveAuthors> {
        inspect(self.0, |db| cancel(db, 1, 0));
        Ok(LiveAuthors::Insiders)
    }
}

#[test]
fn kairos_run_글을_읽는_사이에_취소한_run은_띄우지_않는다() {
    let (_dir, home) = accepted_home();
    assert_eq!(run_session_live(&home, &[RUN], Some("/c"), &no_spawn(), &CancelsWhileReading(&home)), 1);
    assert_eq!(inspect(&home, |db| item(db, 1).state), ItemState::Open);
}

#[test]
fn kairos_run_claude가_없으면_github을_읽지_않는다() {
    let (_dir, home) = accepted_home();
    let live = FakeLive::insiders();
    assert_eq!(run_session_live(&home, &[RUN], None, &no_spawn(), &live), 1);
    assert!(live.urls.lock().unwrap().is_empty());
}

#[test]
fn 확인_필요_run도_늦게_도착한_터미널_명령으로_뜬다() {
    let (_dir, home) = accepted_home();
    inspect(&home, |db| db.connection().execute("UPDATE runs SET state = 'needs_attention'", []).unwrap());
    let spawner = Recording::new(|| 0);
    assert_eq!(run_session(&home, &[RUN], Some("/c"), &spawner), 0);
    assert_eq!(spawner.count(), 1);
}

/// 고치는 세션은 작업 공간의 git 경로를 막고 뜬다. 작업 공간을 모르면 띄우지 않고 실패로 내린다.
#[test]
fn 작업_공간을_모르면_고치는_세션을_띄우지_않고_실패로_내린다() {
    let (_dir, home) = accepted_home();
    let profile = SessionProfile::new(
        Path::new("/synthetic/kairos"),
        Path::new("/synthetic/kairos-home"),
        Path::new("/synthetic/home"),
    );
    let clock = FixedClock(at(AT));
    let spawner = no_spawn();
    let live = FakeLive::insiders();
    let runner =
        SessionRunner::new(home.clone(), Some("/synthetic/claude".to_owned()), &profile, &spawner, &live, &clock);
    assert_eq!(runner.run(&[RUN.to_owned()]).unwrap(), 1);
    let row = inspect(&home, run);
    assert_eq!((row.state, row.fail_code.as_deref()), (RunState::Failed, Some("session_failed")));
    assert_eq!(inspect(&home, |db| item(db, 1).state), ItemState::Open);
}
