//! 세션 끝까지 시험과 설치 시험(Rust 설치본 기준), 기본 경로 시험.
//! 수집 명령의 시험 환경 시험.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use common::*;
use kairos_core::KairosHome;
#[cfg(target_os = "macos")]
use kairos_core::model::SourceName;
use kairos_ops::VERSION;
use kairos_tick::permission_problems;
use serde_json::json;

const RUN_ID: &str = "11111111-2222-4333-8444-555555555555";

/// 받은 인자와 토큰을 적는다.
#[cfg(target_os = "macos")]
fn fake_claude(cli: &Cli) -> (String, std::path::PathBuf) {
    let claude = cli.root.join("fake-claude");
    let dump = cli.root.join("claude-argv");
    let script = format!(
        "#!/bin/sh\nprintf '%s' \"${{GITHUB_PERSONAL_ACCESS_TOKEN-}}\" > '{dump}.token'\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\"; done > '{dump}'\n",
        dump = dump.display()
    );
    write_script(&claude, &script);
    (claude.to_str().unwrap().to_owned(), dump)
}

/// 세션을 여는 흐름이다. 세션은 macOS에서만 연다.
#[cfg(target_os = "macos")]
#[test]
fn 끝까지_쉼표_따옴표_셸_문자가_든_제목이_있어도_claude_인자에는_url만_들어간다() {
    let mut cli = Cli::new();
    let pwned = cli.root.join("pwned");
    let title =
        format!("합성, \"큰따옴표\" 'x' $(touch {home}/pwned); `touch {home}/pwned2` | z", home = cli.root.display());
    seed_issue(&cli.db(), Some(&title));
    cli.write_session_token();
    cli.open_sessions();
    let orca = cli.fake_orca();
    let gh_log = cli.fake_gh();
    let bin = cli.install_binaries();
    let accepted = cli.run_installed(&bin, &["accept", "1", "--json"]);
    assert_eq!(accepted.status, 0, "{}", accepted.err);
    assert!(!orca.raw().contains("큰따옴표"));
    let calls = orca.calls();
    let create = calls.iter().find(|call| call.args[..2] == ["terminal", "create"]).unwrap();
    let index = create.args.iter().position(|arg| arg == "--command").unwrap();
    let command = &create.args[index + 1];

    let (claude, dump) = fake_claude(&cli);
    // Orca가 셸에 입력하는 것과 같은 문자열을 실제 셸로 돌린다.
    let shell = Command::new("/bin/sh")
        .args(["-c", command])
        .env_clear()
        .envs(&cli.env)
        .env("KAIROS_CLAUDE", &claude)
        .env("GITHUB_PERSONAL_ACCESS_TOKEN", "synthetic-token")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(shell.status.code(), Some(0), "{}", String::from_utf8_lossy(&shell.stderr));
    let argv = fs::read_to_string(&dump).unwrap();
    assert_eq!(fs::read_to_string(dump.with_extension("token")).unwrap(), "", "env -u로 지운다");
    assert!(!argv.contains("큰따옴표"));
    assert!(argv.contains(&issue_url(1)));
    assert!(!pwned.exists());
    assert!(!cli.root.join("pwned2").exists());
    let gh_calls = fs::read_to_string(gh_log).unwrap();
    assert_eq!(gh_calls, "auth token --user kairos-dev\napi graphql --input -\n", "띄우기 직전에 글을 다시 읽는다");
}

/// 세션을 여는 흐름이다. 세션은 macOS에서만 연다.
#[cfg(target_os = "macos")]
#[test]
fn kairos_run은_gh로_글을_다시_읽지_못하면_claude를_띄우지_않는다() {
    let mut cli = cli_with_issue();
    cli.fake_orca();
    let bin = cli.install_binaries();
    let accepted = cli.run_installed(&bin, &["accept", "1", "--json"]).body();
    let run_id = accepted["run"]["id"].as_str().unwrap().to_owned();
    let (claude, dump) = fake_claude(&cli);
    cli.write_session_token();
    // KAIROS_GH는 없는 파일을 가리킨다.
    let output = Command::new(bin.join("kairos-run"))
        .arg(&run_id)
        .env_clear()
        .envs(&cli.env)
        .env("KAIROS_CLAUDE", &claude)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "kairos-run: 이슈의 글을 모두 확인하지 못해 세션을 열지 않았습니다 (live_gh_missing)\n"
    );
    assert!(!dump.exists());
}

