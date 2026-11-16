use std::path::Path;
use std::sync::LazyLock;

use kairos_core::platform::{self, Os};
use kairos_core::{KairosError, KairosResult};
use regex::Regex;

use super::COLLECT_LABEL;
use super::plan::{InstallPaths, os_dirname, os_join};
use super::scheduler::Scheduler;

/// 스파이크 결과의 PATH 규칙: 절대 경로만 쓰고 volta와 cargo의 shim 폴더는 넣지 않는다.
static SHIM_DIR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\\/]\.(?:volta|cargo)[\\/]bin$").expect("상수 정규식"));
const MACOS_PATH: [&str; 6] =
    ["/opt/homebrew/bin", "/Applications/Orca.app/Contents/Resources/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"];
const LINUX_PATH: [&str; 5] = ["/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"];
const NOT_FOUND: &str = "claude 또는 gh 실행 파일을 찾지 못했습니다. --claude, --gh로 절대 경로를 주세요";

/// 설치본 PATH의 고정 폴더. 찾은 claude와 gh의 폴더 뒤에 붙는다.
/// Windows는 `SystemRoot`의 시스템 폴더다. 값이 없거나 절대 경로가 아니면 `C:\Windows`로 본다.
pub(super) fn fixed_path(os: Os) -> Vec<String> {
    match os {
        Os::MacOs => MACOS_PATH.iter().map(|dir| (*dir).to_owned()).collect(),
        Os::Linux => LINUX_PATH.iter().map(|dir| (*dir).to_owned()).collect(),
        Os::Windows => {
            let root = std::env::var("SystemRoot")
                .ok()
                .filter(|value| platform::is_absolute(Os::Windows, value))
                .unwrap_or_else(|| r"C:\Windows".to_owned());
            vec![
                os_join(os, &[&root, "System32"]),
                root.clone(),
                os_join(os, &[&root, "System32", "WindowsPowerShell", "v1.0"]),
            ]
        }
    }
}

pub(super) fn is_shim_dir(dir: &str) -> bool {
    SHIM_DIR.is_match(dir)
}

/// 실행 권한은 소유자, 그룹, 기타 중 하나만 봐도 된다. `access(2)`는 `unsafe`라 쓰지 않는다.
fn is_executable_file(path: &Path) -> bool {
    platform::is_executable_file(path)
}

/// 이름으로 찾을 파일 이름들. Windows는 `.exe`를 먼저, npm이 까는 `.cmd`를 다음에 본다.
fn candidate_names(os: Os, name: &str) -> Vec<String> {
    match os {
        Os::Windows => vec![format!("{name}.exe"), format!("{name}.cmd")],
        _ => vec![name.to_owned()],
    }
}

/// Windows의 `.cmd`는 실행 권한 비트가 없으므로 일반 파일이면 된다.
fn runnable(os: Os, path: &Path) -> bool {
    match os {
        Os::Windows if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("cmd")) => {
            std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
        }
        _ => is_executable_file(path),
    }
}

/// `search_path`와 고정 폴더에서 실행 파일을 찾는다.
/// 상대 경로 항목을 받으면 설치한 폴더의 파일이 plist에 들어가고, KAIROS_GH는 토큰을 받는 실행 파일이다.
pub fn find_binary(name: &str, search_path: &str) -> Option<String> {
    find_binary_for(Os::current(), name, search_path)
}

pub(super) fn find_binary_for(os: Os, name: &str, search_path: &str) -> Option<String> {
    let fixed = fixed_path(os);
    platform::split_path_list(os, search_path)
        .map(str::to_owned)
        .chain(fixed)
        .filter(|dir| platform::is_absolute(os, dir) && !is_shim_dir(dir))
        .flat_map(|dir| candidate_names(os, name).into_iter().map(move |file| os_join(os, &[&dir, &file])))
        .find(|candidate| runnable(os, Path::new(candidate)))
}

/// 지금 설치된 수집 잡이 쓰는 claude의 폴더.
///
/// 원본은 지금 도는 node의 폴더를 후보로 썼다. volta 환경에서는 PATH에 shim만 있고 실제 claude가 그 옆에 있어서다.
/// Rust에는 node가 없으므로, 앞선 설치가 찾아서 스케줄러가 실제로 띄워 온 경로를 같은 자리의 후보로 삼는다.
/// 폴더만 잇고 실행 파일 검사는 다시 하므로 node를 올려 경로가 사라졌으면 잡지 않고 실패로 알린다.
fn installed_claude_dir(os: Os, service_dir: &Path) -> Option<String> {
    let claude = Scheduler::for_os(os).installed_value(service_dir, COLLECT_LABEL, "KAIROS_CLAUDE")?;
    Some(os_dirname(os, &claude).to_owned())
}

/// 직접 준 경로도 찾은 경로와 같은 규칙을 지킨다. 틀린 값이 plist에 들어가면 다음 틱까지 드러나지 않는다.
fn check_given(os: Os, path: &str) -> KairosResult<String> {
    if !platform::is_absolute(os, path) {
        return Err(KairosError::new("install paths must be absolute"));
    }
    if is_shim_dir(os_dirname(os, path)) {
        return Err(KairosError::new("install paths must not be shims"));
    }
    if !runnable(os, Path::new(path)) {
        return Err(KairosError::new("install path is not an executable file"));
    }
    Ok(path.to_owned())
}

