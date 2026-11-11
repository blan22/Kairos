use std::fs::{self, DirBuilder, Permissions};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use kairos_core::home::DIR_MODE;
use kairos_core::{KairosError, KairosResult};

use super::plan::InstallPlan;
use crate::fs_util::remove_if_present;

const EXECUTABLE_MODE: u32 = 0o755;
const BINARIES: [&str; 2] = ["kairos", "kairos-run"];
const SAME_ROOT: &str = "install_same_root";
/// `ops` 안의 알림 헬퍼 폴더. 소스, 빌드 찌꺼기와 함께 있어 번들만 가져간다.
const NOTIFIER_DIR: &str = "notifier";
const NOTIFIER_APP: &str = "KairosNotifier.app";

/// 지금 도는 실행 파일이 있는 폴더. kairos와 kairos-run이 나란히 빌드된다.
pub(super) fn current_binary_dir() -> KairosResult<PathBuf> {
    let current = fs::canonicalize(std::env::current_exe()?)?;
    current.parent().map(Path::to_path_buf).ok_or_else(|| KairosError::new("install_binary_dir_unknown"))
}

/// 실행 파일을 임시 이름으로 복사한 뒤 rename한다. launchd가 3분마다 이 경로를 띄우므로 복사 중인 파일이 실행되면 안 된다.
fn place_binary(source: &Path, target: &Path) -> KairosResult<()> {
    let executable = Permissions::from_mode(EXECUTABLE_MODE);
    // 설치본의 kairos로 다시 설치하면 원본과 대상이 같다. 복사하면 파일이 비워진다.
    if fs::canonicalize(target).is_ok_and(|real| fs::canonicalize(source).is_ok_and(|origin| origin == real)) {
        fs::set_permissions(target, executable)?;
        return Ok(());
    }
    let mut temporary = target.as_os_str().to_owned();
    temporary.push(format!(".{}.tmp", std::process::id()));
    let temporary = PathBuf::from(temporary);
    let placed = fs::copy(source, &temporary)
        .and_then(|_| fs::set_permissions(&temporary, executable))
        .and_then(|()| fs::rename(&temporary, target));
    if placed.is_err() {
        let _ = remove_if_present(&temporary);
    }
    Ok(placed?)
}

/// 두 경로가 실제로 같은 곳인가. 없는 경로는 같지 않다.
fn same_place(a: &Path, b: &Path) -> bool {
    fs::canonicalize(a).is_ok_and(|first| fs::canonicalize(b).is_ok_and(|second| first == second))
}

fn copy_tree(source: &Path, target: &Path) -> KairosResult<()> {
    // 같은 파일 위로 복사하면 내용이 비워진다. 감시자와 수집 모드가 경고 없이 죽는다.
    if same_place(source, target) {
        return Err(KairosError::new(SAME_ROOT));
    }
    let metadata = fs::metadata(source)?;
    if !metadata.is_dir() {
        fs::copy(source, target)?;
        return Ok(());
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let name = entry?.file_name();
        copy_tree(&source.join(&name), &target.join(&name))?;
    }
    fs::set_permissions(target, metadata.permissions())?;
    Ok(())
}

/// 번들을 임시 이름으로 복사한 뒤 바꿔 끼운다. 같은 버전을 다시 설치해도 이전 번들의 파일이 남지 않는다.
/// 남은 파일은 봉인된 리소스 서명을 깨뜨린다.
fn place_bundle(source: &Path, target: &Path) -> KairosResult<()> {
    let sibling = |suffix: &str| {
        let mut name = target.as_os_str().to_owned();
        name.push(format!(".{}.{suffix}", std::process::id()));
        PathBuf::from(name)
    };
    let (fresh, stale) = (sibling("tmp"), sibling("old"));
    let _ = fs::remove_dir_all(&fresh);
    if let Err(error) = copy_tree(source, &fresh) {
        let _ = fs::remove_dir_all(&fresh);
        return Err(error);
    }
    // 폴더는 비어 있지 않은 폴더 위로 rename되지 않는다. 이전 번들을 비켜 놓고 끼운 뒤 지운다.
    let moved_aside = target.exists();
    if moved_aside && let Err(error) = fs::rename(target, &stale) {
        let _ = fs::remove_dir_all(&fresh);
        return Err(error.into());
    }
    if let Err(error) = fs::rename(&fresh, target) {
        if moved_aside {
            let _ = fs::rename(&stale, target);
        }
        let _ = fs::remove_dir_all(&fresh);
        return Err(error.into());
    }
    if moved_aside {
        fs::remove_dir_all(&stale)?;
    }
    Ok(())
}

