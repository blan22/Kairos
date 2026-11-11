use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use kairos_core::home::user_home;
use kairos_core::{KairosError, KairosHome, KairosResult};
use regex::Regex;

use super::LABELS;
use super::binary::{FIXED_PATH, is_shim_dir};
use crate::posix;

/// 설치본의 버전. 실행본 폴더 이름이 된다.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([A-Z_]+)\}\}").expect("상수 정규식"));

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallPaths {
    pub claude: String,
    pub gh: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlistFile {
    pub label: String,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallPlan {
    pub version: String,
    pub runtime_dir: PathBuf,
    pub kairos_bin: PathBuf,
    pub watchdog: PathBuf,
    pub log_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub path_value: String,
    pub launch_agents_dir: PathBuf,
    pub plists: Vec<PlistFile>,
}

fn xml_escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `{{KEY}}` 자리를 채운다. 값이 없는 자리표시자가 있으면 실패한다.
/// 실행본이 Node가 아니므로 `NODE` 값은 없다. `{{NODE}}`가 남은 템플릿은 여기서 막힌다.
pub fn render_template(template: &str, values: &BTreeMap<&str, String>) -> KairosResult<String> {
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
        output.push_str(&xml_escape(value));
        copied = whole.end();
    }
    output.push_str(&template[copied..]);
    Ok(output)
}

pub fn default_launch_agents_dir() -> PathBuf {
    user_home().join("Library").join("LaunchAgents")
}

#[derive(Clone, Debug)]
pub struct PlanInput<'a> {
    pub home: &'a KairosHome,
    /// 템플릿이 있는 `ops/`의 부모.
    pub root: &'a Path,
    pub paths: &'a InstallPaths,
    /// None이면 `~/Library/LaunchAgents`다.
    pub launch_agents_dir: Option<PathBuf>,
}

fn path_value(paths: &InstallPaths) -> String {
    let mut dirs: Vec<&str> = Vec::new();
    for dir in [posix::dirname(&paths.claude), posix::dirname(&paths.gh)].into_iter().chain(FIXED_PATH) {
        if !dirs.contains(&dir) && !is_shim_dir(dir) {
            dirs.push(dir);
        }
    }
    dirs.join(":")
}

pub fn plan_install(input: &PlanInput<'_>) -> KairosResult<InstallPlan> {
    let paths = input.paths;
    if !paths.claude.starts_with('/') || !paths.gh.starts_with('/') {
        return Err(KairosError::new("install paths must be absolute"));
    }
    let home = input.home.root().to_string_lossy();
    let runtime_dir = posix::join(&[&home, VERSION]);
    let launch_agents_dir = input.launch_agents_dir.clone().unwrap_or_else(default_launch_agents_dir);
    let path_value = path_value(paths);
    let kairos_bin = posix::join(&[&runtime_dir, "bin", "kairos"]);
    let watchdog = posix::join(&[&runtime_dir, "ops", "kairos-watchdog.zsh"]);
    let plugin_dir = posix::join(&[&runtime_dir, "plugins", "collector"]);
    let log_dir = posix::join(&[&home, "logs"]);
    let mut values = BTreeMap::from([
        ("CLAUDE", paths.claude.clone()),
        ("GH", paths.gh.clone()),
        ("KAIROS_BIN", kairos_bin.clone()),
        ("WATCHDOG", watchdog.clone()),
        ("PLUGIN_DIR", plugin_dir.clone()),
        ("KAIROS_HOME", home.into_owned()),
        ("LOG_DIR", log_dir.clone()),
        ("PATH", path_value.clone()),
    ]);
    let mut plists = Vec::with_capacity(LABELS.len());
    for label in LABELS {
        values.insert("LABEL", label.to_owned());
        let template = fs::read_to_string(input.root.join("ops").join(format!("{label}.plist.template")))?;
        plists.push(PlistFile {
            label: label.to_owned(),
            path: launch_agents_dir.join(format!("{label}.plist")),
            content: render_template(&template, &values)?,
        });
    }
    Ok(InstallPlan {
        version: VERSION.to_owned(),
        runtime_dir: runtime_dir.into(),
        kairos_bin: kairos_bin.into(),
        watchdog: watchdog.into(),
        log_dir: log_dir.into(),
        plugin_dir: plugin_dir.into(),
        path_value,
        launch_agents_dir,
        plists,
    })
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
            path_value(&paths),
            "/opt/homebrew/bin:/Applications/Orca.app/Contents/Resources/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        );
    }
}