/// 저장소의 ops 폴더와 작은 플러그인 폴더로 만든 설치 자료.
/// 저장소의 수집 템플릿은 아직 node 줄이 있다. 전환 때 지운다. kairos-ops의 시험과 같이 그 줄만 뺀다.
fn asset_root(cli: &Cli) -> std::path::PathBuf {
    let root = cli.root.join("assets");
    fs::create_dir_all(root.join("ops")).unwrap();
    fs::create_dir_all(root.join("plugins/collector")).unwrap();
    fs::write(root.join("plugins/collector/hook.txt"), "합성").unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ops");
    // 알림 헬퍼 폴더(ops/notifier)는 이 시험의 대상이 아니다. 템플릿과 감시자만 옮긴다. systemd와 Windows 템플릿은 하위 폴더에 있다.
    for folder in ["", "systemd", "windows"] {
        fs::create_dir_all(root.join("ops").join(folder)).unwrap();
        for entry in fs::read_dir(repo.join(folder)).unwrap() {
            let entry = entry.unwrap();
            if !entry.file_type().unwrap().is_file() {
                continue;
            }
            let text = fs::read_to_string(entry.path()).unwrap();
            let kept: Vec<&str> = text.lines().filter(|line| !line.contains("{{NODE}}")).collect();
            fs::write(root.join("ops").join(folder).join(entry.file_name()), format!("{}\n", kept.join("\n"))).unwrap();
        }
    }
    root
}

/// 이 OS의 스케줄러가 쓰는 파일 이름. 첫째가 등록 파일이다.
fn job_files(label: &str) -> Vec<String> {
    kairos_ops::Scheduler::for_os(kairos_core::platform::Os::current()).file_names(label)
}

/// 수집 잡의 실행 파일과 claude가 적힌 파일. systemd는 서비스 유닛이다.
fn collect_job(agents: &Path) -> String {
    let names = job_files("dev.kairos.collect");
    fs::read_to_string(agents.join(names.last().unwrap())).unwrap()
}

fn fake_tool(cli: &Cli, name: &str) -> String {
    let path = cli.root.join(name);
    write_script(&path, "#!/bin/sh\nexit 0\n");
    path.to_str().unwrap().to_owned()
}

#[test]
fn install은_kairos_run을_실행_권한으로_복사한다() {
    let cli = Cli::new();
    let (claude, gh) = (fake_tool(&cli, "claude"), fake_tool(&cli, "gh"));
    let assets = asset_root(&cli);
    let agents = cli.root.join("agents");
    let bin = cli.install_binaries();
    let done = cli.run_installed(
        &bin,
        &[
            "install",
            "--apply",
            "--claude",
            &claude,
            "--gh",
            &gh,
            "--root",
            assets.to_str().unwrap(),
            "--launch-agents-dir",
            agents.to_str().unwrap(),
        ],
    );
    assert_eq!(done.status, 0, "{} {}", done.out, done.err);
    let runtime = cli.root.join(VERSION);
    let installed = runtime.join("bin").join("kairos-run");
    assert_eq!(fs::metadata(&installed).unwrap().permissions().mode() & 0o777, 0o755);
    assert_eq!(fs::read(&installed).unwrap(), fs::read(bin.join("kairos-run")).unwrap());
    assert_eq!(fs::metadata(runtime.join("bin/kairos")).unwrap().permissions().mode() & 0o777, 0o755);
    assert!(runtime.join("plugins/collector/hook.txt").exists());
    assert!(runtime.join("ops/kairos-watchdog.zsh").exists());
    for name in job_files("dev.kairos.collect") {
        assert!(agents.join(&name).exists(), "{name}");
    }
    let job = collect_job(&agents);
    assert!(job.contains(runtime.join("bin/kairos").to_str().unwrap()), "{job}");
    assert!(job.contains(&claude), "{job}");
}

#[test]
fn install_dry_run은_저장소_폴더에서_실행하면_그_자료를_쓰고_아무것도_쓰지_않는다() {
    let cli = Cli::new();
    let (claude, gh) = (fake_tool(&cli, "claude"), fake_tool(&cli, "gh"));
    let agents = cli.root.join("agents");
    let assets = asset_root(&cli);
    let nested = assets.join("plugins/collector");
    let output = Command::new(BIN)
        .args(["install", "--claude", &claude, "--gh", &gh, "--launch-agents-dir", agents.to_str().unwrap()])
        .env_clear()
        .envs(&cli.env)
        .current_dir(nested)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert_eq!(output.status.code(), Some(0), "{text} {}", String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("모드: 시험(dry-run). 아무것도 쓰지 않음"), "{text}");
    assert!(!agents.exists());
    assert!(!cli.root.join(VERSION).exists());
}