fn resolve_tool(os: Os, name: &str, given: Option<&str>, search_path: &str) -> KairosResult<String> {
    match given {
        Some(path) => check_given(os, path),
        None => find_binary_for(os, name, search_path).ok_or_else(|| KairosError::new(NOT_FOUND)),
    }
}

/// 후보 순서는 원본처럼 PATH, 실제 claude가 있는 폴더, 고정 폴더다.
fn search_path_with_installed(os: Os, env_path: &str, service_dir: &Path) -> String {
    match installed_claude_dir(os, service_dir) {
        Some(dir) => format!("{env_path}{}{dir}", os.path_list_separator()),
        None => env_path.to_owned(),
    }
}

/// 세션을 띄울 claude. Orca 터미널의 PATH에는 volta shim만 있어 설치된 잡의 폴더까지 봐야 찾는다.
pub fn find_claude(env_path: &str, service_dir: &Path) -> Option<String> {
    let os = Os::current();
    find_binary_for(os, "claude", &search_path_with_installed(os, env_path, service_dir))
}

pub(super) fn resolve_paths(
    os: Os,
    claude: Option<&str>,
    gh: Option<&str>,
    env_path: &str,
    service_dir: &Path,
) -> KairosResult<InstallPaths> {
    let search_path = search_path_with_installed(os, env_path, service_dir);
    Ok(InstallPaths {
        claude: resolve_tool(os, "claude", claude, &search_path)?,
        gh: resolve_tool(os, "gh", gh, &search_path)?,
    })
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::os::unix::fs::OpenOptionsExt;

    use super::*;
    use std::fs;

    const TOOL: &str = "kairos-synthetic-tool";

    fn find_claude_mac(env_path: &str, agents: &Path) -> Option<String> {
        find_binary_for(Os::MacOs, "claude", &search_path_with_installed(Os::MacOs, env_path, agents))
    }

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
        search_path_with_installed(Os::MacOs, env_path, agents)
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
        assert_eq!(find_claude_mac(shim.to_str().unwrap(), &agents).as_deref(), real.to_str());
    }

    #[test]
    fn 설치된_plist의_폴더가_node_폴더_후보를_대신한다() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("image/node/1.2.3/bin");
        executable(&real.join(TOOL), 0o755);
        let agents = dir.path().join("agents");
        installed_plist(&agents, &format!("{}/claude", real.display()));
        let found = resolve_tool(Os::MacOs, TOOL, None, &search("/nonexistent", &agents)).unwrap();
        assert_eq!(found, format!("{}/{TOOL}", real.display()));
    }

    #[test]
    fn plist의_값은_xml로_풀어서_읽는다() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join("agents");
        installed_plist(&agents, "/a&amp;b/bin/claude");
        assert_eq!(installed_claude_dir(Os::MacOs, &agents).as_deref(), Some("/a&b/bin"));
    }

    #[test]
    fn plist가_가리키던_파일이_사라졌으면_잡지_않고_실패한다() {
        let dir = tempfile::tempdir().unwrap();
        let agents = dir.path().join("agents");
        installed_plist(&agents, &format!("{}/gone/claude", dir.path().display()));
        let failure = resolve_tool(Os::MacOs, TOOL, None, &search("", &agents)).unwrap_err();
        assert!(failure.code().contains("--claude"));
    }

    #[test]
    fn plist도_path도_없으면_실패하고_옵션을_안내한다() {
        let dir = tempfile::tempdir().unwrap();
        let failure = resolve_tool(Os::MacOs, TOOL, None, &search("", &dir.path().join("none"))).unwrap_err();
        assert!(failure.code().contains("--gh"));
    }

    #[test]
    fn plist의_폴더가_shim이면_후보가_아니다() {
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join(".volta/bin");
        executable(&shim.join(TOOL), 0o755);
        let agents = dir.path().join("agents");
        installed_plist(&agents, &format!("{}/claude", shim.display()));
        assert_eq!(find_binary_for(Os::MacOs, TOOL, &search("", &agents)), None);
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
        assert_eq!(find_binary_for(Os::MacOs, TOOL, &search_path), Some(format!("{}/{TOOL}", first.display())));
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
        assert_eq!(resolve_tool(Os::MacOs, "claude", Some(&path), "").unwrap(), path);
        assert_eq!(check_given(Os::MacOs, "claude").unwrap_err().code(), "install paths must be absolute");
        let missing = dir.path().join("bin/none");
        for bad in [&plain, &missing] {
            let failure = check_given(Os::MacOs, &bad.to_string_lossy()).unwrap_err();
            assert_eq!(failure.code(), "install path is not an executable file");
        }
        assert_eq!(
            check_given(Os::MacOs, &shim.to_string_lossy()).unwrap_err().code(),
            "install paths must not be shims"
        );
    }

    #[test]
    fn 실행_권한이_없거나_폴더면_찾지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        executable(&dir.path().join(TOOL), 0o644);
        fs::create_dir(dir.path().join("kairos-synthetic-dir")).unwrap();
        let search_path = dir.path().to_string_lossy();
        assert_eq!(find_binary_for(Os::MacOs, TOOL, &search_path), None);
        assert_eq!(find_binary("kairos-synthetic-dir", &search_path), None);
    }
}
