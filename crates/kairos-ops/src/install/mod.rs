mod allowlist;
mod binary;
mod launchctl;
mod plan;
mod remove;
mod runtime;
mod scheduler;
mod systemd;
mod tasks;

use std::fs::{self, DirBuilder, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use kairos_core::home::DIR_MODE;
use kairos_core::platform::{CreateMode, Os, set_mode};
use kairos_core::process::ProcessRunner;
use kairos_core::{KairosHome, KairosResult};

pub use allowlist::collector_allowlist;
pub use binary::{find_binary, find_claude};
use launchctl::Launchctl;
pub use plan::{
    InstallPaths, InstallPlan, PlanInput, ServiceFile, VERSION, default_launch_agents_dir, default_service_dir,
    plan_install, render_template,
};
pub use remove::{RemovalPlan, plan_runtime_removal, purge_refusal, remove_data, remove_runtime, runtime_refusal};
pub use scheduler::Scheduler;
use systemd::Systemctl;
use tasks::Schtasks;
pub use tasks::{NOTIFIER_APP_ID, REVEAL_SCHEME, system_tool};

use crate::fs_util::remove_if_present;

pub const COLLECT_LABEL: &str = "dev.kairos.collect";
pub const WATCHDOG_LABEL: &str = "dev.kairos.watchdog";
pub const LABELS: [&str; 2] = [COLLECT_LABEL, WATCHDOG_LABEL];
/// 0.2까지 쓰던 라벨의 끝. 앞은 `com.<설치한 사람>.`이라 사람마다 달라 폴더에서 찾는다.
const LEGACY_SUFFIXES: [&str; 2] = [".kairos.collect", ".kairos.watchdog"];
const BOOTSTRAP_RETRIES: usize = 5;
const BOOTSTRAP_RETRY: Duration = Duration::from_millis(500);
/// bootout이 끝나기 전에 올리면 나오는 코드(측정).
const BOOTSTRAP_BUSY: i32 = 5;
const LOG_MODE: u32 = 0o600;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallMode {
    /// 기본값이다. 아무것도 쓰지 않는다.
    DryRun,
    Apply,
    Register,
}

impl InstallMode {
    fn describe(self, scheduler: Scheduler) -> String {
        let tool = match scheduler {
            Scheduler::Launchd => "launchctl",
            other => other.name(),
        };
        match self {
            Self::DryRun => "시험(dry-run). 아무것도 쓰지 않음".to_owned(),
            Self::Apply => format!("파일만 설치. {tool} 등록 안 함"),
            Self::Register => format!("설치 후 {tool} 등록"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub home: KairosHome,
    /// `ops/`와 `plugins/`의 복사 원본 루트. 템플릿도 여기서 읽는다.
    pub root: PathBuf,
    pub mode: InstallMode,
    /// 스케줄러 파일 폴더. None이면 `default_service_dir`다(macOS는 `~/Library/LaunchAgents`).
    pub service_dir: Option<PathBuf>,
    /// 설치할 운영체제. 기본은 지금 운영체제다. 시험은 다른 운영체제의 파일과 호출을 만든다.
    pub os: Os,
    /// `--claude`. 주면 찾지 않고 이 경로를 검사해 쓴다.
    pub claude: Option<String>,
    /// `--gh`. `claude`와 같다.
    pub gh: Option<String>,
    /// kairos와 kairos-run이 있는 폴더. None이면 지금 도는 실행 파일의 폴더다.
    pub binary_dir: Option<PathBuf>,
    /// dry-run에서 생성될 plist를 이 폴더에 쓴다. 지정한 폴더 밖은 건드리지 않는다.
    pub preview_dir: Option<PathBuf>,
    pub bootstrap_retry: Duration,
}

impl InstallOptions {
    pub fn new(home: KairosHome, root: impl Into<PathBuf>) -> Self {
        Self {
            home,
            root: root.into(),
            mode: InstallMode::DryRun,
            service_dir: None,
            os: Os::current(),
            claude: None,
            gh: None,
            binary_dir: None,
            preview_dir: None,
            bootstrap_retry: BOOTSTRAP_RETRY,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallResult {
    pub plan: InstallPlan,
    pub mode: InstallMode,
    pub lines: Vec<String>,
    pub failed: bool,
}

/// `launchctl print`가 그 이름의 서비스가 없을 때 내는 코드.
const NO_SERVICE: i32 = 113;

#[derive(Clone, Debug)]
pub struct UninstallOptions {
    /// 스케줄러 파일 폴더. None이면 `default_service_dir`다(macOS는 `~/Library/LaunchAgents`).
    pub service_dir: Option<PathBuf>,
    /// 데이터 폴더. 작업 스케줄러의 기본 파일 폴더가 그 아래다. None이면 `KairosHome::from_env`다.
    pub home: Option<KairosHome>,
    pub dry_run: bool,
    /// 스케줄러(launchctl, systemctl, schtasks)를 부르지 않고 파일만 지운다.
    pub skip_launchctl: bool,
    /// 실행본이나 데이터까지 지우는 호출의 앞 단계다. 끝의 안내 두 줄(남긴 것)은 호출자가 직접 알리므로 찍지 않고,
    /// 내리지 못한 잡이 아직 올라 있는지 확인해 실패로 알린다. 남은 잡은 지운 경로를 계속 가리킨다.
    pub before_removal: bool,
    pub os: Os,
}

impl Default for UninstallOptions {
    fn default() -> Self {
        Self {
            service_dir: None,
            home: None,
            dry_run: false,
            skip_launchctl: false,
            before_removal: false,
            os: Os::current(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UninstallResult {
    pub lines: Vec<String>,
    pub failed: bool,
}

/// 실행본 배치와 launchd 등록.
#[derive(Clone)]
pub struct Installer {
    runner: Arc<dyn ProcessRunner>,
}

impl Installer {
    pub fn new(runner: Arc<dyn ProcessRunner>) -> Self {
        Self { runner }
    }

    pub fn install(&self, options: &InstallOptions) -> KairosResult<InstallResult> {
        let os = options.os;
        let scheduler = Scheduler::for_os(os);
        let service_dir = options.service_dir.clone().unwrap_or_else(|| default_service_dir(os, &options.home));
        let env_path = std::env::var("PATH").unwrap_or_default();
        let paths =
            binary::resolve_paths(os, options.claude.as_deref(), options.gh.as_deref(), &env_path, &service_dir)?;
        let plan = plan_install(&PlanInput {
            home: &options.home,
            root: &options.root,
            paths: &paths,
            service_dir: Some(service_dir),
            os,
        })?;
        let mut lines = vec![
            format!("모드: {}", options.mode.describe(scheduler)),
            format!("실행본: {}", plan.runtime_dir.display()),
            format!("claude {}", paths.claude),
            format!("gh {}", paths.gh),
            format!("PATH {}", plan.path_value),
        ];
        lines.extend(plan.services.iter().map(|file| format!("{} {}", scheduler.file_noun(), file.path.display())));

        if options.mode == InstallMode::DryRun {
            write_preview(&plan, options, &mut lines)?;
            lines.push("등록하려면 kairos install --register".to_owned());
            return Ok(InstallResult { plan, mode: options.mode, lines, failed: false });
        }

        write_files(&plan, options, &self.runner)?;
        lines.push(match scheduler {
            Scheduler::Launchd => "실행본과 plist를 설치했습니다.".to_owned(),
            other => format!("실행본과 {}을 설치했습니다.", other.file_noun()),
        });
        let failed = options.mode == InstallMode::Register
            && match scheduler {
                Scheduler::Launchd => self.register(&plan, options, &mut lines),
                Scheduler::Systemd => self.register_systemd(&mut lines),
                Scheduler::TaskScheduler => self.register_tasks(&plan, options, &mut lines),
            };
        Ok(InstallResult { plan, mode: options.mode, lines, failed })
    }

    /// systemd 사용자 timer를 켠다. 유닛을 다시 읽고 켠 뒤 다시 시작해 바뀐 일정을 바로 쓴다. 실패가 있었으면 참이다.
    fn register_systemd(&self, lines: &mut Vec<String>) -> bool {
        let systemctl = Systemctl::new(&self.runner);
        let reload = systemctl.run(&["daemon-reload"]);
        if reload != 0 {
            lines.push(format!("{}(코드 {reload})", systemd::NO_USER_SESSION));
            return true;
        }
        let mut failed = false;
        for label in LABELS {
            let timer = format!("{label}.timer");
            let mut code = systemctl.run(&["enable", &timer]);
            if code == 0 {
                code = systemctl.run(&["restart", &timer]);
            }
            failed |= code != 0;
            let status = if code == 0 { "등록됨".to_owned() } else { format!("등록 실패(코드 {code})") };
            lines.push(format!("{label}: {status}"));
            if code == 0 && label == COLLECT_LABEL {
                lines.push(collect_now_line(systemctl.run(&["start", "--no-block", &format!("{label}.service")])));
            }
        }
        failed
    }

    /// 작업 스케줄러에 작업을 만들고(이미 있으면 바꾼다), 알림의 링크 처리기와 앱 ID를 쓴다. 실패가 있었으면 참이다.
    fn register_tasks(&self, plan: &InstallPlan, options: &InstallOptions, lines: &mut Vec<String>) -> bool {
        let schtasks = Schtasks::new(&self.runner);
        let mut failed = false;
        for file in plan.services.iter().filter(|file| file.path.extension().is_some_and(|ext| ext == "xml")) {
            let name = tasks::task_name(&file.label);
            let path = file.path.to_string_lossy();
            let code = schtasks.run(&["/Create", "/TN", &name, "/XML", &path, "/F"]);
            failed |= code != 0;
            let status = if code == 0 { "등록됨".to_owned() } else { format!("등록 실패(코드 {code})") };
            lines.push(format!("{}: {status}", file.label));
            if code == 0 && file.label == COLLECT_LABEL {
                lines.push(collect_now_line(schtasks.run(&["/Run", "/TN", &name])));
            }
        }
        let current = options.home.root().join("current").join("bin").join("kairos.exe");
        let missed = schtasks.register_notifier(&current.to_string_lossy());
        lines.push(if missed == 0 {
            "알림: 등록됨(누르면 kairos reveal)".to_owned()
        } else {
            format!("알림: 등록 실패({missed}개 키). 알림을 눌러도 패널이 열리지 않습니다")
        });
        failed
    }

    /// 실패가 있었으면 참이다.
    fn register(&self, plan: &InstallPlan, options: &InstallOptions, lines: &mut Vec<String>) -> bool {
        let launchctl = Launchctl::new(&self.runner);
        let domain = launchctl.domain();
        let mut failed = false;
        for label in legacy_labels(&plan.service_dir) {
            let legacy = plan.service_dir.join(format!("{label}.plist"));
            launchctl.run(&["bootout", &format!("{domain}/{label}")]);
            let removed = remove_if_present(&legacy).is_ok() && !legacy.exists();
            lines.push(format!("{label}: 옛 라벨 {}", if removed { "내림" } else { "내리지 못함" }));
            failed |= !removed;
        }
        for plist in &plan.services {
            let path = plist.path.to_string_lossy();
            let bootstrap = || launchctl.run(&["bootstrap", &domain, &path]);
            // 이미 등록돼 있으면 내리고 다시 올린다. 내리기 실패는 무시한다.
            launchctl.run(&["bootout", &format!("{domain}/{}", plist.label)]);
            // bootout이 끝나기 전에 올리면 코드 5로 실패한다(측정). 수집이 조용히 멈추지 않게 잠깐 기다려 다시 시도한다.
            let mut code = bootstrap();
            let mut attempt = 0;
            while attempt < BOOTSTRAP_RETRIES && code == BOOTSTRAP_BUSY {
                thread::sleep(options.bootstrap_retry);
                code = bootstrap();
                attempt += 1;
            }
            failed |= code != 0;
            let status = if code == 0 { "등록됨".to_owned() } else { format!("등록 실패(코드 {code})") };
            lines.push(format!("{}: {status}", plist.label));
            if code == 0 && plist.label == COLLECT_LABEL {
                lines.push(Self::collect_now(&launchctl, &domain));
            }
        }
        failed
    }

    /// 등록한 수집 잡을 바로 한 번 돌린다. 다음 3분 눈금을 기다리지 않고 설치 직후에 수집하고, 알릴 것이 있으면 그 틱이 알린다.
    /// launchd가 plist의 인자와 환경 그대로 띄우므로 수집 시간, 수집 중단, 방해 금지 판정은 예약된 틱과 같다.
    /// 이미 도는 틱이 있으면 launchd가 새로 띄우지 않는다. 실패해도 잡은 등록돼 있어 다음 눈금에 수집하므로 설치 실패로 세지 않는다.
    fn collect_now(launchctl: &Launchctl<'_>, domain: &str) -> String {
        collect_now_line(launchctl.run(&["kickstart", &format!("{domain}/{COLLECT_LABEL}")]))
    }

    pub fn uninstall(&self, options: &UninstallOptions) -> KairosResult<UninstallResult> {
        match Scheduler::for_os(options.os) {
            Scheduler::Launchd => self.uninstall_launchd(options),
            other => self.uninstall_other(other, options),
        }
    }

    /// systemd와 작업 스케줄러. 잡을 내리고 파일을 지운다. 끝의 안내는 launchd와 같다.
    fn uninstall_other(&self, scheduler: Scheduler, options: &UninstallOptions) -> KairosResult<UninstallResult> {
        let home = options.home.clone().unwrap_or_else(KairosHome::from_env);
        let dir = options.service_dir.clone().unwrap_or_else(|| default_service_dir(options.os, &home));
        let first = if options.dry_run {
            "시험(dry-run). 아무것도 바꾸지 않음"
        } else {
            "kairos 잡을 내립니다"
        };
        let mut lines = vec![first.to_owned()];
        let mut failed = false;
        let call = !options.dry_run && !options.skip_launchctl;
        let (systemctl, schtasks) = (Systemctl::new(&self.runner), Schtasks::new(&self.runner));
        for label in LABELS {
            let files: Vec<PathBuf> = scheduler.file_names(label).iter().map(|name| dir.join(name)).collect();
            if options.dry_run {
                let shown: Vec<String> = files
                    .iter()
                    .map(|path| format!("{}{}", path.display(), if path.exists() { "" } else { " (없음)" }))
                    .collect();
                lines.push(format!("내릴 잡 {label}, 지울 파일 {}", shown.join(", ")));
                continue;
            }
            if call {
                // 지우기 앞 단계에서만 아직 올라 있는지 다시 묻는다. 확인에 실패하면 올라 있는 것으로 센다.
                let (code, still) = if scheduler == Scheduler::Systemd {
                    let timer = format!("{label}.timer");
                    let code = systemctl.run(&["disable", "--now", &timer]);
                    systemctl.run(&["stop", &format!("{label}.service")]);
                    let check = (code != 0 && options.before_removal).then(|| systemctl.run(&["is-active", &timer]));
                    (code, check.map(|code| code != systemd::INACTIVE))
                } else {
                    let name = tasks::task_name(label);
                    let code = schtasks.run(&["/Delete", "/TN", &name, "/F"]);
                    let check = (code != 0 && options.before_removal).then(|| schtasks.run(&["/Query", "/TN", &name]));
                    (code, check.map(|code| code != tasks::NO_TASK))
                };
                failed |= still == Some(true);
                let status = match (code, still) {
                    (0, _) => "내림".to_owned(),
                    (_, Some(true)) => format!("내리지 못함(코드 {code})"),
                    _ => format!("내릴 것 없음 또는 실패(코드 {code})"),
                };
                lines.push(format!("{label}: {status}"));
            }
            for path in &files {
                remove_if_present(path)?;
                failed |= path.exists();
            }
        }
        if call {
            if scheduler == Scheduler::Systemd {
                systemctl.run(&["daemon-reload"]);
            } else {
                schtasks.unregister_notifier();
            }
        }
        if !options.before_removal {
            push_uninstall_notes(&mut lines);
        }
        Ok(UninstallResult { lines, failed })
    }

    fn uninstall_launchd(&self, options: &UninstallOptions) -> KairosResult<UninstallResult> {
        let dir = options.service_dir.clone().unwrap_or_else(default_launch_agents_dir);
        let first = if options.dry_run {
            "시험(dry-run). 아무것도 바꾸지 않음"
        } else {
            "kairos 잡을 내립니다"
        };
        let mut lines = vec![first.to_owned()];
        let mut failed = false;
        let launchctl = Launchctl::new(&self.runner);
        let domain = (!options.dry_run && !options.skip_launchctl).then(|| launchctl.domain());
        // 옛 라벨은 plist가 있는 것만 나온다.
        let labels = LABELS.into_iter().map(str::to_owned).chain(legacy_labels(&dir));
        for label in labels {
            let plist = dir.join(format!("{label}.plist"));
            if options.dry_run {
                let absent = if plist.exists() { "" } else { " (없음)" };
                lines.push(format!("내릴 잡 {label}, 지울 파일 {}{absent}", plist.display()));
                continue;
            }
            if let Some(domain) = &domain {
                let target = format!("{domain}/{label}");
                let code = launchctl.run(&["bootout", &target]);
                // 이미 내려가 있으면 실패 코드가 나온다. 지우기 앞 단계에서만 아직 올라 있는지 다시 묻는다.
                // 없다는 답(NO_SERVICE)일 때만 없다고 본다. 시간 초과나 실행 실패는 모르는 것이라 실패로 센다.
                let check = (code != 0 && options.before_removal).then(|| launchctl.run(&["print", &target]));
                failed |= check.is_some_and(|code| code != NO_SERVICE);
                let status = match (code, check) {
                    (0, _) => "내림".to_owned(),
                    (_, Some(0)) => format!("내리지 못함(코드 {code})"),
                    (_, Some(NO_SERVICE)) | (_, None) => format!("내릴 것 없음 또는 실패(코드 {code})"),
                    (_, Some(other)) => format!("내려갔는지 확인하지 못함(코드 {code}, 확인 {other})"),
                };
                lines.push(format!("{label}: {status}"));
            }
            remove_if_present(&plist)?;
            failed |= plist.exists();
        }
        if !options.before_removal {
            push_uninstall_notes(&mut lines);
        }
        Ok(UninstallResult { lines, failed })
    }
}

fn push_uninstall_notes(lines: &mut Vec<String>) {
    lines.push(
        "데이터 폴더와 DB는 지우지 않았습니다. 실행본까지 지우려면 kairos uninstall --runtime, 데이터까지 지우려면 --purge."
            .to_owned(),
    );
    // 플러그인 승인은 사용자가 했으므로 내리는 것도 사용자가 한다. 내리지 않으면 워커가 설치본 kairos를 계속 부른다.
    lines.push(
        "Orca 패널 플러그인은 내리지 않았습니다. 올린 적이 있다면 orca plugin dev unload <설치본의 plugins/orca-panel 경로>로 내리세요."
            .to_owned(),
    );
}

/// 등록 직후 첫 수집의 줄. 실패해도 잡은 등록돼 있어 다음 눈금에 수집하므로 설치 실패로 세지 않는다.
fn collect_now_line(code: i32) -> String {
    if code == 0 {
        format!("{COLLECT_LABEL}: 첫 수집을 바로 시작함")
    } else {
        format!("{COLLECT_LABEL}: 첫 수집을 바로 시작하지 못함(코드 {code}). 다음 틱에 수집합니다")
    }
}

/// 0.2까지 쓰던 라벨인가. `com.<이름>.kairos.collect`, `com.<이름>.kairos.watchdog`이고 이름은 한 조각(영숫자, `-`, `_`)이다.
pub fn is_legacy_label(label: &str) -> bool {
    let Some(rest) = label.strip_prefix("com.") else {
        return false;
    };
    LEGACY_SUFFIXES.iter().any(|suffix| {
        rest.strip_suffix(suffix).is_some_and(|name| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
    })
}

/// LaunchAgents 폴더에 남은 옛 라벨, 이름순. 등록과 제거 때 내리고 plist를 지운다. 남겨 두면 수집이 두 번 돈다.
/// 이름만 보고 고르면 같은 모양의 남의 잡을 지운다. 내용이 kairos가 쓴 plist인 것만 고른다.
pub fn legacy_labels(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut labels: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str()?.strip_suffix(".plist").map(str::to_owned))
        .filter(|label| is_legacy_label(label) && written_by_kairos(&dir.join(format!("{label}.plist")), label))
        .collect();
    labels.sort();
    labels
}

/// 옛 kairos plist의 표지: 링크가 아닌 작은 파일이고, 자기 라벨과 `KAIROS_HOME` 환경 키가 있다.
fn written_by_kairos(path: &Path, label: &str) -> bool {
    const MAX_PLIST_BYTES: u64 = 64 * 1024;
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.is_file() || meta.len() > MAX_PLIST_BYTES {
        return false;
    }
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    text.contains(&format!("<string>{label}</string>")) && text.contains("<key>KAIROS_HOME</key>")
}

fn write_preview(plan: &InstallPlan, options: &InstallOptions, lines: &mut Vec<String>) -> KairosResult<()> {
    let Some(directory) = &options.preview_dir else {
        return Ok(());
    };
    fs::create_dir_all(directory)?;
    for file in &plan.services {
        let Some(name) = file.path.file_name() else {
            continue;
        };
        let path = directory.join(name);
        fs::write(&path, scheduler::encoded(file))?;
        lines.push(format!("미리보기 {}", path.display()));
    }
    Ok(())
}

fn write_files(plan: &InstallPlan, options: &InstallOptions, runner: &Arc<dyn ProcessRunner>) -> KairosResult<()> {
    options.home.ensure()?;
    DirBuilder::new().recursive(true).create_mode(DIR_MODE).create(&plan.log_dir)?;
    set_mode(&plan.log_dir, DIR_MODE)?;
    // launchd가 만들면 644가 되어 doctor가 권한 문제로 본다. 미리 600으로 만든다.
    for name in ["collect.log", "watchdog.log"] {
        let log = plan.log_dir.join(name);
        OpenOptions::new().append(true).create(true).create_mode(LOG_MODE).open(&log)?;
        set_mode(&log, LOG_MODE)?;
    }
    let binary_dir = match &options.binary_dir {
        Some(directory) => directory.clone(),
        None => runtime::current_binary_dir()?,
    };
    runtime::copy_runtime(plan, &options.root, &binary_dir)?;
    allowlist::write_allowlist(&plan.plugin_dir)?;
    runtime::link_current(plan.os, options.home.root(), &plan.version, runner)?;
    fs::create_dir_all(&plan.service_dir)?;
    for service in &plan.services {
        let mut file =
            OpenOptions::new().write(true).create(true).truncate(true).create_mode(0o644).open(&service.path)?;
        std::io::Write::write_all(&mut file, &scheduler::encoded(service))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kairos_core::process::{ProcessOutput, ProcessRequest};

    use super::*;

    const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/install-root");

    /// 호출을 적고 `fail`에 든 첫 인자에만 1을 돌려준다.
    struct Fake {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        fail: Vec<&'static str>,
    }

    impl ProcessRunner for Fake {
        fn run(&self, request: &ProcessRequest) -> ProcessOutput {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((request.program.clone(), request.args.clone()));
            let failed = request.args.iter().any(|arg| self.fail.contains(&arg.as_str()));
            ProcessOutput::exited(i32::from(failed), "")
        }
    }

    fn fake(fail: &[&'static str]) -> Arc<Fake> {
        Arc::new(Fake { calls: Mutex::new(Vec::new()), fail: fail.to_vec() })
    }

    fn args(fake: &Fake) -> Vec<Vec<String>> {
        fake.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(_, args)| args.clone())
            .collect()
    }

    fn plan(os: Os, home: &str, paths: InstallPaths) -> InstallPlan {
        let dir = std::env::temp_dir();
        plan_install(&PlanInput {
            home: &KairosHome::at(home),
            root: Path::new(ROOT),
            paths: &paths,
            service_dir: Some(dir),
            os,
        })
        .unwrap_or_else(|error| panic!("{}", error.code()))
    }

    #[test]
    fn systemd_등록은_다시_읽고_켜고_다시_시작한_뒤_첫_수집을_띄운다() {
        let runner = fake(&[]);
        let installer = Installer::new(runner.clone());
        let mut lines = Vec::new();
        assert!(!installer.register_systemd(&mut lines));
        let user = |rest: &[&str]| {
            std::iter::once("--user").chain(rest.iter().copied()).map(str::to_owned).collect::<Vec<_>>()
        };
        assert_eq!(
            args(&runner),
            [
                user(&["daemon-reload"]),
                user(&["enable", "dev.kairos.collect.timer"]),
                user(&["restart", "dev.kairos.collect.timer"]),
                user(&["start", "--no-block", "dev.kairos.collect.service"]),
                user(&["enable", "dev.kairos.watchdog.timer"]),
                user(&["restart", "dev.kairos.watchdog.timer"]),
            ]
        );
        assert_eq!(
            lines,
            ["dev.kairos.collect: 등록됨", "dev.kairos.collect: 첫 수집을 바로 시작함", "dev.kairos.watchdog: 등록됨"]
        );
    }

    #[test]
    fn systemd_사용자_세션이_없으면_wsl_안내와_함께_실패한다() {
        let runner = fake(&["daemon-reload"]);
        let mut lines = Vec::new();
        assert!(Installer::new(runner.clone()).register_systemd(&mut lines));
        assert_eq!(args(&runner).len(), 1, "다시 읽기에 실패하면 켜지 않는다");
        assert!(lines[0].contains("/etc/wsl.conf") && lines[0].ends_with("(코드 1)"), "{lines:?}");
    }

    #[test]
    fn 작업_스케줄러_등록은_xml로_작업을_만들고_알림_링크를_사용자_범위에_쓴다() {
        let runner = fake(&[]);
        let home = r"C:\Users\synthetic\AppData\Local\kairos";
        let paths = InstallPaths { claude: r"C:\t\claude.exe".into(), gh: r"C:\t\gh.exe".into() };
        let plan = plan(Os::Windows, home, paths);
        let options = InstallOptions { os: Os::Windows, ..InstallOptions::new(KairosHome::at(home), ROOT) };
        let mut lines = Vec::new();
        assert!(!Installer::new(runner.clone()).register_tasks(&plan, &options, &mut lines));
        let calls = args(&runner);
        let xml = std::env::temp_dir().join("dev.kairos.collect.xml");
        assert_eq!(calls[0], ["/Create", "/TN", r"\kairos\dev.kairos.collect", "/XML", &xml.to_string_lossy(), "/F"]);
        assert_eq!(calls[1], ["/Run", "/TN", r"\kairos\dev.kairos.collect"]);
        assert_eq!(calls[2][..3], ["/Create", "/TN", r"\kairos\dev.kairos.watchdog"]);
        let open = &calls[5];
        assert_eq!(open[1], r"HKCU\Software\Classes\kairos-reveal\shell\open\command");
        let current = KairosHome::at(home).root().join("current").join("bin").join("kairos.exe");
        assert_eq!(open[4], format!("\"{}\" reveal --link \"%1\"", current.display()));
        assert!(calls.iter().all(|args| !args.iter().any(|arg| arg.contains("HKLM"))), "관리자 범위에 쓰지 않는다");
        assert_eq!(lines.last().map(String::as_str), Some("알림: 등록됨(누르면 kairos reveal)"));
    }

    #[test]
    fn 작업을_만들지_못하면_실패이고_첫_수집을_띄우지_않는다() {
        let runner = fake(&["/Create"]);
        let home = r"C:\synthetic\kairos";
        let paths = InstallPaths { claude: r"C:\t\claude.exe".into(), gh: r"C:\t\gh.exe".into() };
        let plan = plan(Os::Windows, home, paths);
        let options = InstallOptions { os: Os::Windows, ..InstallOptions::new(KairosHome::at(home), ROOT) };
        let mut lines = Vec::new();
        assert!(Installer::new(runner.clone()).register_tasks(&plan, &options, &mut lines));
        assert!(!args(&runner).iter().any(|args| args[0] == "/Run"));
        assert_eq!(lines[0], "dev.kairos.collect: 등록 실패(코드 1)");
    }
}
