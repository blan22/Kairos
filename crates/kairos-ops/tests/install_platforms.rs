//! Linux(systemd)와 Windows(작업 스케줄러) 설치. 파일 생성과 호출 순서만 본다. 스케줄러는 가짜다.
//! 어느 호스트에서나 돈다. 다른 운영체제의 파일은 `os`를 주어 만든다.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kairos_core::KairosHome;
use kairos_core::platform::Os;
use kairos_core::process::{ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_ops::{InstallPaths, InstallPlan, Installer, PlanInput, Scheduler, UninstallOptions, VERSION, plan_install};
use serde_json::Value;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/install-root");
const REAL_OPS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ops");

/// 프로그램 이름과 인자를 받아 종료 코드를 정한다.
type Reply = Box<dyn Fn(&str, &[String]) -> i32 + Send + Sync>;

/// 호출을 (프로그램 이름, 인자)로 적고 `reply`의 코드를 돌려준다.
struct FakeScheduler {
    calls: Mutex<Vec<(String, Vec<String>)>>,
    reply: Reply,
}

impl FakeScheduler {
    fn replying(reply: impl Fn(&str, &[String]) -> i32 + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self { calls: Mutex::new(Vec::new()), reply: Box::new(reply) })
    }

    fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }

    /// 인자만, 프로그램 이름이 `name`으로 끝나는 호출.
    fn args_of(&self, name: &str) -> Vec<Vec<String>> {
        self.calls().into_iter().filter(|(program, _)| program.ends_with(name)).map(|(_, args)| args).collect()
    }
}

impl ProcessRunner for FakeScheduler {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        self.calls.lock().unwrap().push((request.program.clone(), request.args.clone()));
        ProcessOutput::exited((self.reply)(&request.program, &request.args), "")
    }
}

fn linux_paths() -> InstallPaths {
    InstallPaths { claude: "/home/synthetic/.local/bin/claude".into(), gh: "/usr/bin/gh".into() }
}

fn windows_paths() -> InstallPaths {
    InstallPaths {
        claude: r"C:\Users\synthetic\.local\bin\claude.exe".into(),
        gh: r"C:\Program Files\GitHub CLI\gh.exe".into(),
    }
}

fn plan_for(os: Os, home: &KairosHome, root: &Path, paths: &InstallPaths, service_dir: &Path) -> InstallPlan {
    plan_install(&PlanInput { home, root, paths, service_dir: Some(service_dir.to_path_buf()), os }).unwrap()
}

fn file<'a>(plan: &'a InstallPlan, name: &str) -> &'a str {
    let found = plan.services.iter().find(|file| file.path.file_name().is_some_and(|n| n == name));
    &found.unwrap_or_else(|| panic!("{name} 없음")).content
}

