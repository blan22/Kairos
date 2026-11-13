use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use kairos_core::{KairosError, KairosResult};
use regex::Regex;

use super::COLLECT_LABEL;
use super::plan::InstallPaths;
use crate::posix;

/// 스파이크 결과의 PATH 규칙: 절대 경로만 쓰고 volta와 cargo의 shim 폴더는 넣지 않는다.
static SHIM_DIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\\/]\.(?:volta|cargo)[\\/]bin$").expect("상수 정규식"));
pub(super) const FIXED_PATH: [&str; 6] =
    ["/opt/homebrew/bin", "/Applications/Orca.app/Contents/Resources/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"];
static INSTALLED_CLAUDE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<key>KAIROS_CLAUDE</key>[ \t\r\n]*<string>([^<]*)</string>").expect("상수 정규식"));
const NOT_FOUND: &str = "claude 또는 gh 실행 파일을 찾지 못했습니다. --claude, --gh로 절대 경로를 주세요";

pub(super) fn is_shim_dir(dir: &str) -> bool {
    SHIM_DIR.is_match(dir)
}

/// 실행 권한은 소유자, 그룹, 기타 중 하나만 봐도 된다. `access(2)`는 `unsafe`라 쓰지 않는다.
fn is_executable_file(path: &Path) -> bool {
    kairos_core::platform::is_executable_file(path)
}

/// `search_path`와 고정 폴더에서 실행 파일을 찾는다.
/// 상대 경로 항목을 받으면 설치한 폴더의 파일이 plist에 들어가고, KAIROS_GH는 토큰을 받는 실행 파일이다.
pub fn find_binary(name: &str, search_path: &str) -> Option<String> {
    search_path
        .split(':')
        .chain(FIXED_PATH)
        .filter(|dir| dir.starts_with('/') && !is_shim_dir(dir))
        .map(|dir| posix::join(&[dir, name]))
        .find(|candidate| is_executable_file(Path::new(candidate)))
}

fn xml_unescape(value: &str) -> String {
    value.replace("&quot;", "\"").replace("&gt;", ">").replace("&lt;", "<").replace("&amp;", "&")
}

/// 지금 설치된 수집 plist가 쓰는 claude의 폴더.
///
/// 원본은 지금 도는 node의 폴더를 후보로 썼다. volta 환경에서는 PATH에 shim만 있고 실제 claude가 그 옆에 있어서다.
/// Rust에는 node가 없으므로, 앞선 설치가 찾아서 launchd가 실제로 띄워 온 경로를 같은 자리의 후보로 삼는다.
/// 폴더만 잇고 실행 파일 검사는 다시 하므로 node를 올려 경로가 사라졌으면 잡지 않고 실패로 알린다.
fn installed_claude_dir(launch_agents_dir: &Path) -> Option<String> {
    let plist = fs::read_to_string(launch_agents_dir.join(format!("{COLLECT_LABEL}.plist"))).ok()?;
    let value = INSTALLED_CLAUDE.captures(&plist)?.get(1)?.as_str();
    Some(posix::dirname(&xml_unescape(value)).to_owned())
}

/// 직접 준 경로도 찾은 경로와 같은 규칙을 지킨다. 틀린 값이 plist에 들어가면 다음 틱까지 드러나지 않는다.
fn check_given(path: &str) -> KairosResult<String> {
    if !path.starts_with('/') {
        return Err(KairosError::new("install paths must be absolute"));
    }
    if is_shim_dir(posix::dirname(path)) {
        return Err(KairosError::new("install paths must not be shims"));
    }
    if !is_executable_file(Path::new(path)) {
        return Err(KairosError::new("install path is not an executable file"));
    }
    Ok(path.to_owned())
}

fn resolve_tool(name: &str, given: Option<&str>, search_path: &str) -> KairosResult<String> {
    match given {
        Some(path) => check_given(path),
        None => find_binary(name, search_path).ok_or_else(|| KairosError::new(NOT_FOUND)),
    }
}

/// 후보 순서는 원본과 같다. PATH, node 폴더를 대신하는 설치된 폴더, 고정 폴더 차례다.
/// 후보 순서는 원본처럼 PATH, 실제 claude가 있는 폴더, 고정 폴더다.
fn search_path_with_installed(env_path: &str, launch_agents_dir: &Path) -> String {
    match installed_claude_dir(launch_agents_dir) {
        Some(dir) => format!("{env_path}:{dir}"),
        None => env_path.to_owned(),
    }
}

/// 세션을 띄울 claude. Orca 터미널의 PATH에는 volta shim만 있어 설치된 plist의 폴더까지 봐야 찾는다.
pub fn find_claude(env_path: &str, launch_agents_dir: &Path) -> Option<String> {
    find_binary("claude", &search_path_with_installed(env_path, launch_agents_dir))
}

