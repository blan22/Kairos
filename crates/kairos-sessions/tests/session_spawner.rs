//! 세션 끝까지 시험과 세션 생성 시험.
//! 신호 처리기는 프로세스 전체에 남으므로 실제 실행은 이 시험 파일을 다시 띄운 대역 프로세스에서 한다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant as Monotonic};

use common::*;
use kairos_agenda::{ItemState, Playbook};
use kairos_core::KairosHome;
use kairos_core::ports::{FixedClock, SystemClock};
use kairos_sessions::{
    CLEARED_ENV, RunState, SessionCommand, SessionLauncher, SessionProfile, SessionRunner, SessionSpawner,
    SessionWorkspace, SystemSessionSpawner,
};
use kairos_store::Database;

const DOUBLE: &str = "대역_프로세스";

/// 다른 시험이 다시 띄울 때만 일을 한다.
#[test]
#[ignore = "session_spawner 시험이 직접 띄운다"]
fn 대역_프로세스() {
    if let Ok(script) = std::env::var("KAIROS_TEST_SPAWN") {
        let args = vec!["-c".to_owned(), script];
        let command = SessionCommand::new(std::env::var("KAIROS_TEST_FILE").unwrap(), args);
        std::process::exit(SystemSessionSpawner.spawn(&command));
    }
    let home = KairosHome::at(std::env::var("KAIROS_TEST_HOME").unwrap());
    let claude = std::env::var("KAIROS_TEST_CLAUDE").ok();
    // --settings 파일은 데이터 폴더 아래에 쓰므로 실제 시험 데이터 폴더로 만든다.
    let profile = profile_at(&home);
    let live = FakeLive::insiders();
    let runner = SessionRunner::new(home, claude, &profile, &SystemSessionSpawner, &live, &SystemClock);
    let run_id = std::env::var("KAIROS_TEST_RUN_ID").unwrap();
    std::process::exit(runner.run(&[run_id]).unwrap());
}

fn profile_at(home: &KairosHome) -> SessionProfile {
    SessionProfile::new(Path::new("/synthetic/kairos"), home.root(), Path::new("/synthetic/home")).with_workspace(
        SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap(),
    )
}

fn double() -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--ignored", "--exact", DOUBLE, "--nocapture", "--test-threads=1"]);
    command.env_remove("KAIROS_TEST_SPAWN").env_remove("RUST_TEST_THREADS");
    command
}

fn spawn_script(script: &str) -> Command {
    let mut command = double();
    command.env("KAIROS_TEST_SPAWN", script).env("KAIROS_TEST_FILE", "/bin/sh");
    command.stdout(Stdio::null()).stderr(Stdio::null());
    command
}

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn wait_for(path: &Path) {
    let started = Monotonic::now();
    while !path.exists() {
        assert!(started.elapsed() < Duration::from_secs(20), "기다린 파일이 없다");
        thread::sleep(Duration::from_millis(10));
    }
}

fn signal(child: &Child, name: &str) {
    let status = Command::new("/bin/kill").args(["-s", name, &child.id().to_string()]).status().unwrap();
    assert!(status.success());
}

fn trap_script(dir: &Path) -> String {
    let (ready, got) = (dir.join("ready"), dir.join("got"));
    format!(
        "trap 'echo HUP > \"{got}\"; exit 5' HUP\n\
         trap 'echo TERM > \"{got}\"; exit 6' TERM\n\
         echo > \"{ready}\"\n\
         while :; do sleep 0.02; done\n",
        got = got.display(),
        ready = ready.display(),
    )
}

#[test]
fn 자식의_종료_코드를_그대로_돌려준다() {
    let status = spawn_script("exit 7").status().unwrap();
    assert_eq!(status.code(), Some(7));
}

#[test]
fn 신호로_죽은_자식은_1이다() {
    let status = spawn_script("kill -KILL $$").status().unwrap();
    assert_eq!(status.code(), Some(1));
}

#[test]
fn 띄우지_못하면_127이다() {
    let status = spawn_script("exit 0").env("KAIROS_TEST_FILE", "/nonexistent/synthetic").status().unwrap();
    assert_eq!(status.code(), Some(127));
}

