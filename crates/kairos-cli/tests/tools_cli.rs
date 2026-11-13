//! 설치 뒤 도구 명령(`uninstall --runtime|--purge`, `update`, `completions`)의 프로세스 시험.
//! launchctl은 언제나 `--no-launchctl`로 막는다. 실제 잡을 내리면 안 된다. lsregister와 security는 가짜다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use serde_json::json;

struct Installed {
    cli: Cli,
    /// 데이터 폴더. 홈 폴더(`cli.root`) 아래다. 홈 폴더 자체는 지우기를 거절한다.
    data: PathBuf,
    agents: PathBuf,
    log: PathBuf,
}

impl Installed {
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// 잡을 내리는 단계는 임시 폴더의 plist만 지운다. launchctl은 부르지 않는다.
    fn uninstall(&self, flags: &[&str], input: &[u8]) -> Done {
        let agents = self.agents.to_str().unwrap();
        let args = [&["uninstall", "--no-launchctl", "--launch-agents-dir", agents], flags].concat();
        self.cli.pipe(&args, input)
    }
}

/// 이 OS의 수집 잡 등록 파일. macOS는 plist, Linux는 timer, Windows는 작업 XML이다.
fn collect_job() -> String {
    kairos_ops::Scheduler::for_os(kairos_core::platform::Os::current()).file_names("dev.kairos.collect").remove(0)
}

/// 데이터를 지운 뒤 키체인 토큰을 지우는 마지막 호출. Linux는 secret-tool로 찾은 뒤 지운다.
fn token_delete_call() -> &'static str {
    match kairos_core::platform::Os::current() {
        kairos_core::platform::Os::Linux => "security clear service kairos account session-gh-token",
        _ => "security delete-generic-password -s kairos -a session-gh-token",
    }
}

/// 알림 헬퍼 번들을 LaunchServices에서 빼는 호출. macOS에만 있다.
fn notifier_calls(app: &Path) -> Vec<String> {
    if kairos_core::platform::Os::current() == kairos_core::platform::Os::MacOs {
        vec![format!("lsregister -u {}", app.display())]
    } else {
        Vec::new()
    }
}

/// 설치본 모양(`data/0.2.0`, `current`, `~/.local/bin/kairos`)과 수집 기록, 잡 파일을 둔다.
fn installed() -> Installed {
    let mut cli = Cli::new();
    let root = cli.root.clone();
    let data = root.join("data");
    let runtime = data.join("0.2.0");
    fs::create_dir_all(runtime.join("bin")).unwrap();
    for name in ["kairos", "kairos-run"] {
        fs::write(runtime.join("bin").join(name), "binary").unwrap();
    }
    fs::create_dir_all(runtime.join("ops/notifier/KairosNotifier.app")).unwrap();
    fs::create_dir_all(runtime.join("plugins/orca-panel")).unwrap();
    fs::create_dir_all(data.join("backup")).unwrap();
    fs::write(data.join("kairos.db"), "db").unwrap();
    symlink("0.2.0", data.join("current")).unwrap();
    fs::create_dir_all(root.join(".local/bin")).unwrap();
    symlink(data.join("current/bin/kairos"), root.join(".local/bin/kairos")).unwrap();
    let agents = root.join("agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(agents.join(collect_job()), "x").unwrap();
    let log = root.join("calls.log");
    let log_text = log.to_str().unwrap();
    for name in ["lsregister", "security"] {
        let script = root.join(name);
        write_script(&script, &format!("#!/bin/sh\nprintf '{name} %s\\n' \"$*\" >> '{log_text}'\n"));
        cli.set(&format!("KAIROS_{}", name.to_uppercase()), script.to_str().unwrap());
    }
    cli.set("KAIROS_HOME", data.to_str().unwrap());
    Installed { cli, data, agents, log }
}

#[test]
fn uninstall_runtime은_확인을_받고_실행본과_명령_링크만_지운다() {
    let setup = installed();
    let done = setup.uninstall(&["--runtime"], b"y\n");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("실행본을 지웁니다. 데이터 폴더는 남깁니다"), "{}", done.out);
    assert!(!setup.data.join("0.2.0").exists());
    assert!(fs::symlink_metadata(setup.data.join("current")).is_err());
    assert!(fs::symlink_metadata(setup.cli.root.join(".local/bin/kairos")).is_err());
    assert!(!setup.agents.join(collect_job()).exists());
    // 데이터는 남는다.
    assert!(setup.data.join("kairos.db").is_file() && setup.data.join("backup").is_dir());
    assert!(done.out.contains(&format!("데이터 폴더는 남겼습니다: {}", setup.data.display())), "{}", done.out);
    let panel = setup.data.join("0.2.0/plugins/orca-panel");
    // 임시 폴더 경로는 따옴표가 필요 없는 글자뿐이다.
    assert!(done.out.contains(&format!("orca plugin dev unload {}\n", panel.display())), "{}", done.out);
    let app = setup.data.join("0.2.0/ops/notifier/KairosNotifier.app");
    assert_eq!(setup.calls(), notifier_calls(&app));
}

