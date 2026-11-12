mod allowlist;
mod binary;
mod launchctl;
mod plan;
mod remove;
mod runtime;

use std::fs::{self, DirBuilder, OpenOptions, Permissions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use kairos_core::home::DIR_MODE;
use kairos_core::process::ProcessRunner;
use kairos_core::{KairosHome, KairosResult};

pub use allowlist::collector_allowlist;
pub use binary::{find_binary, find_claude};
use launchctl::Launchctl;
pub use plan::{
    InstallPaths, InstallPlan, PlanInput, PlistFile, VERSION, default_launch_agents_dir, plan_install, render_template,
};
pub use remove::{RemovalPlan, plan_runtime_removal, purge_refusal, remove_data, remove_runtime, runtime_refusal};

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
    const fn describe(self) -> &'static str {
        match self {
            Self::DryRun => "시험(dry-run). 아무것도 쓰지 않음",
            Self::Apply => "파일만 설치. launchctl 등록 안 함",
            Self::Register => "설치 후 launchctl 등록",
        }
    }
}

#[derive(Clone, Debug)]
pub struct InstallOptions {
    pub home: KairosHome,
    /// `ops/`와 `plugins/`의 복사 원본 루트. 템플릿도 여기서 읽는다.
    pub root: PathBuf,
    pub mode: InstallMode,
    /// None이면 `~/Library/LaunchAgents`다.
    pub launch_agents_dir: Option<PathBuf>,
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
            launch_agents_dir: None,
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

#[derive(Clone, Debug, Default)]
pub struct UninstallOptions {
    /// None이면 `~/Library/LaunchAgents`다.
    pub launch_agents_dir: Option<PathBuf>,
    pub dry_run: bool,
    /// launchctl을 부르지 않고 plist만 지운다.
    pub skip_launchctl: bool,
    /// 실행본이나 데이터까지 지우는 호출의 앞 단계다. 끝의 안내 두 줄(남긴 것)은 호출자가 직접 알리므로 찍지 않고,
    /// 내리지 못한 잡이 아직 올라 있는지 확인해 실패로 알린다. 남은 잡은 지운 경로를 계속 가리킨다.
    pub before_removal: bool,
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
        let launch_agents_dir = options.launch_agents_dir.clone().unwrap_or_else(default_launch_agents_dir);
        let env_path = std::env::var("PATH").unwrap_or_default();
        let paths =
            binary::resolve_paths(options.claude.as_deref(), options.gh.as_deref(), &env_path, &launch_agents_dir)?;
        let plan = plan_install(&PlanInput {
            home: &options.home,
            root: &options.root,
            paths: &paths,
            launch_agents_dir: Some(launch_agents_dir),
        })?;
        let mut lines = vec![
            format!("모드: {}", options.mode.describe()),
            format!("실행본: {}", plan.runtime_dir.display()),
            format!("claude {}", paths.claude),
            format!("gh {}", paths.gh),
            format!("PATH {}", plan.path_value),
        ];
        lines.extend(plan.plists.iter().map(|plist| format!("plist {}", plist.path.display())));

        if options.mode == InstallMode::DryRun {
            write_preview(&plan, options, &mut lines)?;
            lines.push("등록하려면 kairos install --register".to_owned());
            return Ok(InstallResult { plan, mode: options.mode, lines, failed: false });
        }

        write_files(&plan, options)?;
        lines.push("실행본과 plist를 설치했습니다.".to_owned());
        let failed = options.mode == InstallMode::Register && self.register(&plan, options, &mut lines);
        Ok(InstallResult { plan, mode: options.mode, lines, failed })
    }

    /// 실패가 있었으면 참이다.
    fn register(&self, plan: &InstallPlan, options: &InstallOptions, lines: &mut Vec<String>) -> bool {
        let launchctl = Launchctl::new(&self.runner);
        let domain = launchctl.domain();
        let mut failed = false;
        for label in legacy_labels(&plan.launch_agents_dir) {
            let legacy = plan.launch_agents_dir.join(format!("{label}.plist"));
            launchctl.run(&["bootout", &format!("{domain}/{label}")]);
            let removed = remove_if_present(&legacy).is_ok() && !legacy.exists();
            lines.push(format!("{label}: 옛 라벨 {}", if removed { "내림" } else { "내리지 못함" }));
            failed |= !removed;
        }
        for plist in &plan.plists {
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
        }
        failed
    }

    pub fn uninstall(&self, options: &UninstallOptions) -> KairosResult<UninstallResult> {
        let dir = options.launch_agents_dir.clone().unwrap_or_else(default_launch_agents_dir);
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
        Ok(UninstallResult { lines, failed })
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
    for plist in &plan.plists {
        let path = directory.join(format!("{}.plist", plist.label));
        fs::write(&path, &plist.content)?;
        lines.push(format!("미리보기 {}", path.display()));
    }
    Ok(())
}

fn write_files(plan: &InstallPlan, options: &InstallOptions) -> KairosResult<()> {
    options.home.ensure()?;
    DirBuilder::new().recursive(true).mode(DIR_MODE).create(&plan.log_dir)?;
    fs::set_permissions(&plan.log_dir, Permissions::from_mode(DIR_MODE))?;
    // launchd가 만들면 644가 되어 doctor가 권한 문제로 본다. 미리 600으로 만든다.
    for name in ["collect.log", "watchdog.log"] {
        let log = plan.log_dir.join(name);
        OpenOptions::new().append(true).create(true).mode(LOG_MODE).open(&log)?;
        fs::set_permissions(&log, Permissions::from_mode(LOG_MODE))?;
    }
    let binary_dir = match &options.binary_dir {
        Some(directory) => directory.clone(),
        None => runtime::current_binary_dir()?,
    };
    runtime::copy_runtime(plan, &options.root, &binary_dir)?;
    allowlist::write_allowlist(&plan.plugin_dir)?;
    runtime::link_current(options.home.root(), &plan.version)?;
    fs::create_dir_all(&plan.launch_agents_dir)?;
    for plist in &plan.plists {
        let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o644).open(&plist.path)?;
        std::io::Write::write_all(&mut file, plist.content.as_bytes())?;
    }
    Ok(())
}