#[test]
fn hup와_term을_자식에게_넘기고_자식을_기다린다() {
    for (name, code) in [("HUP", 5), ("TERM", 6)] {
        let dir = tempfile::tempdir().unwrap();
        let mut child = spawn_script(&trap_script(dir.path())).spawn().unwrap();
        wait_for(&dir.path().join("ready"));
        signal(&child, name);
        assert_eq!(child.wait().unwrap().code(), Some(code), "{name}");
        assert_eq!(fs::read_to_string(dir.path().join("got")).unwrap().trim(), name);
    }
}

#[test]
fn int는_삼키고_자식에게_넘기지_않는다() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = spawn_script(&trap_script(dir.path())).spawn().unwrap();
    wait_for(&dir.path().join("ready"));
    signal(&child, "INT");
    thread::sleep(Duration::from_millis(200));
    assert!(child.try_wait().unwrap().is_none(), "kairos-run은 SIGINT로 끝나지 않는다");
    assert!(!dir.path().join("got").exists());
    signal(&child, "TERM");
    assert_eq!(child.wait().unwrap().code(), Some(6));
}

/// SIG_IGN으로 막았다면 자식이 물려받아 살아남아 3으로 끝난다.
#[test]
fn 자식의_sigint는_기본_동작으로_돌아간다() {
    let status = spawn_script("kill -INT $$; sleep 1; exit 3").status().unwrap();
    assert_eq!(status.code(), Some(1));
}

struct Scene {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: KairosHome,
}

fn scene(title: Option<String>) -> Scene {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue_with(&db, 1, |object| {
        if title.is_some() {
            object.text_head = title;
        }
    });
    write_session_token(&home, &format!("{SYNTHETIC_TOKEN}\n"), 0o600);
    let root = dir.path().join("tools");
    fs::create_dir(&root).unwrap();
    Scene { root, _dir: dir, home }
}

fn fake_run_binary(root: &Path) -> String {
    let path = root.join("fake kairos-run");
    let exe = std::env::current_exe().unwrap();
    executable(
        &path,
        &format!(
            "#!/bin/sh\nKAIROS_TEST_RUN_ID=\"$1\" exec '{}' --ignored --exact {DOUBLE} --nocapture --test-threads=1\n",
            exe.display()
        ),
    );
    path.to_str().unwrap().to_owned()
}

/// 받은 인자와 환경, --settings 파일의 권한과 내용을 파일에 남기는 가짜 claude.
fn fake_claude(root: &Path) -> PathBuf {
    let claude = root.join("fake-claude");
    let tokens: String = CLEARED_ENV
        .iter()
        .map(|key| format!("printf '%s=%s\\n' {key} \"${{{key}-unset}}\" >> \"{}\"\n", root.join("env").display()))
        .collect();
    let settings = format!(
        "prev=\n\
         for arg in \"$@\"; do\n\
         if [ \"$prev\" = --settings ]; then\n\
         stat -f '%Lp' \"$arg\" > \"{mode}\"; stat -f '%Lp' \"$(dirname \"$arg\")\" > \"{dir_mode}\"; cat \"$arg\" > \"{content}\"\n\
         fi\n\
         prev=$arg\n\
         done\n",
        mode = root.join("settings-mode").display(),
        dir_mode = root.join("settings-dir-mode").display(),
        content = root.join("settings").display(),
    );
    executable(
        &claude,
        &format!("#!/bin/sh\n{tokens}{settings}printf '%s\\0' \"$@\" > \"{}\"\n", root.join("argv").display()),
    );
    claude
}

/// Orca가 셸에 입력하는 것과 같은 문자열을 얻는다.
fn accepted_command(scene: &Scene, run_binary: &str) -> String {
    let db = Database::open(&scene.home).unwrap();
    let orca = FakeOrca::new();
    let (client, clock, ids) = (client(&orca), FixedClock(at(AT)), FixedId(RUN.to_owned()));
    SessionLauncher::new(&db, &client, &clock, &admit_all, run_binary).with_ids(&ids).accept(1).unwrap();
    assert!(!orca.flat().iter().any(|argument| argument.contains("큰따옴표")));
    let args = orca.args("terminal create");
    args[args.iter().position(|arg| arg == "--command").unwrap() + 1].clone()
}

fn shell(scene: &Scene, command: &str, claude: &Path) -> Command {
    let mut shell = Command::new("/bin/sh");
    shell.args(["-c", command]).env("KAIROS_TEST_HOME", scene.home.root()).env("KAIROS_TEST_CLAUDE", claude);
    shell.env_remove("KAIROS_TEST_SPAWN");
    shell
}

