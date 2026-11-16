use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use kairos_core::home::user_home;
use kairos_core::platform::{self, Os};
use kairos_core::{KairosError, KairosHome, KairosResult};
use regex::Regex;

use super::LABELS;
use super::binary::{fixed_path, is_shim_dir};
use super::scheduler::Scheduler;
use crate::posix;

/// 설치본의 버전. 실행본 폴더 이름이 된다.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([A-Z_]+)\}\}").expect("상수 정규식"));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallPaths {
    pub claude: String,
    pub gh: String,
}

/// 스케줄러에 넘길 파일 하나. launchd는 plist, systemd는 service와 timer, 작업 스케줄러는 작업 XML과 런처 설정이다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ServiceFile {
    pub label: String,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallPlan {
    pub os: Os,
    pub version: String,
    pub runtime_dir: PathBuf,
    pub kairos_bin: PathBuf,
    /// zsh 감시자. macOS만 쓴다. 다른 운영체제는 `kairos watchdog`이 같은 판정을 한다.
    pub watchdog: Option<PathBuf>,
    pub log_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub path_value: String,
    /// launchd는 `~/Library/LaunchAgents`, systemd는 `~/.config/systemd/user`, 작업 스케줄러는 `<데이터 폴더>/tasks`.
    pub service_dir: PathBuf,
    pub services: Vec<ServiceFile>,
}

pub(super) fn xml_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `{{KEY}}` 자리를 채운다. 값이 없는 자리표시자가 있으면 실패한다.
/// 실행본이 Node가 아니므로 `NODE` 값은 없다. `{{NODE}}`가 남은 템플릿은 여기서 막힌다.
pub fn render_template(template: &str, values: &BTreeMap<&str, String>) -> KairosResult<String> {
    render_with(template, values, |value| Ok(xml_escape(value)))
}

/// `render_template`와 같고 값마다 `escape`를 거친다. 넣을 수 없는 값이면 `escape`가 실패한다.
pub(super) fn render_with(
    template: &str,
    values: &BTreeMap<&str, String>,
    escape: impl Fn(&str) -> KairosResult<String>,
) -> KairosResult<String> {
    let mut output = String::with_capacity(template.len());
    let mut copied = 0;
    for captures in PLACEHOLDER.captures_iter(template) {
        let (Some(whole), Some(key)) = (captures.get(0), captures.get(1)) else {
            continue;
        };
        let Some(value) = values.get(key.as_str()) else {
            return Err(KairosError::new(format!("unfilled placeholder: {}", key.as_str())));
        };
        output.push_str(&template[copied..whole.start()]);
        output.push_str(&escape(value)?);
        copied = whole.end();
    }
    output.push_str(&template[copied..]);
    Ok(output)
}

/// 이 운영체제의 스케줄러 파일 폴더. 작업 스케줄러는 파일을 읽어 등록하므로 원본을 데이터 폴더에 둔다.
pub fn default_service_dir(os: Os, home: &KairosHome) -> PathBuf {
    match os {
        Os::MacOs => user_home().join("Library").join("LaunchAgents"),
        Os::Linux => {
            // systemd는 XDG_CONFIG_HOME 아래를 읽는다. 상대 경로는 명세상 무시한다.
            let config = std::env::var("XDG_CONFIG_HOME")
                .ok()
                .filter(|value| value.starts_with('/'))
                .map_or_else(|| user_home().join(".config"), PathBuf::from);
            config.join("systemd").join("user")
        }
        Os::Windows => home.root().join("tasks"),
    }
}

/// `~/Library/LaunchAgents`. 0.4까지의 이름이다.
pub fn default_launch_agents_dir() -> PathBuf {
    user_home().join("Library").join("LaunchAgents")
}

#[derive(Clone, Debug)]
pub struct PlanInput<'a> {
    pub home: &'a KairosHome,
    /// 템플릿이 있는 `ops/`의 부모.
    pub root: &'a Path,
    pub paths: &'a InstallPaths,
    /// None이면 `default_service_dir`다.
    pub service_dir: Option<PathBuf>,
    pub os: Os,
}

/// 경로 조각을 운영체제의 구분자로 잇는다. unix는 `node:path`의 `join`과 같다.
pub(super) fn os_join(os: Os, parts: &[&str]) -> String {
    match os {
        Os::Windows => {
            let filled: Vec<&str> = parts
                .iter()
                .enumerate()
                .map(
                    |(index, part)| {
                        if index == 0 { part.trim_end_matches(['\\', '/']) } else { part.trim_matches(['\\', '/']) }
                    },
                )
                .filter(|part| !part.is_empty())
                .collect();
            filled.join("\\")
        }
        _ => posix::join(parts),
    }
}

/// 경로의 폴더. Windows는 두 구분자를 모두 본다.
pub(super) fn os_dirname(os: Os, path: &str) -> &str {
    match os {
        Os::Windows => path.rfind(['\\', '/']).map_or(".", |index| &path[..index]),
        _ => posix::dirname(path),
    }
}

fn path_value(os: Os, paths: &InstallPaths) -> String {
    let mut dirs: Vec<String> = Vec::new();
    let found = [os_dirname(os, &paths.claude).to_owned(), os_dirname(os, &paths.gh).to_owned()];
    for dir in found.into_iter().chain(fixed_path(os)) {
        if !dirs.contains(&dir) && !is_shim_dir(&dir) {
            dirs.push(dir);
        }
    }
    dirs.join(&os.path_list_separator().to_string())
}