#[test]
fn systemd_유닛은_timer와_service를_만들고_3분과_5분_간격이다() {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at("/home/synthetic/.local/share/kairos");
    for root in [Path::new(ROOT), &Path::new(REAL_OPS).join("..")] {
        let plan = plan_for(Os::Linux, &home, root, &linux_paths(), dir.path());
        let names: Vec<String> =
            plan.services.iter().map(|file| file.path.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(
            names,
            [
                "dev.kairos.collect.timer",
                "dev.kairos.collect.service",
                "dev.kairos.watchdog.timer",
                "dev.kairos.watchdog.service"
            ]
        );
        let bin = format!("/home/synthetic/.local/share/kairos/{VERSION}/bin/kairos");
        let collect = file(&plan, "dev.kairos.collect.service");
        assert!(collect.contains(&format!("ExecStart=\"{bin}\" collect\n")), "{collect}");
        assert!(collect.contains("Environment=\"KAIROS_CLAUDE=/home/synthetic/.local/bin/claude\"\n"));
        assert!(collect.contains("Environment=\"KAIROS_HOME=/home/synthetic/.local/share/kairos\"\n"));
        assert!(collect.contains(
            "Environment=\"PATH=/home/synthetic/.local/bin:/usr/bin:/usr/local/bin:/bin:/usr/sbin:/sbin\"\n"
        ));
        assert!(collect.contains("UMask=0077\n"));
        assert!(file(&plan, "dev.kairos.collect.timer").contains("OnCalendar=*:0/3\n"));
        assert!(file(&plan, "dev.kairos.collect.timer").contains("Unit=dev.kairos.collect.service\n"));
        assert!(file(&plan, "dev.kairos.watchdog.timer").contains("OnCalendar=*:0/5\n"));
        let watchdog = file(&plan, "dev.kairos.watchdog.service");
        assert!(watchdog.contains(&format!("ExecStart=\"{bin}\" watchdog\n")), "{watchdog}");
        assert!(!watchdog.contains("KAIROS_CLAUDE"), "감시자는 claude를 받지 않는다");
        assert!(plan.watchdog.is_none(), "zsh 감시자는 macOS만 쓴다");
        for service in &plan.services {
            assert!(!service.content.contains("{{"), "{}", service.content);
        }
    }
}

#[test]
fn systemd_유닛에_넣을_수_없는_경로는_설치하지_않는다() {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at("/home/synthetic/$data/kairos");
    let failure = plan_install(&PlanInput {
        home: &home,
        root: Path::new(ROOT),
        paths: &linux_paths(),
        service_dir: Some(dir.path().to_path_buf()),
        os: Os::Linux,
    })
    .unwrap_err();
    assert_eq!(failure.code(), "install_value_unsupported");
}

#[test]
fn 작업_스케줄러는_런처와_실행_설명으로_kairos를_띄운다() {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at(r"C:\Users\synthetic\AppData\Local\kairos");
    for root in [Path::new(ROOT), &Path::new(REAL_OPS).join("..")] {
        let plan = plan_for(Os::Windows, &home, root, &windows_paths(), dir.path());
        let runtime = format!(r"C:\Users\synthetic\AppData\Local\kairos\{VERSION}");
        assert_eq!(plan.kairos_bin, PathBuf::from(format!(r"{runtime}\bin\kairos.exe")));
        let task = file(&plan, "dev.kairos.collect.xml");
        assert!(task.contains(&format!(r"<Command>{runtime}\bin\kairos-launch.exe</Command>")), "{task}");
        let spec_path = dir.path().join("dev.kairos.collect.json");
        assert!(task.contains(&format!("<Arguments>\"{}\"</Arguments>", spec_path.display())), "{task}");
        assert!(task.contains("<Interval>PT3M</Interval>") && task.contains(r"<URI>\kairos\dev.kairos.collect</URI>"));
        assert!(task.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
        assert!(file(&plan, "dev.kairos.watchdog.xml").contains("<Interval>PT5M</Interval>"));

        let spec: Value = serde_json::from_str(file(&plan, "dev.kairos.collect.json")).unwrap();
        assert_eq!(spec["args"], serde_json::json!(["collect"]));
        assert_eq!(spec["env"]["KAIROS_CLAUDE"], r"C:\Users\synthetic\.local\bin\claude.exe");
        assert_eq!(spec["env"]["KAIROS_HOME"], r"C:\Users\synthetic\AppData\Local\kairos");
        let path = spec["env"]["PATH"].as_str().unwrap();
        assert!(path.starts_with(r"C:\Users\synthetic\.local\bin;C:\Program Files\GitHub CLI;"), "{path}");
        assert_eq!(spec["log"], r"C:\Users\synthetic\AppData\Local\kairos\logs\collect.log");
        let watchdog: Value = serde_json::from_str(file(&plan, "dev.kairos.watchdog.json")).unwrap();
        assert_eq!(watchdog["args"], serde_json::json!(["watchdog"]));
        assert_eq!(watchdog["env"].as_object().unwrap().keys().collect::<Vec<_>>(), ["KAIROS_HOME"]);
    }
}

#[test]
fn 설치된_파일에서_claude_경로를_읽는다() {
    let dir = tempfile::tempdir().unwrap();
    let linux = plan_for(Os::Linux, &KairosHome::at("/synthetic/kairos"), Path::new(ROOT), &linux_paths(), dir.path());
    for service in &linux.services {
        fs::write(&service.path, &service.content).unwrap();
    }
    assert_eq!(
        Scheduler::Systemd.installed_value(dir.path(), "dev.kairos.collect", "KAIROS_CLAUDE").as_deref(),
        Some("/home/synthetic/.local/bin/claude")
    );
    let windows = tempfile::tempdir().unwrap();
    let plan = plan_for(
        Os::Windows,
        &KairosHome::at(r"C:\synthetic\kairos"),
        Path::new(ROOT),
        &windows_paths(),
        windows.path(),
    );
    for service in &plan.services {
        fs::write(&service.path, &service.content).unwrap();
    }
    assert_eq!(
        Scheduler::TaskScheduler.installed_value(windows.path(), "dev.kairos.collect", "KAIROS_CLAUDE").as_deref(),
        Some(r"C:\Users\synthetic\.local\bin\claude.exe")
    );
    assert_eq!(Scheduler::Systemd.installed_value(windows.path(), "dev.kairos.collect", "KAIROS_CLAUDE"), None);
}

fn uninstall_options(os: Os, dir: &Path, before_removal: bool) -> UninstallOptions {
    UninstallOptions {
        service_dir: Some(dir.to_path_buf()),
        home: Some(KairosHome::at(dir)),
        before_removal,
        os,
        ..UninstallOptions::default()
    }
}

fn write_all(plan: &InstallPlan) {
    for service in &plan.services {
        fs::write(&service.path, &service.content).unwrap();
    }
}

#[test]
fn systemd_제거는_timer를_끄고_파일을_지운_뒤_다시_읽게_한다() {
    let dir = tempfile::tempdir().unwrap();
    write_all(&plan_for(Os::Linux, &KairosHome::at("/synthetic/kairos"), Path::new(ROOT), &linux_paths(), dir.path()));
    fs::write(dir.path().join("other.timer"), "x").unwrap();
    let fake = FakeScheduler::replying(|_, _| 0);
    let result = Installer::new(fake.clone()).uninstall(&uninstall_options(Os::Linux, dir.path(), false)).unwrap();
    assert!(!result.failed, "{:?}", result.lines);
    let calls = fake.args_of("systemctl");
    assert_eq!(calls[0], ["--user", "disable", "--now", "dev.kairos.collect.timer"]);
    assert_eq!(calls[1], ["--user", "stop", "dev.kairos.collect.service"]);
    assert_eq!(calls.last().unwrap(), &["--user", "daemon-reload"]);
    let left: Vec<String> = fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["other.timer"], "남의 유닛은 지우지 않는다");
    assert!(result.lines.iter().any(|line| line == "dev.kairos.collect: 내림"));
}

#[test]
fn systemd_지우기_앞_단계는_timer가_아직_돌면_실패로_알린다() {
    let dir = tempfile::tempdir().unwrap();
    // disable이 실패하고 is-active가 0(돌고 있음)이다.
    let fake = FakeScheduler::replying(|_, args| match args.get(1).map(String::as_str) {
        Some("disable") => 1,
        Some("is-active") => 0,
        _ => 0,
    });
    let result = Installer::new(fake).uninstall(&uninstall_options(Os::Linux, dir.path(), true)).unwrap();
    assert!(result.failed);
    assert!(result.lines.iter().any(|line| line == "dev.kairos.collect: 내리지 못함(코드 1)"), "{:?}", result.lines);
    // 돌지 않으면(3) 내릴 것이 없던 것이다.
    let quiet = FakeScheduler::replying(|_, args| if args.get(1).is_some_and(|a| a == "disable") { 1 } else { 3 });
    assert!(!Installer::new(quiet).uninstall(&uninstall_options(Os::Linux, dir.path(), true)).unwrap().failed);
}

#[test]
fn 작업_스케줄러_제거는_작업과_알림_등록을_지운다() {
    let dir = tempfile::tempdir().unwrap();
    let plan =
        plan_for(Os::Windows, &KairosHome::at(r"C:\synthetic\kairos"), Path::new(ROOT), &windows_paths(), dir.path());
    write_all(&plan);
    let fake = FakeScheduler::replying(|_, _| 0);
    let result = Installer::new(fake.clone()).uninstall(&uninstall_options(Os::Windows, dir.path(), false)).unwrap();
    assert!(!result.failed, "{:?}", result.lines);
    assert_eq!(
        fake.args_of("schtasks.exe"),
        [
            vec!["/Delete", "/TN", r"\kairos\dev.kairos.collect", "/F"],
            vec!["/Delete", "/TN", r"\kairos\dev.kairos.watchdog", "/F"]
        ]
    );
    let reg = fake.args_of("reg.exe");
    assert_eq!(reg[0], ["delete", r"HKCU\Software\Classes\kairos-reveal", "/f"]);
    assert_eq!(reg[1], ["delete", r"HKCU\Software\Classes\AppUserModelId\dev.kairos.notifier", "/f"]);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn dry_run과_스케줄러를_부르지_않는_제거는_호출하지_않는다() {
    for os in [Os::Linux, Os::Windows] {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeScheduler::replying(|_, _| 0);
        let dry = UninstallOptions { dry_run: true, ..uninstall_options(os, dir.path(), false) };
        let lines = Installer::new(fake.clone()).uninstall(&dry).unwrap().lines;
        assert!(lines[1].starts_with("내릴 잡 dev.kairos.collect, 지울 파일 "), "{lines:?}");
        let skip = UninstallOptions { skip_launchctl: true, ..uninstall_options(os, dir.path(), false) };
        Installer::new(fake.clone()).uninstall(&skip).unwrap();
        assert!(fake.calls().is_empty(), "{os:?}");
    }
}

#[test]
fn 기본_파일_폴더는_운영체제마다_다르다() {
    let home = KairosHome::at("/synthetic/kairos");
    assert!(kairos_ops::default_service_dir(Os::MacOs, &home).ends_with("Library/LaunchAgents"));
    assert!(kairos_ops::default_service_dir(Os::Linux, &home).ends_with("systemd/user"));
    assert_eq!(kairos_ops::default_service_dir(Os::Windows, &home), Path::new("/synthetic/kairos/tasks"));
}