fn state(scene: &Scene) -> (RunState, ItemState) {
    let db = Database::open(&scene.home).unwrap();
    (run(&db).state, item(&db, 1).state)
}

/// 원본: 끝까지: 쉼표, 따옴표, 셸 문자가 든 제목이 있어도 claude 인자에는 URL만 들어간다.
/// CLI 대신 라이브러리로 수락하고, 터미널 명령은 실제 셸로, 세션은 실제 /usr/bin/env로 돌린다.
#[test]
fn 끝까지_쉼표_따옴표_셸_문자가_든_제목이_있어도_claude_인자에는_url만_들어간다() {
    let dir = tempfile::tempdir().unwrap();
    let pwned = dir.path().join("pwned");
    let title = format!("합성, \"큰따옴표\" 'x' $(touch {0}); `touch {0}2` | z", pwned.display());
    let scene = scene(Some(title));
    let claude = fake_claude(&scene.root);
    let command = accepted_command(&scene, &fake_run_binary(&scene.root));

    let mut shell = shell(&scene, &command, &claude);
    for key in CLEARED_ENV {
        shell.env(key, "synthetic-token");
    }
    let output = shell.stdin(Stdio::null()).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let env = fs::read_to_string(scene.root.join("env")).unwrap();
    assert_eq!(env.lines().count(), CLEARED_ENV.len());
    assert!(
        env.lines().all(|line| line.ends_with("=unset") || line == format!("GH_TOKEN={SYNTHETIC_TOKEN}")),
        "env -u로 지우고 GH_TOKEN은 소유자 토큰으로 바꾼다: {env}"
    );
    let argv = fs::read_to_string(scene.root.join("argv")).unwrap();
    let args: Vec<&str> = argv.trim_end_matches('\0').split('\0').collect();
    assert!(!argv.contains("큰따옴표"));
    assert_eq!(*args.last().unwrap(), SessionProfile::prompt(Playbook::QaIssue, &issue_url(1)).unwrap());
    let expected = profile_at(&scene.home)
        .command(claude.to_str().unwrap(), RUN, Playbook::QaIssue, &issue_url(1), session_token())
        .unwrap();
    // /usr/bin/env가 -u와 claude 경로를 먹으므로 그 뒤의 값이 claude가 받은 인자다. GH_TOKEN은 -u에서 빠진다.
    assert_eq!(args, expected.args[(CLEARED_ENV.len() - 1) * 2 + 1..]);
    assert!(!pwned.exists());
    assert!(!dir.path().join("pwned2").exists());
    assert_eq!(state(&scene).0, RunState::Ended);
}

/// 소유자가 둔 토큰은 실제 /usr/bin/env를 지나 claude의 GH_TOKEN으로만 닿는다. 다른 토큰은 그대로 지운다.
#[test]
fn 끝까지_토큰_파일의_값은_claude_환경으로만_가고_인자에는_없다() {
    let scene = scene(None);
    let token = SYNTHETIC_TOKEN;
    let claude = fake_claude(&scene.root);
    let command = accepted_command(&scene, &fake_run_binary(&scene.root));

    let mut shell = shell(&scene, &command, &claude);
    for key in CLEARED_ENV {
        shell.env(key, "synthetic-start-token");
    }
    let output = shell.stdin(Stdio::null()).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let env = fs::read_to_string(scene.root.join("env")).unwrap();
    assert_eq!(env.lines().count(), CLEARED_ENV.len());
    assert!(env.lines().any(|line| line == format!("GH_TOKEN={token}")), "{env}");
    assert!(
        env.lines().filter(|line| !line.starts_with("GH_TOKEN=")).all(|line| line.ends_with("=unset")),
        "시작 환경의 다른 토큰은 env -u로 지운다: {env}"
    );
    let argv = fs::read_to_string(scene.root.join("argv")).unwrap();
    assert!(!argv.contains("SYNTHETIC0123"), "토큰은 인자에 없다. 다른 사용자도 ps로 읽는다");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC0123"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC0123"));
    // claude가 본 --settings 파일은 0700 폴더의 0600 파일이고 GH_TOKEN을 읽기 전용 토큰으로 고정한다.
    assert_eq!(fs::read_to_string(scene.root.join("settings-mode")).unwrap().trim(), "600");
    assert_eq!(fs::read_to_string(scene.root.join("settings-dir-mode")).unwrap().trim(), "700");
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(scene.root.join("settings")).unwrap()).unwrap();
    assert_eq!(settings["env"]["GH_TOKEN"], token);
    let path = profile_at(&scene.home).settings_path(RUN);
    assert!(argv.split('\0').any(|arg| Path::new(arg) == path), "--settings는 데이터 폴더의 파일을 가리킨다");
    assert!(!path.exists(), "claude가 끝나면 지운다");
    assert_eq!(state(&scene).0, RunState::Ended);
}