pub fn plan_install(input: &PlanInput<'_>) -> KairosResult<InstallPlan> {
    let (os, paths) = (input.os, input.paths);
    if !platform::is_absolute(os, &paths.claude) || !platform::is_absolute(os, &paths.gh) {
        return Err(KairosError::new("install paths must be absolute"));
    }
    let home = input.home.root().to_string_lossy();
    let runtime_dir = os_join(os, &[&home, VERSION]);
    let service_dir = input.service_dir.clone().unwrap_or_else(|| default_service_dir(os, input.home));
    let path_value = path_value(os, paths);
    let kairos_bin = os_join(os, &[&runtime_dir, "bin", &os.exe("kairos")]);
    let watchdog = (os == Os::MacOs).then(|| posix::join(&[&runtime_dir, "ops", "kairos-watchdog.zsh"]));
    let plugin_dir = os_join(os, &[&runtime_dir, "plugins", "collector"]);
    let log_dir = os_join(os, &[&home, "logs"]);
    let mut values = BTreeMap::from([
        ("CLAUDE", paths.claude.clone()),
        ("GH", paths.gh.clone()),
        ("KAIROS_BIN", kairos_bin.clone()),
        ("PLUGIN_DIR", plugin_dir.clone()),
        ("KAIROS_HOME", home.into_owned()),
        ("LOG_DIR", log_dir.clone()),
        ("PATH", path_value.clone()),
        ("SERVICE_DIR", service_dir.to_string_lossy().into_owned()),
        ("LAUNCHER", os_join(os, &[&runtime_dir, "bin", &os.exe("kairos-launch")])),
    ]);
    if let Some(watchdog) = &watchdog {
        values.insert("WATCHDOG", watchdog.clone());
    }
    let scheduler = Scheduler::for_os(os);
    let mut services = Vec::with_capacity(LABELS.len() * 2);
    for label in LABELS {
        values.insert("LABEL", label.to_owned());
        services.extend(scheduler.render(label, &input.root.join("ops"), &service_dir, &values)?);
    }
    Ok(InstallPlan {
        os,
        version: VERSION.to_owned(),
        runtime_dir: runtime_dir.into(),
        kairos_bin: kairos_bin.into(),
        watchdog: watchdog.map(PathBuf::from),
        log_dir: log_dir.into(),
        plugin_dir: plugin_dir.into(),
        path_value,
        service_dir,
        services,
    })
}

/// 템플릿 파일을 읽는다.
pub(super) fn read_template(path: &Path) -> KairosResult<String> {
    Ok(fs::read_to_string(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> BTreeMap<&'static str, String> {
        BTreeMap::from([("KEY", "a&<b>\"c".to_owned()), ("OTHER", "{{KEY}}".to_owned())])
    }

    #[test]
    fn 자리표시자를_채우고_xml로_이스케이프한다() {
        assert_eq!(render_template("x{{KEY}}y", &values()).unwrap(), "xa&amp;&lt;b&gt;&quot;cy");
    }

    #[test]
    fn 채운_값을_다시_해석하지_않는다() {
        assert_eq!(render_template("{{OTHER}}", &values()).unwrap(), "{{KEY}}");
    }

    #[test]
    fn 값이_없는_자리표시자는_실패한다() {
        let failure = render_template("<string>{{NODE}}</string>", &values()).unwrap_err();
        assert_eq!(failure.code(), "unfilled placeholder: NODE");
    }

    #[test]
    fn path_값은_중복과_shim을_빼고_앞에서부터_둔다() {
        let paths = InstallPaths { claude: "/opt/homebrew/bin/claude".into(), gh: "/x/.volta/bin/gh".into() };
        assert_eq!(
            path_value(Os::MacOs, &paths),
            "/opt/homebrew/bin:/Applications/Orca.app/Contents/Resources/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
        let paths = InstallPaths { claude: "/home/u/.local/bin/claude".into(), gh: "/usr/bin/gh".into() };
        assert_eq!(path_value(Os::Linux, &paths), "/home/u/.local/bin:/usr/bin:/usr/local/bin:/bin:/usr/sbin:/sbin");
    }

    #[test]
    fn windows_경로는_역슬래시로_잇고_세미콜론으로_나눈다() {
        assert_eq!(
            os_join(Os::Windows, &[r"C:\Users\u\AppData\Local\kairos\", "0.4.2", "bin"]),
            r"C:\Users\u\AppData\Local\kairos\0.4.2\bin"
        );
        assert_eq!(os_dirname(Os::Windows, r"C:\Tools\gh.exe"), r"C:\Tools");
        assert_eq!(os_dirname(Os::Windows, "C:/Tools/gh.exe"), "C:/Tools");
        let paths = InstallPaths {
            claude: r"C:\Users\u\.local\bin\claude.exe".into(),
            gh: r"C:\Program Files\GitHub CLI\gh.exe".into(),
        };
        let value = path_value(Os::Windows, &paths);
        assert!(value.starts_with(r"C:\Users\u\.local\bin;C:\Program Files\GitHub CLI;"), "{value}");
    }
}
