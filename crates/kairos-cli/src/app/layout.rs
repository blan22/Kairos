//! 설치본의 폴더 구조. 세션 거부 목록과 터미널 명령이 이 경로를 쓴다.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const ASSET_DIRS: [&str; 2] = ["ops", "plugins"];

fn holds_assets(dir: &Path) -> bool {
    ASSET_DIRS.iter().all(|name| dir.join(name).is_dir())
}

/// 설치본은 `<root>/bin/kairos`다. 개발 빌드에서는 `<root>`가 target 폴더가 되지만 구조는 같게 만든다.
/// 어긋나면 세션 거부 목록의 kairos 경로가 틀어진다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallLayout {
    root: PathBuf,
    binary: PathBuf,
}

impl InstallLayout {
    /// 실행 파일 경로에서 뽑는다.
    pub fn from_binary(binary: impl Into<PathBuf>) -> Option<Self> {
        let binary = binary.into();
        let root = binary.parent()?.parent()?.to_path_buf();
        Some(Self { root, binary })
    }

    /// 지금 도는 실행 파일. 심볼릭 링크로 불러도 실제 경로를 쓴다.
    pub fn current() -> Option<Self> {
        Self::from_binary(fs::canonicalize(env::current_exe().ok()?).ok()?)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `<root>/bin/kairos-run`.
    pub fn run_binary(&self) -> PathBuf {
        self.root.join("bin").join("kairos-run")
    }

    /// 설치 자료(`ops/`, `plugins/`)가 있는 폴더. 설치본은 `<root>`이고 개발 저장소는 저장소 루트다.
    /// target 폴더가 저장소 밖이면 실행 파일에서 찾지 못하므로 지금 폴더의 조상을 본다.
    pub fn asset_root(&self) -> PathBuf {
        let from_binary = self.binary_asset_root();
        let from_cwd = || {
            let cwd = env::current_dir().ok()?;
            cwd.ancestors().find(|dir| holds_assets(dir)).map(Path::to_path_buf)
        };
        from_binary.or_else(from_cwd).unwrap_or_else(|| self.root.clone())
    }

    /// 실행 파일의 조상에서만 찾은 설치 자료 폴더. 지금 폴더는 보지 않는다.
    /// 묻지 않고 신뢰할 파일(`setup --yes`의 조직 프로필)은 여기서만 고른다. 지금 폴더는 남의 저장소일 수 있다.
    pub fn binary_asset_root(&self) -> Option<PathBuf> {
        self.binary.ancestors().find(|dir| holds_assets(dir)).map(Path::to_path_buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 설치본_구조로_경로를_만든다() {
        let layout = InstallLayout::from_binary("/opt/kairos/0.2.0/bin/kairos").unwrap();
        assert_eq!(layout.root(), Path::new("/opt/kairos/0.2.0"));
        assert_eq!(layout.run_binary(), Path::new("/opt/kairos/0.2.0/bin/kairos-run"));
    }

    #[test]
    fn 개발_빌드는_target_폴더가_루트다() {
        let layout = InstallLayout::from_binary("/synthetic/target/debug/kairos").unwrap();
        assert_eq!(layout.root(), Path::new("/synthetic/target"));
        assert_eq!(layout.run_binary(), Path::new("/synthetic/target/bin/kairos-run"));
    }

    #[test]
    fn 실행_파일_조상에서_설치_자료를_찾는다() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["ops", "plugins", "bin"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let layout = InstallLayout::from_binary(dir.path().join("bin").join("kairos")).unwrap();
        assert_eq!(layout.asset_root(), dir.path());
    }
}
