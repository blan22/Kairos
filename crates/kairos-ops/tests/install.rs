//! 설치. 시험은 크레이트 안의 합성 템플릿만 쓴다. launchctl은 가짜다
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::TestHome;
use kairos_core::process::{ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_ops::{
    InstallMode, InstallOptions, InstallPaths, InstallPlan, Installer, PlanInput, UninstallOptions, VERSION,
    plan_install,
};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/install-root");
const REAL_OPS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ops");

type Reply = dyn Fn(&[String]) -> i32 + Send + Sync;

/// launchctl의 가짜. 호출을 적고 코드를 돌려준다. 다른 프로그램은 uid 조회만 받는다.
struct FakeLaunchctl {
    calls: Mutex<Vec<Vec<String>>>,
    reply: Box<Reply>,
}

impl FakeLaunchctl {
    fn replying(reply: impl Fn(&[String]) -> i32 + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self { calls: Mutex::new(Vec::new()), reply: Box::new(reply) })
    }

    fn ok() -> Arc<Self> {
        Self::replying(|_| 0)
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl ProcessRunner for FakeLaunchctl {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        if request.program == "/usr/bin/id" {
            return ProcessOutput::exited(0, "501\n");
        }
        assert_eq!(request.program, "/bin/launchctl");
        self.calls.lock().unwrap().push(request.args.clone());
        ProcessOutput::exited((self.reply)(&request.args), "")
    }
}

fn installer(fake: &Arc<FakeLaunchctl>) -> Installer {
    Installer::new(fake.clone())
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn paths() -> InstallPaths {
    InstallPaths { claude: "/opt/c/bin/claude".into(), gh: "/opt/homebrew/bin/gh".into() }
}

fn plan(test: &TestHome, paths: &InstallPaths) -> Result<InstallPlan, kairos_core::KairosError> {
    plan_install(&PlanInput {
        home: &test.home,
        root: Path::new(ROOT),
        paths,
        launch_agents_dir: Some(test.join("agents")),
    })
}

/// 설치할 합성 실행 파일과 도구를 만든다.
struct Setup {
    test: TestHome,
    agents: PathBuf,
    options: InstallOptions,
}

impl Setup {
    fn new(install_mode: InstallMode) -> Self {
        let test = TestHome::new();
        let tools = test.join("tools");
        fs::create_dir(&tools).unwrap();
        for name in ["kairos", "kairos-run"] {
            fs::write(tools.join(name), format!("합성 {name}")).unwrap();
        }
        let bin = test.join("opt/c/bin");
        fs::create_dir_all(&bin).unwrap();
        for name in ["claude", "gh"] {
            fs::write(bin.join(name), "#!/bin/sh\n").unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let agents = test.join("agents");
        let options = InstallOptions {
            mode: install_mode,
            launch_agents_dir: Some(agents.clone()),
            claude: Some(bin.join("claude").to_string_lossy().into_owned()),
            gh: Some(bin.join("gh").to_string_lossy().into_owned()),
            binary_dir: Some(tools),
            bootstrap_retry: Duration::ZERO,
            ..InstallOptions::new(test.home.clone(), ROOT)
        };
        Self { test, agents, options }
    }
}

fn minutes(content: &str) -> usize {
    content.matches("<key>Minute</key>").count()
}

#[test]
fn install_기본값은_dry_run이라_아무것도_쓰지_않는다() {
    let setup = Setup::new(InstallMode::DryRun);
    let fake = FakeLaunchctl::ok();
    let dry = installer(&fake).install(&setup.options).unwrap();
    assert_eq!(InstallOptions::new(setup.test.home.clone(), ROOT).mode, InstallMode::DryRun);
    assert_eq!(dry.mode, InstallMode::DryRun);
    assert!(!setup.agents.exists());
    assert!(!dry.plan.runtime_dir.exists());
    assert!(!dry.plan.log_dir.exists());
    assert!(fake.calls().is_empty());
}

#[test]
fn plist_절대_경로_3분과_5분_간격_shim_없음_자리표시자_없음() {
    let test = TestHome::new();
    let plan = plan(&test, &paths()).unwrap();
    let [collect, watchdog] = &plan.plists[..] else { panic!("plist 둘") };
    assert_eq!(collect.label, "dev.kairos.collect");
    assert_eq!(watchdog.label, "dev.kairos.watchdog");
    for plist in &plan.plists {
        assert!(!plist.content.contains("{{"));
        for shim in [".volta/bin", ".cargo/bin", "shims"] {
            assert!(!plist.content.contains(shim));
        }
        assert!(!plist.content.contains("StartInterval"));
    }
    assert_eq!(minutes(&collect.content), 20);
    assert_eq!(minutes(&watchdog.content), 12);
    let kairos_bin = plan.kairos_bin.display();
    assert!(
        collect.content.contains(&format!("<array>\n    <string>{kairos_bin}</string>\n    <string>collect</string>"))
    );
    assert!(collect.content.contains("/opt/c/bin:"));
    assert!(watchdog.content.contains("<string>/bin/zsh</string>"));
    assert!(watchdog.content.contains("<string>-f</string>"));
    assert_eq!(plan.runtime_dir, test.join(VERSION));
    assert_eq!(VERSION, "0.3.0");
}

#[test]
fn plist는_umask_077을_적어_launchd_기본값에_기대지_않는다() {
    let test = TestHome::new();
    let umask = regex::Regex::new(r"<key>Umask</key>\s*<integer>63</integer>").unwrap();
    for plist in plan(&test, &paths()).unwrap().plists {
        assert!(umask.is_match(&plist.content));
    }
}

#[test]
fn 상대_경로_도구는_계획이_거부한다() {
    let test = TestHome::new();
    let failure = plan(&test, &InstallPaths { gh: "gh".into(), ..paths() }).unwrap_err();
    assert_eq!(failure.code(), "install paths must be absolute");
}

#[test]
fn install_apply는_실행본과_유효한_plist만_만들고_launchctl을_부르지_않는다() {
    let setup = Setup::new(InstallMode::Apply);
    let fake = FakeLaunchctl::ok();
    let applied = installer(&fake).install(&setup.options).unwrap();
    assert!(fake.calls().is_empty());
    let runtime = &applied.plan.runtime_dir;
    assert!(runtime.join("bin/kairos").exists());
    assert!(runtime.join("ops/kairos-watchdog.zsh").exists());
    assert!(!runtime.join("test").exists());
    assert_eq!(mode(&applied.plan.log_dir), 0o700);
    for plist in &applied.plan.plists {
        let lint = Command::new("/usr/bin/plutil").arg("-lint").arg(&plist.path).output().unwrap();
        assert!(lint.status.success(), "{}", String::from_utf8_lossy(&lint.stdout));
    }
}

#[test]
fn bootout이_끝나기_전의_코드_5는_다시_시도하고_끝내_실패하면_실패로_알린다() {
    let setup = Setup::new(InstallMode::Register);
    let left = Mutex::new(2);
    let flaky = FakeLaunchctl::replying(move |args| {
        let mut left = left.lock().unwrap();
        if args[0] == "bootstrap" && *left > 0 {
            *left -= 1;
            return 5;
        }
        0
    });
    assert!(!installer(&flaky).install(&setup.options).unwrap().failed);
    let broken = FakeLaunchctl::replying(|args| if args[0] == "bootstrap" { 5 } else { 0 });
    let failed = installer(&broken).install(&setup.options).unwrap();
    assert!(failed.failed);
    // 처음 한 번과 다시 시도 5번이 plist마다 있다.
    let bootstraps = broken.calls().iter().filter(|call| call[0] == "bootstrap").count();
    assert_eq!(bootstraps, 12);
    assert!(failed.lines.contains(&"dev.kairos.collect: 등록 실패(코드 5)".to_owned()));
}

#[test]
fn install_register만_launchctl_bootstrap을_부른다() {
    let setup = Setup::new(InstallMode::Register);
    let fake = FakeLaunchctl::ok();
    let registered = installer(&fake).install(&setup.options).unwrap();
    let calls = fake.calls();
    assert_eq!(
        calls.iter().map(|call| call[0].as_str()).collect::<Vec<_>>(),
        ["bootout", "bootstrap", "kickstart", "bootout", "bootstrap"]
    );
    assert_eq!(calls[0], ["bootout", "gui/501/dev.kairos.collect"]);
    assert_eq!(calls[1][1], "gui/501");
    assert_eq!(calls[1][2], registered.plan.plists[0].path.to_string_lossy());
}

#[test]
fn 설치본의_kairos로_같은_판을_다시_설치하면_복사하지_않고_다시_등록한다() {
    let setup = Setup::new(InstallMode::Apply);
    let unused = FakeLaunchctl::ok();
    let first = installer(&unused).install(&setup.options).unwrap();
    let runtime = first.plan.runtime_dir.clone();
    let binary = runtime.join("bin/kairos");
    let watchdog = runtime.join("ops/kairos-watchdog.zsh");
    let before = (fs::read(&binary).unwrap(), fs::read(&watchdog).unwrap());
    assert!(!before.1.is_empty());
    fs::remove_file(setup.agents.join("dev.kairos.collect.plist")).unwrap();

    // kairos setup이 설치본의 kairos로 돌면 설치 자료와 실행 파일이 모두 설치본 안에 있다.
    let again = InstallOptions {
        mode: InstallMode::Register,
        root: runtime.clone(),
        binary_dir: Some(runtime.join("bin")),
        ..setup.options.clone()
    };
    let fake = FakeLaunchctl::ok();
    let done = installer(&fake).install(&again).unwrap();
    assert!(!done.failed, "{:?}", done.lines);
    assert_eq!((fs::read(&binary).unwrap(), fs::read(&watchdog).unwrap()), before);
    assert_eq!(mode(&watchdog), 0o755);
    assert!(setup.agents.join("dev.kairos.collect.plist").is_file());
    assert_eq!(fake.calls().iter().filter(|call| call[0] == "kickstart").count(), 1);

    // 설치 자료만 설치본이고 실행 파일이 다른 곳이면 무엇이 놓일지 모르므로 예전처럼 거절한다.
    let mixed = InstallOptions { root: runtime.clone(), ..setup.options.clone() };
    let refused = installer(&FakeLaunchctl::ok()).install(&mixed).unwrap_err();
    assert_eq!(refused.code(), "install_same_root");
    assert_eq!((fs::read(&binary).unwrap(), fs::read(&watchdog).unwrap()), before);
}

#[test]
fn install_register는_수집_잡을_등록한_뒤_바로_한_번_돌린다() {
    let setup = Setup::new(InstallMode::Register);
    let fake = FakeLaunchctl::ok();
    let registered = installer(&fake).install(&setup.options).unwrap();
    // 수집 잡만 돌린다. 인자는 고정된 라벨뿐이고 감시자는 제 주기를 기다린다.
    let kicks: Vec<Vec<String>> = fake.calls().into_iter().filter(|call| call[0] == "kickstart").collect();
    assert_eq!(kicks, [["kickstart", "gui/501/dev.kairos.collect"]]);
    assert!(!registered.failed);
    let at = |line: &str| registered.lines.iter().position(|found| found == line).unwrap();
    assert!(at("dev.kairos.collect: 등록됨") < at("dev.kairos.collect: 첫 수집을 바로 시작함"));
    assert!(at("dev.kairos.collect: 첫 수집을 바로 시작함") < at("dev.kairos.watchdog: 등록됨"));
}

#[test]
fn 첫_수집을_시작하지_못해도_설치는_실패가_아니고_등록에_실패한_잡은_돌리지_않는다() {
    let setup = Setup::new(InstallMode::Register);
    let stuck = FakeLaunchctl::replying(|args| if args[0] == "kickstart" { 3 } else { 0 });
    let registered = installer(&stuck).install(&setup.options).unwrap();
    assert!(!registered.failed);
    let line = "dev.kairos.collect: 첫 수집을 바로 시작하지 못함(코드 3). 다음 틱에 수집합니다";
    assert!(registered.lines.contains(&line.to_owned()), "{:?}", registered.lines);

    let broken = Setup::new(InstallMode::Register);
    let fake = FakeLaunchctl::replying(|args| if args[0] == "bootstrap" { 5 } else { 0 });
    let failed = installer(&fake).install(&broken.options).unwrap();
    assert!(failed.failed);
    assert!(fake.calls().iter().all(|call| call[0] != "kickstart"));
}

#[test]
fn uninstall은_우리_라벨_둘만_내리고_dry_run은_아무것도_바꾸지_않는다() {
    let setup = Setup::new(InstallMode::Apply);
    let unused = FakeLaunchctl::ok();
    installer(&unused).install(&setup.options).unwrap();
    fs::write(setup.agents.join("other.plist"), "x").unwrap();
    let fake = FakeLaunchctl::ok();
    let dry = UninstallOptions {
        launch_agents_dir: Some(setup.agents.clone()),
        dry_run: true,
        skip_launchctl: false,
        before_removal: false,
    };
    installer(&fake).uninstall(&dry).unwrap();
    assert!(fake.calls().is_empty());
    assert_eq!(fs::read_dir(&setup.agents).unwrap().count(), 3);
    let real = UninstallOptions { dry_run: false, ..dry };
    installer(&fake).uninstall(&real).unwrap();
    let left: Vec<_> = fs::read_dir(&setup.agents).unwrap().map(|entry| entry.unwrap().file_name()).collect();
    assert_eq!(left, ["other.plist"]);
    let pattern = regex::Regex::new(r"dev\.kairos\.(collect|watchdog)$").unwrap();
    assert!(fake.calls().iter().all(|call| call[0] == "bootout" && pattern.is_match(&call[1])));
}

// 아래는 이번 전환의 변경점과 원본에 없는 시험이다.

#[test]
fn 실행본에는_두_실행_파일과_ops와_plugins만_놓이고_권한은_0755다() {
    let setup = Setup::new(InstallMode::Apply);
    let runtime = installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap().plan.runtime_dir;
    let mut names: Vec<String> = fs::read_dir(&runtime)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["bin", "ops", "plugins"]);
    assert_eq!(mode(&runtime), 0o700);
    assert_eq!(fs::read_to_string(runtime.join("bin/kairos-run")).unwrap(), "합성 kairos-run");
    for path in ["bin/kairos", "bin/kairos-run", "ops/kairos-watchdog.zsh"] {
        assert_eq!(mode(&runtime.join(path)), 0o755, "{path}");
    }
    assert!(runtime.join("plugins/collector/.claude-plugin/plugin.json").exists());
}

#[test]
fn 실행_파일이_빠져_있으면_실패하고_실행본을_만들지_않는다() {
    let setup = Setup::new(InstallMode::Apply);
    fs::remove_file(setup.options.binary_dir.as_ref().unwrap().join("kairos-run")).unwrap();
    let failure = installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap_err();
    assert_eq!(failure.code(), "install_binary_missing");
    assert!(!setup.test.join(VERSION).exists());
}

#[test]
fn 로그_파일은_0600으로_미리_만든다() {
    let setup = Setup::new(InstallMode::Apply);
    let plan = installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap().plan;
    for name in ["collect.log", "watchdog.log"] {
        assert_eq!(mode(&plan.log_dir.join(name)), 0o600, "{name}");
    }
}

#[test]
fn 미리보기는_지정한_폴더에만_쓴다() {
    let setup = Setup::new(InstallMode::DryRun);
    let preview = setup.test.join("preview");
    let options = InstallOptions { preview_dir: Some(preview.clone()), ..setup.options.clone() };
    let dry = installer(&FakeLaunchctl::ok()).install(&options).unwrap();
    assert_eq!(fs::read_dir(&preview).unwrap().count(), 2);
    assert!(!setup.agents.exists());
    assert_eq!(fs::read_to_string(preview.join("dev.kairos.collect.plist")).unwrap(), dry.plan.plists[0].content);
}

#[test]
fn 안내_줄은_node_없이_나온다() {
    let setup = Setup::new(InstallMode::DryRun);
    let lines = installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap().lines;
    assert_eq!(lines[0], "모드: 시험(dry-run). 아무것도 쓰지 않음");
    assert!(lines.iter().any(|line| line.starts_with("claude /")));
    assert!(!lines.iter().any(|line| line.starts_with("node ")));
    assert_eq!(lines.last().map(String::as_str), Some("등록하려면 kairos install --register"));
}

#[test]
fn 내리기만_할_때는_launchctl을_부르지_않고_plist를_지운다() {
    let setup = Setup::new(InstallMode::Apply);
    installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap();
    let fake = FakeLaunchctl::ok();
    let options = UninstallOptions {
        launch_agents_dir: Some(setup.agents.clone()),
        dry_run: false,
        skip_launchctl: true,
        before_removal: false,
    };
    assert!(!installer(&fake).uninstall(&options).unwrap().failed);
    assert!(fake.calls().is_empty());
    assert_eq!(fs::read_dir(&setup.agents).unwrap().count(), 0);
}

#[test]
fn 지우기_앞_단계는_내리지_못한_잡이_아직_올라_있으면_실패로_알린다() {
    let options = |setup: &Setup| UninstallOptions {
        launch_agents_dir: Some(setup.agents.clone()),
        dry_run: false,
        skip_launchctl: false,
        before_removal: true,
    };
    // bootout이 실패하고 print가 성공한다. 잡이 아직 올라 있다.
    let setup = Setup::new(InstallMode::Apply);
    installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap();
    let stuck = FakeLaunchctl::replying(|args| i32::from(args[0] == "bootout") * 5);
    let result = installer(&stuck).uninstall(&options(&setup)).unwrap();
    assert!(result.failed);
    assert!(result.lines.iter().any(|line| line == "dev.kairos.collect: 내리지 못함(코드 5)"), "{:?}", result.lines);
    assert_eq!(stuck.calls()[1], ["print", "gui/501/dev.kairos.collect"]);
    assert!(!result.lines.iter().any(|line| line.contains("--runtime")), "끝 안내는 호출자가 한다");
    // bootout도 print도 실패한다. 처음부터 올라 있지 않았다.
    let setup = Setup::new(InstallMode::Apply);
    installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap();
    let absent = FakeLaunchctl::replying(|args| if args[0] == "bootout" { 3 } else { 113 });
    let result = installer(&absent).uninstall(&options(&setup)).unwrap();
    assert!(!result.failed, "{:?}", result.lines);
    // print가 다른 까닭으로 실패한다(시간 초과, 실행 실패). 내려갔는지 모르므로 실패다.
    let setup = Setup::new(InstallMode::Apply);
    installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap();
    let unknown = FakeLaunchctl::replying(|args| if args[0] == "bootout" { 5 } else { 1 });
    let result = installer(&unknown).uninstall(&options(&setup)).unwrap();
    assert!(result.failed);
    assert!(
        result.lines.iter().any(|line| line.contains("내려갔는지 확인하지 못함(코드 5, 확인 1)")),
        "{:?}",
        result.lines
    );
    // 내려갔으면 다시 묻지 않는다.
    let setup = Setup::new(InstallMode::Apply);
    installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap();
    let fine = FakeLaunchctl::ok();
    assert!(!installer(&fine).uninstall(&options(&setup)).unwrap().failed);
    assert!(fine.calls().iter().all(|call| call[0] == "bootout"));
}

#[test]
fn 합성_템플릿은_실제_템플릿에서_node_줄만_뺀_것이다() {
    for name in ["dev.kairos.collect.plist.template", "dev.kairos.watchdog.plist.template"] {
        let real = fs::read_to_string(Path::new(REAL_OPS).join(name)).unwrap();
        let without_node: String = real.split_inclusive('\n').filter(|line| !line.contains("{{NODE}}")).collect();
        assert_eq!(fs::read_to_string(Path::new(ROOT).join("ops").join(name)).unwrap(), without_node, "{name}");
    }
}

/// 저장소의 allowed.ts는 바인딩이 없을 때 설치기가 만드는 것과 같아야 한다. 다르면 훅 시험과 설치본이 갈린다.
#[test]
fn 저장소의_허용_목록은_바인딩_없는_설치_결과와_같다() {
    let repo =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../plugins/collector/hooks/allowed.ts"))
            .unwrap();
    assert_eq!(repo, kairos_ops::collector_allowlist());
}

#[test]
fn 설치는_current를_이번_판으로_건다() {
    let setup = Setup::new(InstallMode::Apply);
    installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap();
    installer(&FakeLaunchctl::ok()).install(&setup.options).unwrap();
    let link = setup.test.home.root().join("current");
    assert_eq!(std::fs::read_link(&link).unwrap(), std::path::Path::new(kairos_ops::VERSION));
    assert!(link.join("bin").join("kairos").exists());
}

#[test]
fn 등록은_옛_라벨을_내리고_plist를_지운다() {
    let setup = Setup::new(InstallMode::Register);
    fs::create_dir_all(&setup.agents).unwrap();
    let legacy = setup.agents.join("com.example-user.kairos.collect.plist");
    fs::write(&legacy, legacy_plist("com.example-user.kairos.collect")).unwrap();
    // 모양이 다른 남의 잡은 건드리지 않는다.
    let others =
        ["com.a.b.kairos.collect.plist", "com.example.other.collect.plist", "org.example.kairos.watchdog.plist"];
    for name in others {
        fs::write(setup.agents.join(name), legacy_plist(name.trim_end_matches(".plist"))).unwrap();
    }
    // 이름이 같은 모양이어도 kairos가 쓴 plist가 아니면 남의 잡이다.
    let vendor = setup.agents.join("com.vendor.kairos.watchdog.plist");
    fs::write(&vendor, "<plist><dict><key>Label</key><string>com.vendor.kairos.watchdog</string></dict></plist>")
        .unwrap();
    // kairos plist를 가리키는 링크도 내리지 않는다.
    let target = setup.agents.join("elsewhere.xml");
    fs::write(&target, legacy_plist("com.linked.kairos.collect")).unwrap();
    std::os::unix::fs::symlink(&target, setup.agents.join("com.linked.kairos.collect.plist")).unwrap();
    let fake = FakeLaunchctl::ok();
    let registered = installer(&fake).install(&setup.options).unwrap();
    assert!(!legacy.exists());
    assert_eq!(fake.calls()[0], ["bootout", "gui/501/com.example-user.kairos.collect"]);
    assert!(registered.lines.contains(&"com.example-user.kairos.collect: 옛 라벨 내림".to_owned()));
    assert!(!registered.failed);
    for name in others {
        assert!(setup.agents.join(name).exists(), "{name}");
    }
    assert!(vendor.exists());
    assert!(setup.agents.join("com.linked.kairos.collect.plist").exists());
    assert_eq!(fake.calls().iter().filter(|call| call[0] == "bootout" && call[1].contains("/com.")).count(), 1);
}

/// 0.2 설치기가 쓰던 plist의 줄임판. 자기 라벨과 KAIROS_HOME 환경 키가 있다.
fn legacy_plist(label: &str) -> String {
    format!(
        "<plist><dict><key>Label</key><string>{label}</string><key>EnvironmentVariables</key><dict><key>KAIROS_HOME</key><string>/tmp/x</string></dict></dict></plist>"
    )
}

#[test]
fn 옛_라벨은_com_이름_kairos_모양만_받는다() {
    use kairos_ops::is_legacy_label;
    for label in ["com.someone.kairos.collect", "com.some_one-2.kairos.watchdog"] {
        assert!(is_legacy_label(label), "{label}");
    }
    for label in [
        "dev.kairos.collect",
        "com..kairos.collect",
        "com.a.b.kairos.collect",
        "com.x.kairos.other",
        "com.x y.kairos.collect",
    ] {
        assert!(!is_legacy_label(label), "{label}");
    }
}