pub(super) fn resolve_paths(
    claude: Option<&str>,
    gh: Option<&str>,
    env_path: &str,
    launch_agents_dir: &Path,
) -> KairosResult<InstallPaths> {
    let search_path = search_path_with_installed(env_path, launch_agents_dir);
    Ok(InstallPaths {
        claude: resolve_tool("claude", claude, &search_path)?,
        gh: resolve_tool("gh", gh, &search_path)?,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::OpenOptionsExt;

    use super::*;

    const TOOL: &str = "kairos-synthetic-tool";

    fn executable(path: &Path, mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::OpenOptions::new().write(true).create(true).truncate(true).mode(mode).open(path).unwrap();
    }

    fn installed_plist(agents: &Path, claude: &str) {
        fs::create_dir_all(agents).unwrap();
        let content = format!(
            "<dict>\n    <key>KAIROS_CLAUDE</key>\n    <string>{claude}</string>\n    <key>KAIROS_GH</key>\n    <string>/x/gh</string>\n</dict>"
        );
        fs::write(agents.join(format!("{COLLECT_LABEL}.plist")), content).unwrap();
    }

    fn search(env_path: &str, agents: &Path) -> String {
        search_path_with_installed(env_path, agents)
    }

    #[test]
    fn 세션은_path에_shim만_있어도_설치된_plist의_claude를_찾는다() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("image/bin/claude");
        executable(&real, 0o755);
        let agents = dir.path().join("agents");
        installed_plist(&agents, real.to_str().unwrap());
        let shim = dir.path().join(".volta/bin");
        executable(&shim.join("claude"), 0o755);
        assert_eq!(find_claude(shim.to_str().unwrap(), &agents).as_deref(), real.to_str());
    }

    #[test]
    fn 설치된_plist의_폴더가_node_폴더_후보를_대신한다() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("image/node/1.2.3/bin");
        executable(&real.join(TOOL), 0o755);
        let agents = dir.path().join("agents");
        installed_plist(&agents, &format!("{}/claude", real.display()));
        let found = resolve_tool(TOOL, None, &search("/nonexistent", &agents)).unwrap();
        assert_eq!(found, format!("{}/{TOOL}", real.display()));
    }

    #[test]
    fn plist의_값은_xml로_풀어서_읽는다() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join("agents");
        installed_plist(&agents, "/a&amp;b/bin/claude");
        assert_eq!(installed_claude_dir(&agents).as_deref(), Some("/a&b/bin"));
    }

    #[test]
    fn plist가_가리키던_파일이_사라졌으면_잡지_않고_실패한다() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join("agents");
        installed_plist(&agents, &format!("{}/gone/claude", dir.path().display()));
        let failure = resolve_tool(TOOL, None, &search("", &agents)).unwrap_err();
        assert!(failure.code().contains("--claude"));
    }

    #[test]
    fn plist도_path도_없으면_실패하고_옵션을_안내한다() {
        let dir = tempfile::tempdir().unwrap();
        let failure = resolve_tool(TOOL, None, &search("", &dir.path().join("none"))).unwrap_err();
        assert!(failure.code().contains("--gh"));
    }

    #[test]
    fn plist의_폴더가_shim이면_후보가_아니다() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join(".volta/bin");
        executable(&shim.join(TOOL), 0o755);
        let agents = dir.path().join("agents");
        installed_plist(&agents, &format!("{}/claude", shim.display()));
        assert_eq!(find_binary(TOOL, &search("", &agents)), None);
    }

    #[test]
    fn path에_있으면_설치된_폴더보다_앞선다() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let installed = dir.path().join("installed");
        executable(&first.join(TOOL), 0o755);
        executable(&installed.join(TOOL), 0o755);
        let agents = dir.path().join("agents");
        installed_plist(&agents, &format!("{}/claude", installed.display()));
        let search_path = search(&first.to_string_lossy(), &agents);
        assert_eq!(find_binary(TOOL, &search_path), Some(format!("{}/{TOOL}", first.display())));
    }

    #[test]
    fn 직접_준_경로는_있는_실행_파일이어야_한다() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("bin/claude");
        executable(&good, 0o755);
        let plain = dir.path().join("bin/plain");
        executable(&plain, 0o644);
        let shim = dir.path().join(".cargo/bin/claude");
        executable(&shim, 0o755);
        let path = good.to_string_lossy();
        assert_eq!(resolve_tool("claude", Some(&path), "").unwrap(), path);
        assert_eq!(check_given("claude").unwrap_err().code(), "install paths must be absolute");
        let missing = dir.path().join("bin/none");
        for bad in [&plain, &missing] {
            let failure = check_given(&bad.to_string_lossy()).unwrap_err();
            assert_eq!(failure.code(), "install path is not an executable file");
        }
        assert_eq!(check_given(&shim.to_string_lossy()).unwrap_err().code(), "install paths must not be shims");
    }

    #[test]
    fn 실행_권한이_없거나_폴더면_찾지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        executable(&dir.path().join(TOOL), 0o644);
        fs::create_dir(dir.path().join("kairos-synthetic-dir")).unwrap();
        let search_path = dir.path().to_string_lossy();
        assert_eq!(find_binary(TOOL, &search_path), None);
        assert_eq!(find_binary("kairos-synthetic-dir", &search_path), None);
    }
}