#[test]
fn uninstall은_답이_없거나_아니오면_아무것도_바꾸지_않는다() {
    for (flag, input) in [("--runtime", &b""[..]), ("--runtime", b"n\n"), ("--purge", b"\n")] {
        let setup = installed();
        let done = setup.uninstall(&[flag], input);
        assert_eq!(done.status, 1, "{flag} {}{}", done.out, done.err);
        assert!(done.out.contains("아무것도 바꾸지 않았습니다"), "{}", done.out);
        assert!(setup.data.join("0.2.0/bin/kairos").is_file());
        assert!(setup.data.join("kairos.db").is_file());
        assert!(setup.agents.join(collect_job()).exists(), "잡도 내리지 않는다");
        assert!(setup.calls().is_empty(), "{:?}", setup.calls());
    }
}

#[test]
fn uninstall_dry_run은_지울_것만_보여_준다() {
    let setup = installed();
    let done = setup.uninstall(&["--purge", "--dry-run"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("지울 것(dry-run이라 지우지 않음)"), "{}", done.out);
    assert!(done.out.contains(&format!("  {}", setup.data.join("0.2.0").display())), "{}", done.out);
    assert!(done.out.contains("키체인의 세션 토큰"), "{}", done.out);
    assert!(setup.data.join("0.2.0/bin/kairos").is_file() && setup.data.join("kairos.db").is_file());
    assert!(setup.agents.join(collect_job()).exists());
    assert!(setup.calls().is_empty(), "{:?}", setup.calls());
}

#[test]
fn uninstall_purge는_데이터_폴더와_키체인_토큰까지_지운다() {
    let setup = installed();
    let done = setup.uninstall(&["--purge", "--yes"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(!setup.data.exists());
    assert!(setup.cli.root.is_dir(), "홈 폴더는 그대로다");
    assert!(done.out.contains(&format!("지움 {}", setup.data.display())), "{}", done.out);
    assert_eq!(setup.calls().last().unwrap(), token_delete_call());
}

#[test]
fn uninstall_purge는_홈_폴더와_표지_없는_폴더를_거절한다() {
    let mut setup = installed();
    // 홈 폴더가 데이터 폴더다(시험 하네스의 기본).
    let user_home = setup.cli.root.clone();
    setup.cli.set("KAIROS_HOME", user_home.to_str().unwrap());
    let done = setup.uninstall(&["--purge", "--yes"], b"");
    assert_eq!(done.status, 1, "{}{}", done.out, done.err);
    assert!(done.out.contains("지우지 않음: 데이터 폴더가 홈 폴더이거나 그 위입니다"), "{}", done.out);
    let plain = setup.cli.root.join("documents");
    fs::create_dir_all(&plain).unwrap();
    setup.cli.set("KAIROS_HOME", plain.to_str().unwrap());
    let done = setup.uninstall(&["--purge", "--yes"], b"");
    assert_eq!(done.status, 1, "{}{}", done.out, done.err);
    assert!(done.out.contains("kairos 데이터 폴더로 보이지 않습니다"), "{}", done.out);
    assert!(plain.is_dir() && setup.data.join("kairos.db").is_file());
    assert!(setup.agents.join(collect_job()).exists());
}

#[test]
fn uninstall_runtime은_홈_폴더를_데이터_폴더로_주면_지우지_않는다() {
    let mut setup = installed();
    // ~/.local/bin/kairos 링크가 있어도 ~/.local을 실행본으로 보지 않는다.
    let user_home = setup.cli.root.clone();
    setup.cli.set("KAIROS_HOME", user_home.to_str().unwrap());
    let done = setup.uninstall(&["--runtime", "--yes"], b"");
    assert_eq!(done.status, 1, "{}{}", done.out, done.err);
    assert!(done.out.contains("지우지 않음: 데이터 폴더가 홈 폴더이거나 그 위입니다"), "{}", done.out);
    assert!(fs::symlink_metadata(user_home.join(".local/bin/kairos")).is_ok());
    assert!(setup.data.join("0.2.0/bin/kairos").is_file());
    assert!(setup.agents.join(collect_job()).exists(), "잡도 내리지 않는다");
    // 데이터 폴더를 가리키는 링크에 `/`를 붙여도 거절한다.
    let alias = user_home.join("alias");
    symlink(&setup.data, &alias).unwrap();
    setup.cli.set("KAIROS_HOME", &format!("{}/", alias.display()));
    for flag in ["--runtime", "--purge"] {
        let done = setup.uninstall(&[flag, "--yes"], b"");
        assert_eq!(done.status, 1, "{flag} {}{}", done.out, done.err);
        assert!(done.out.contains("지우지 않음: 데이터 폴더가 링크입니다"), "{}", done.out);
    }
    assert!(setup.data.join("0.2.0/bin/kairos").is_file() && setup.data.join("kairos.db").is_file());
}

#[test]
fn uninstall은_세션_안에서_실행본을_지우지_않는다() {
    let setup = installed();
    let agents = setup.agents.to_str().unwrap();
    let args = ["uninstall", "--no-launchctl", "--launch-agents-dir", agents, "--runtime", "--yes"];
    let done = setup.cli.run_with(&[("KAIROS_SESSION", "1")], &args);
    assert_eq!(done.status, 1, "{}{}", done.out, done.err);
    assert!(setup.data.join("0.2.0/bin/kairos").is_file());
    // dry-run은 목록만 보여 주므로 된다.
    let dry = [&args[..], &["--dry-run"]].concat();
    let done = setup.cli.run_with(&[("KAIROS_SESSION", "1")], &dry);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("지울 것(dry-run이라 지우지 않음)"), "{}", done.out);
    assert!(setup.data.join("0.2.0/bin/kairos").is_file());
}

#[test]
fn 설정이_깨져도_지우기와_도구_명령은_돈다() {
    let setup = installed();
    fs::write(setup.data.join("config.toml"), "[schedule\n").unwrap();
    // 깨진 설정은 다른 명령을 막는다.
    assert_eq!(setup.cli.run(&["list"]).status, 1);
    let update = setup.cli.run(&["update"]);
    assert_eq!((update.status, update.err.as_str()), (0, ""), "{}", update.out);
    let completions = setup.cli.run(&["completions", "zsh"]);
    assert_eq!((completions.status, completions.err.as_str()), (0, ""));
    assert!(completions.out.starts_with("#compdef kairos\n"), "{}", completions.out);
    let done = setup.uninstall(&["--purge", "--yes"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(!setup.data.exists());
}

#[test]
fn uninstall은_runtime과_purge를_함께_주면_purge다() {
    let setup = installed();
    let done = setup.uninstall(&["--runtime", "--purge"], b"y\n");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("(수집 기록, 설정, 백업, 로그 전체)"), "{}", done.out);
    assert!(!setup.data.exists());
    // 데이터 폴더를 지운 뒤에 토큰을 지운다. 데이터를 지우다 멈추면 토큰은 남는다.
    let calls = setup.calls();
    assert_eq!(calls.last().unwrap(), token_delete_call());
}

#[test]
fn update는_설치_방식에_맞는_명령을_알려_주고_아무것도_바꾸지_않는다() {
    // OS마다 설치 스크립트와 릴리스 명령의 셸이 다르다. Windows는 이 시험을 돌리지 않는다(경로와 PowerShell 인용은 단위 시험이 본다).
    let (script, release_command) = if cfg!(target_os = "macos") {
        ("install.sh", "install-release.sh | zsh")
    } else {
        ("install-linux.sh", "install-release-linux.sh | sh")
    };
    if cfg!(windows) {
        return;
    }
    let cli = Cli::new();
    let unknown = cli.run(&["update"]);
    assert_eq!(unknown.status, 0, "{}{}", unknown.out, unknown.err);
    assert!(unknown.out.contains(&format!("지금 판: kairos {}", env!("CARGO_PKG_VERSION"))), "{}", unknown.out);
    assert!(unknown.out.contains("설치 방식: 기록 없음"), "{}", unknown.out);
    assert!(unknown.out.contains(&format!("scripts/{script} --update")), "{}", unknown.out);
    // 알려 줄 명령이 없으면 키째 뺀다.
    assert_eq!(
        cli.json(&["update"]).body()["update"],
        json!({ "version": env!("CARGO_PKG_VERSION"), "method": "unknown" })
    );

    let repo = cli.root.join("my repo");
    fs::create_dir_all(repo.join("scripts")).unwrap();
    fs::write(cli.root.join("install-source"), format!("source:{}\n", repo.display())).unwrap();
    // 저장소가 있지만 스크립트가 없다.
    assert!(cli.run(&["update"]).out.contains("저장소가 그 자리에 없습니다"));
    fs::write(repo.join("scripts").join(script), "#!/bin/sh\n").unwrap();
    let source = cli.run(&["update"]);
    let command = format!("'{}/scripts/{script}' --update", repo.display());
    assert!(source.out.contains(&format!("\n  {command}\n")), "{}", source.out);
    assert_eq!(
        cli.json(&["update"]).body()["update"],
        json!({ "version": env!("CARGO_PKG_VERSION"), "method": "source", "command": command })
    );

    fs::write(cli.root.join("install-source"), "release\n").unwrap();
    let release = cli.run(&["update"]);
    assert!(release.out.contains("설치 방식: 릴리스"), "{}", release.out);
    assert!(release.out.contains(release_command), "{}", release.out);
    let body = cli.json(&["update"]).body();
    assert_eq!(body["update"]["method"], "release");
    assert!(body["update"]["command"].as_str().unwrap().ends_with(release_command), "{body}");
    // 셸이 해석할 글자가 든 저장소 경로는 명령으로 찍지 않는다.
    let odd_repo = cli.root.join("a&b");
    fs::create_dir_all(odd_repo.join("scripts")).unwrap();
    fs::write(odd_repo.join("scripts").join(script), "#!/bin/sh\n").unwrap();
    fs::write(cli.root.join("install-source"), format!("source:{}\n", odd_repo.display())).unwrap();
    let refused = cli.run(&["update"]);
    assert!(refused.out.contains("셸이 해석하는 글자가 있어 명령을 찍지 않습니다"), "{}", refused.out);
    assert!(!refused.out.contains("--update'"), "{}", refused.out);
    assert_eq!(cli.json(&["update"]).body()["update"].get("command"), None);
    // 모양이 틀린 기록은 찍지 않는다.
    fs::write(cli.root.join("install-source"), "source:relative\u{1b}[31m\n").unwrap();
    let odd = cli.run(&["update"]);
    assert!(odd.out.contains("설치 방식: 기록 없음") && !odd.out.contains('\u{1b}'), "{}", odd.out);
}

#[test]
fn completions는_셸이_읽을_수_있는_스크립트를_낸다() {
    let cli = Cli::new();
    for (shell, bin) in [("zsh", "/bin/zsh"), ("bash", "/bin/bash")] {
        // 셸이 없는 컴퓨터(zsh 없는 Linux)는 문법 검사를 건너뛴다.
        if !Path::new(bin).exists() {
            continue;
        }
        let done = cli.run(&["completions", shell]);
        assert_eq!((done.status, done.err.as_str()), (0, ""), "{shell}");
        assert!(done.out.contains("doctor") && !done.out.contains("plan-tick"), "{}", done.out);
        let file = cli.root.join(format!("completion.{shell}"));
        fs::write(&file, &done.out).unwrap();
        // 문법만 본다. 실행하지 않는다.
        let checked = Command::new(bin).arg("-n").arg(&file).output().unwrap();
        assert!(checked.status.success(), "{shell}: {}", String::from_utf8_lossy(&checked.stderr));
    }
    let bad = cli.run(&["completions", "fish"]);
    assert_eq!(bad.status, 2);
    assert!(bad.out.starts_with("사용법: kairos completions zsh|bash\n"), "{}", bad.out);
}

/// bash 완성 함수를 읽어 들인 뒤 낱말과 자리를 주고 후보를 받는다. macOS의 bash 3.2로 돌린다.
fn bash_candidates(script: &std::path::Path, words: &[&str]) -> String {
    let line =
        "eval \"$(cat \"$1\")\"; shift; COMP_WORDS=(\"$@\"); COMP_CWORD=$(($# - 1)); _kairos; echo \"${COMPREPLY[*]}\"";
    let output = Command::new("/bin/bash").arg("-c").arg(line).arg("bash").arg(script).args(words).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

#[test]
fn bash_완성은_명령_바로_뒤에서만_정해진_낱말을_낸다() {
    let cli = Cli::new();
    let script = cli.root.join("completion.bash");
    fs::write(&script, cli.run(&["completions", "bash"]).out).unwrap();
    assert_eq!(bash_candidates(&script, &["kairos", "doc"]), "doctor");
    assert_eq!(bash_candidates(&script, &["kairos", "he"]), "help");
    assert_eq!(bash_candidates(&script, &["kairos", "notify", ""]), "on off setup test");
    assert_eq!(bash_candidates(&script, &["kairos", "sources", "o"]), "on off");
    assert_eq!(bash_candidates(&script, &["kairos", "help", "upd"]), "update");
    // 그 뒤 자리는 소스 이름, ID, 파일이다. 후보를 내지 않고 bash의 기본 완성에 맡긴다.
    assert_eq!(bash_candidates(&script, &["kairos", "sources", "on", ""]), "");
    assert_eq!(bash_candidates(&script, &["kairos", "triage", "keep", ""]), "");
    assert_eq!(bash_candidates(&script, &["kairos", "setup", "--org", "pro"]), "");
    // 옵션은 어느 자리에서나 낸다.
    assert_eq!(bash_candidates(&script, &["kairos", "uninstall", "--dry-run", "--pu"]), "--purge");
    // 등록은 기본 완성으로 넘어가게 한다.
    let registered = Command::new("/bin/bash")
        .arg("-c")
        .arg("eval \"$(cat \"$1\")\"; complete -p kairos")
        .arg("bash")
        .arg(&script)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&registered.stdout).trim(), "complete -o default -F _kairos kairos");
}