/// 데이터 폴더를 따로 둔 설치 준비. 임시 폴더의 다른 파일이 권한 검사에 섞이지 않는다.
fn data_home(cli: &mut Cli) -> std::path::PathBuf {
    let data = cli.root.join("data");
    cli.set("KAIROS_HOME", data.to_str().unwrap());
    data
}

#[test]
fn install_apply는_로그_파일을_0600으로_미리_만든다() {
    let mut cli = Cli::new();
    let (claude, gh) = (fake_tool(&cli, "claude"), fake_tool(&cli, "gh"));
    let assets = asset_root(&cli);
    let data = data_home(&mut cli);
    let agents = cli.root.join("agents");
    let bin = cli.install_binaries();
    let done = cli.run_installed(
        &bin,
        &[
            "install",
            "--apply",
            "--claude",
            &claude,
            "--gh",
            &gh,
            "--root",
            assets.to_str().unwrap(),
            "--launch-agents-dir",
            agents.to_str().unwrap(),
        ],
    );
    assert_eq!(done.status, 0, "{}", done.err);
    assert_eq!(permission_problems(&KairosHome::at(&data)).unwrap(), Vec::<String>::new());
    let mut logs: Vec<String> = fs::read_dir(data.join("logs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    logs.sort();
    assert_eq!(logs, ["collect.log", "watchdog.log"]);
}

#[test]
fn install_dry_run_미리보기는_지정한_폴더에만_plist를_쓴다() {
    let mut cli = Cli::new();
    let (claude, gh) = (fake_tool(&cli, "claude"), fake_tool(&cli, "gh"));
    let assets = asset_root(&cli);
    let data = data_home(&mut cli);
    let (agents, preview) = (cli.root.join("agents"), cli.root.join("preview"));
    let done = cli.run(&[
        "install",
        "--dry-run",
        "--claude",
        &claude,
        "--gh",
        &gh,
        "--root",
        assets.to_str().unwrap(),
        "--launch-agents-dir",
        agents.to_str().unwrap(),
        "--preview-dir",
        preview.to_str().unwrap(),
    ]);
    assert_eq!(done.status, 0, "{}", done.err);
    assert!(done.out.contains("모드: 시험(dry-run). 아무것도 쓰지 않음"));
    let mut names: Vec<String> =
        fs::read_dir(&preview).unwrap().map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    let mut expected: Vec<String> =
        ["dev.kairos.collect", "dev.kairos.watchdog"].into_iter().flat_map(job_files).collect();
    expected.sort();
    assert_eq!(names, expected);
    assert!(!agents.exists());
    assert!(!data.exists());
}

#[test]
fn uninstall_dry_run은_지정한_폴더만_보고_문구를_낸다() {
    let cli = Cli::new();
    let agents = cli.root.join("agents");
    let done = cli.run(&["uninstall", "--dry-run", "--launch-agents-dir", agents.to_str().unwrap()]);
    assert_eq!(done.status, 0);
    assert!(done.out.contains("시험(dry-run). 아무것도 바꾸지 않음"));
    assert!(done.out.contains("orca plugin dev unload"));
}

#[test]
fn kairos_home이_기본_경로를_바꾼다() {
    let mut cli = Cli::new();
    let custom = cli.root.join("custom-home");
    cli.set("KAIROS_HOME", custom.to_str().unwrap());
    assert!(cli.run(&["doctor"]).out.contains(&format!("데이터 폴더 {} ", custom.display())));
    cli.env.remove("KAIROS_HOME");
    let default = kairos_core::platform::Os::current().default_data_dir(&cli.root);
    assert!(cli.run(&["doctor"]).out.contains(&format!("데이터 폴더 {} ", default.display())));
    assert!(default.join("kairos.db").exists());
}

#[test]
fn 시험_환경에_운영_값이_남아_있지_않다() {
    let cli = Cli::new();
    for key in ["KAIROS_CLAUDE", "KAIROS_GH", "KAIROS_ORCA", "KAIROS_OPEN", "KAIROS_NOTIFIER", "KAIROS_HOME", "HOME"] {
        assert!(cli.env[key].starts_with(cli.root.to_str().unwrap()), "{key}");
    }
    assert!(!cli.env.contains_key("KAIROS_PLUGIN_DIR"));
    assert_eq!(cli.env["PATH"], "/usr/bin:/bin");
}

/// 세션을 여는 흐름이다. 세션은 macOS에서만 연다.
#[cfg(target_os = "macos")]
#[test]
fn kairos_run은_argv가_틀리면_db를_열기_전에_2로_끝난다() {
    let cli = Cli::new();
    let output = Command::new(RUN_BIN).arg("not-a-uuid").env_clear().envs(&cli.env).output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&output.stderr), "kairos-run: run ID 형식이 아닙니다\n");
    assert!(!cli.home().db_path().exists());
    let none = Command::new(RUN_BIN).env_clear().envs(&cli.env).output().unwrap();
    assert_eq!(none.status.code(), Some(2));
}