/// 알림 헬퍼는 번들만 놓는다. Swift 소스, build.sh, `.build`(서명 안 된 실행 파일)는 가져가지 않고, 이전 설치가 남긴 것도 지운다.
fn copy_notifier(source: &Path, target: &Path) -> KairosResult<()> {
    if same_place(source, target) {
        return Err(KairosError::new(SAME_ROOT));
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(target)? {
        let entry = entry?;
        if entry.file_name() == NOTIFIER_APP {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    let bundle = source.join(NOTIFIER_APP);
    // 번들을 빌드하지 않았으면 놓지 않는다. 그동안 Mac 알림은 나가지 않는다.
    if bundle.is_dir() {
        place_bundle(&bundle, &target.join(NOTIFIER_APP))?;
    }
    fs::set_permissions(target, fs::metadata(source)?.permissions())?;
    Ok(())
}

/// `ops`를 복사한다. 알림 헬퍼 폴더만 따로 다룬다.
fn copy_ops(source: &Path, target: &Path) -> KairosResult<()> {
    if same_place(source, target) {
        return Err(KairosError::new(SAME_ROOT));
    }
    let metadata = fs::metadata(source)?;
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == NOTIFIER_DIR && entry.file_type()?.is_dir() {
            copy_notifier(&source.join(&name), &target.join(&name))?;
        } else {
            copy_tree(&source.join(&name), &target.join(&name))?;
        }
    }
    fs::set_permissions(target, metadata.permissions())?;
    Ok(())
}

/// 실행본에 필요한 것만 놓는다. 소스, 시험, 개발 도구, 데이터는 가져가지 않는다.
pub(super) fn copy_runtime(plan: &InstallPlan, root: &Path, binary_dir: &Path) -> KairosResult<()> {
    // 실행본을 건드리기 전에 확인한다. kairos-run이 빠지면 세션이 열릴 때에야 드러난다.
    if BINARIES.iter().any(|name| !binary_dir.join(name).is_file()) {
        return Err(KairosError::new("install_binary_missing"));
    }
    // 설치본의 kairos로 다시 설치하면 원본과 대상이 같다. 원본의 cpSync처럼 아무것도 건드리지 않고 실패한다.
    if same_place(root, &plan.runtime_dir) {
        return Err(KairosError::new(SAME_ROOT));
    }
    DirBuilder::new().recursive(true).mode(DIR_MODE).create(&plan.runtime_dir)?;
    let bin_dir = plan.runtime_dir.join("bin");
    fs::create_dir_all(&bin_dir)?;
    for name in BINARIES {
        place_binary(&binary_dir.join(name), &bin_dir.join(name))?;
    }
    if root.join("ops").exists() {
        copy_ops(&root.join("ops"), &plan.runtime_dir.join("ops"))?;
    }
    if root.join("plugins").exists() {
        copy_tree(&root.join("plugins"), &plan.runtime_dir.join("plugins"))?;
    }
    fs::set_permissions(&plan.watchdog, Permissions::from_mode(EXECUTABLE_MODE))?;
    Ok(())
}

/// `<홈>/current`를 이번 판의 폴더로 바꿔 건다. 호스트 어댑터는 판 번호 없이 `current/bin/kairos`를 부른다.
/// 같은 폴더에 임시 링크를 만든 뒤 rename한다. 읽는 쪽은 옛 판이나 새 판만 본다.
pub(super) fn link_current(home: &Path, version: &str) -> KairosResult<()> {
    let link = home.join("current");
    let temporary = home.join(format!(".current.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&temporary);
    std::os::unix::fs::symlink(version, &temporary)?;
    if let Err(error) = fs::rename(&temporary, &link) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 원본과_대상이_같아도_파일을_비우지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("kairos");
        fs::write(&path, "binary").unwrap();
        place_binary(&path, &path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "binary");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, EXECUTABLE_MODE);
    }

    #[test]
    fn 같은_폴더_위로는_복사하지_않고_내용을_지킨다() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ops/watchdog.zsh");
        fs::create_dir(dir.path().join("ops")).unwrap();
        fs::write(&file, "script").unwrap();
        let tree = dir.path().join("ops");
        assert_eq!(copy_tree(&tree, &tree).unwrap_err().code(), SAME_ROOT);
        assert_eq!(copy_tree(&file, &file).unwrap_err().code(), SAME_ROOT);
        assert_eq!(fs::read_to_string(&file).unwrap(), "script");
    }

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// 폴더 아래 파일의 상대 경로. 정렬해 돌려준다.
    fn files(root: &Path) -> Vec<String> {
        fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(root, &path, found);
                } else {
                    found.push(path.strip_prefix(root).unwrap().to_string_lossy().into_owned());
                }
            }
        }
        let mut found = Vec::new();
        walk(root, root, &mut found);
        found.sort();
        found
    }

    #[test]
    fn 알림_헬퍼는_번들만_놓고_소스와_빌드_찌꺼기는_가져가지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target) = (dir.path().join("src/ops"), dir.path().join("rt/ops"));
        write(&source.join("kairos-watchdog.zsh"), "script");
        for name in ["main.swift", "build.sh", "Info.plist", "make_icon.swift", "README.md", ".gitignore"] {
            write(&source.join("notifier").join(name), "source");
        }
        write(&source.join("notifier/.build/KairosNotifier"), "unsigned");
        write(&source.join("notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier"), "signed");
        write(&source.join("notifier/KairosNotifier.app/Contents/_CodeSignature/CodeResources"), "seal");
        copy_ops(&source, &target).unwrap();
        assert_eq!(
            files(&target),
            [
                "kairos-watchdog.zsh",
                "notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier",
                "notifier/KairosNotifier.app/Contents/_CodeSignature/CodeResources",
            ]
        );
    }

    #[test]
    fn 다시_설치하면_이전_번들과_이전에_복사된_찌꺼기가_남지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target) = (dir.path().join("src/ops"), dir.path().join("rt/ops"));
        write(&source.join("notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier"), "new");
        write(&target.join("notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier"), "old");
        write(&target.join("notifier/KairosNotifier.app/Contents/Resources/Gone.icns"), "old");
        write(&target.join("notifier/.build/KairosNotifier"), "unsigned");
        write(&target.join("notifier/main.swift"), "source");
        copy_ops(&source, &target).unwrap();
        assert_eq!(files(&target), ["notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier"]);
        let installed = target.join("notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier");
        assert_eq!(fs::read_to_string(installed).unwrap(), "new");
    }

    #[test]
    fn 번들이_없으면_알림_헬퍼_폴더는_비어_있다() {
        let dir = tempfile::tempdir().unwrap();
        let (source, target) = (dir.path().join("src/ops"), dir.path().join("rt/ops"));
        write(&source.join("notifier/main.swift"), "source");
        copy_ops(&source, &target).unwrap();
        assert!(files(&target).is_empty());
        assert!(target.join("notifier").is_dir());
    }

    #[test]
    fn 바꿔_놓을_때_임시_파일을_남기지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        fs::write(&source, "new").unwrap();
        fs::write(bin.join("kairos"), "old").unwrap();
        place_binary(&source, &bin.join("kairos")).unwrap();
        assert_eq!(fs::read_to_string(bin.join("kairos")).unwrap(), "new");
        assert_eq!(fs::read_dir(&bin).unwrap().count(), 1);
    }
}