/// 같은 자리에 미리 둔 링크를 따라 쓰지 않는다. 쓰지 못하면 claude를 띄우지 않고 run은 실패로 남는다.
#[test]
fn 끝까지_settings_자리에_링크가_있으면_띄우지_않고_링크_끝을_건드리지_않는다() {
    let scene = scene(None);
    let claude = fake_claude(&scene.root);
    let command = accepted_command(&scene, &fake_run_binary(&scene.root));
    let path = profile_at(&scene.home).settings_path(RUN);
    fs::create_dir(path.parent().unwrap()).unwrap();
    let target = scene.root.join("planted");
    fs::write(&target, "synthetic-planted").unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();

    let output = shell(&scene, &command, &claude).stdin(Stdio::null()).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!scene.root.join("argv").exists(), "claude를 띄우지 않는다");
    assert_eq!(fs::read_to_string(&target).unwrap(), "synthetic-planted");
    assert!(fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC0123"));
    let db = Database::open(&scene.home).unwrap();
    assert_eq!((run(&db).state, run(&db).fail_code.as_deref()), (RunState::Failed, Some("session_failed")));
}

/// claude가 없어 env가 127로 끝나도 쓴 파일을 남기지 않는다.
#[test]
fn claude가_없어_실패해도_settings_파일을_지운다() {
    let scene = scene(None);
    accepted_command(&scene, "/synthetic/kairos-run");
    let status = double()
        .env("KAIROS_TEST_HOME", scene.home.root())
        .env("KAIROS_TEST_CLAUDE", "/nonexistent/synthetic-claude")
        .env("KAIROS_TEST_RUN_ID", RUN)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    let dir = profile_at(&scene.home).settings_path(RUN).parent().unwrap().to_owned();
    assert_eq!(fs::read_dir(dir).unwrap().count(), 0);
}

#[test]
fn 자식의_stdio는_터미널을_물려받는다() {
    let scene = scene(None);
    let claude = scene.root.join("echo-claude");
    executable(&claude, "#!/bin/sh\nread line\nprintf 'got:%s\\n' \"$line\"\nprintf 'err:%s\\n' \"$line\" >&2\n");
    let command = accepted_command(&scene, &fake_run_binary(&scene.root));
    let mut child = shell(&scene, &command, &claude)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all("합성 입력\n".as_bytes()).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("got:합성 입력"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("err:합성 입력"));
}

#[test]
fn 터미널이_닫혀도_신호를_넘기고_끝을_적는다() {
    let scene = scene(None);
    let claude = scene.root.join("trap-claude");
    executable(
        &claude,
        &format!(
            "#!/bin/sh\ntrap 'exit 0' HUP\necho > \"{}\"\nwhile :; do sleep 0.02; done\n",
            scene.root.join("ready").display()
        ),
    );
    // 셸을 거치지 않고 대역을 바로 띄워야 신호가 kairos-run에 간다.
    accepted_command(&scene, "/synthetic/kairos-run");
    let mut child = double()
        .env("KAIROS_TEST_HOME", scene.home.root())
        .env("KAIROS_TEST_CLAUDE", &claude)
        .env("KAIROS_TEST_RUN_ID", RUN)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_for(&scene.root.join("ready"));
    signal(&child, "HUP");
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert_eq!(state(&scene), (RunState::Ended, ItemState::Accepted));
}

#[test]
fn 없는_claude는_env가_127로_끝나_no_claude가_된다() {
    let scene = scene(None);
    accepted_command(&scene, "/synthetic/kairos-run");
    let status = double()
        .env("KAIROS_TEST_HOME", scene.home.root())
        .env("KAIROS_TEST_CLAUDE", "/nonexistent/synthetic-claude")
        .env("KAIROS_TEST_RUN_ID", RUN)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(1));
    let db = Database::open(&scene.home).unwrap();
    assert_eq!((run(&db).state, run(&db).fail_code.as_deref()), (RunState::Failed, Some("no_claude")));
}