/// 세션을 여는 흐름이다. 세션은 macOS에서만 연다.
#[cfg(target_os = "macos")]
#[test]
fn kairos_run은_없는_run을_거절하고_claude를_띄우지_않는다() {
    let cli = Cli::new();
    prime(&cli.db(), SourceName::Slack, BEFORE);
    cli.write_session_token();
    let (claude, dump) = fake_claude(&cli);
    let output =
        Command::new(RUN_BIN).arg(RUN_ID).env_clear().envs(&cli.env).env("KAIROS_CLAUDE", &claude).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stderr), "kairos-run: 없는 run입니다\n");
    assert!(!dump.exists());
}

/// 토큰이 없을 때 소유자의 키체인 gh 로그인으로 내려가지 않는다. 고정 문구로 거절하고 claude를 띄우지 않는다.
/// 세션을 여는 흐름이다. 세션은 macOS에서만 연다.
#[cfg(target_os = "macos")]
#[test]
fn kairos_run은_읽기_전용_토큰이_없으면_claude를_띄우지_않는다() {
    let mut cli = cli_with_issue();
    cli.fake_orca();
    let bin = cli.install_binaries();
    let accepted = cli.run_installed(&bin, &["accept", "1", "--json"]).body();
    let run_id = accepted["run"]["id"].as_str().unwrap().to_owned();
    let (claude, dump) = fake_claude(&cli);
    let output = Command::new(bin.join("kairos-run"))
        .arg(&run_id)
        .env_clear()
        .envs(&cli.env)
        .env("KAIROS_CLAUDE", &claude)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "kairos-run: 데이터 폴더에 읽기 전용 session-gh-token이 없어 세션을 시작하지 않습니다\n"
    );
    assert!(!dump.exists());
}

/// 세션을 여는 흐름이다. 세션은 macOS에서만 연다.
#[cfg(target_os = "macos")]
#[test]
fn 세션_거부_목록의_kairos_경로는_실제_설치_폴더다() {
    let mut cli = cli_with_issue();
    cli.fake_orca();
    cli.fake_gh();
    cli.write_session_token();
    let bin = cli.install_binaries();
    let accepted = cli.run_installed(&bin, &["accept", "1", "--json"]).body();
    let run_id = accepted["run"]["id"].as_str().unwrap().to_owned();
    let (claude, dump) = fake_claude(&cli);
    let status = Command::new(bin.join("kairos-run"))
        .arg(&run_id)
        .env_clear()
        .envs(&cli.env)
        .env("KAIROS_CLAUDE", &claude)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    let argv = fs::read_to_string(dump).unwrap();
    let kairos = bin.join("kairos");
    assert!(argv.contains(&format!("Bash({}:*)", kairos.display())), "{argv}");
    assert!(argv.contains(&format!("Bash({}:*)", bin.join("kairos-run").display())));
    assert_eq!(cli.state()["runs"], json!({ "active": 1, "ready": 0 }), "끝을 적어도 state는 다음 명령이 쓴다");
}

/// macOS가 아니면 세션을 어느 길로도 열지 않는다. 수락, `sessions on`, kairos-run 모두 거절한다.
#[cfg(not(target_os = "macos"))]
#[test]
fn macos가_아니면_수락과_sessions_on과_kairos_run이_세션을_거절한다() {
    let cli = Cli::new();
    let on = cli.json(&["sessions", "on"]);
    assert_eq!(on.status, 1);
    assert_eq!(on.body()["error"]["code"], json!("sessions_unsupported"));
    let accept = cli.json(&["accept", "1"]);
    assert_eq!(accept.status, 1);
    assert_eq!(accept.body()["error"]["code"], json!("sessions_unsupported"));
    let cancel = cli.json(&["accept", "1", "--cancel"]);
    assert_ne!(cancel.body()["error"]["code"], json!("sessions_unsupported"), "되돌리기는 막지 않는다");
    let run = Command::new(RUN_BIN).arg(RUN_ID).env_clear().envs(&cli.env).output().unwrap();
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("세션 경계를 지키는 샌드박스가 없어"));
}
