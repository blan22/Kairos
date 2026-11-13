use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::KairosResult;
use crate::platform::{CreateMode, Os, set_mode};

pub const DIR_MODE: u32 = 0o700;
pub const FILE_MODE: u32 = 0o600;

/// 데이터 폴더. 파일 이름과 권한의 약속을 이 객체가 쥔다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KairosHome {
    root: PathBuf,
}

impl KairosHome {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `KAIROS_HOME`이 있으면 그 값, 없으면 운영체제의 기본 폴더(`Os::default_data_dir`)다.
    pub fn from_env() -> Self {
        let override_root = std::env::var("KAIROS_HOME").ok().filter(|value| !crate::js::trim(value).is_empty());
        match override_root {
            Some(root) => Self::at(crate::js::trim(&root)),
            None => Self::at(Os::current().default_data_dir(&user_home())),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub fn db_path(&self) -> PathBuf {
        self.file("kairos.db")
    }

    pub fn state_path(&self) -> PathBuf {
        self.file("state.json")
    }

    pub fn control_path(&self) -> PathBuf {
        self.file("control.json")
    }

    pub fn engine_status_path(&self) -> PathBuf {
        self.file("engine.json")
    }

    /// 데이터 폴더를 품은 git 작업 트리의 뿌리. 있으면 kairos는 돌지 않는다. 수집 기록과 설정이 커밋되어 새는 것을 막는다.
    /// git을 부르지 않고 위로 올라가며 `.git`(폴더나 연결된 worktree의 포인터 파일)을 찾는다.
    /// 사용자 홈에 닿으면 멈춘다. 홈 자체를 dotfiles 저장소로 쓰는 사람이 있고, 기본 데이터 폴더는 홈 아래라서다.
    /// 적힌 경로와 링크를 푼 실제 경로를 모두 본다. 링크 너머의 저장소 안에 쓰는 것도 같은 유출이다.
    /// 아직 없는 폴더는 있는 조상까지 풀고 나머지를 붙인다.
    pub fn enclosing_git_worktree(&self, user_home: &Path) -> Option<PathBuf> {
        let root =
            if self.root.is_absolute() { self.root.clone() } else { std::env::current_dir().ok()?.join(&self.root) };
        let home = fs::canonicalize(user_home).unwrap_or_else(|_| user_home.to_path_buf());
        let find = |root: &Path, home: &Path| {
            for dir in root.ancestors() {
                if dir == home || dir.parent().is_none() {
                    return None;
                }
                if fs::symlink_metadata(dir.join(".git")).is_ok() {
                    return Some(dir.to_path_buf());
                }
            }
            None
        };
        find(&root, user_home).or_else(|| find(&resolved(&root)?, &home))
    }

    /// 폴더를 만들고 권한을 0700으로 고정한다. umask에 맡기지 않는다. Windows는 사용자 프로필의 ACL을 물려받는다.
    pub fn ensure(&self) -> KairosResult<()> {
        fs::create_dir_all(&self.root)?;
        set_mode(&self.root, DIR_MODE)?;
        Ok(())
    }
}

/// 있는 가장 가까운 조상까지 링크를 풀고 남은 이름을 붙인다. 풀 조상이 없으면 None.
fn resolved(path: &Path) -> Option<PathBuf> {
    let mut rest: Vec<&std::ffi::OsStr> = Vec::new();
    for dir in path.ancestors() {
        if let Ok(real) = fs::canonicalize(dir) {
            return Some(rest.iter().rev().fold(real, |acc, name| acc.join(name)));
        }
        rest.push(dir.file_name()?);
    }
    None
}

/// `os.homedir()`. HOME이 없으면 passwd를 본다. 패널과 launchd가 빈 환경으로 불러도 같은 데이터 폴더를 찾아야 한다.
pub fn user_home() -> PathBuf {
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// 내용을 같은 폴더의 임시 파일에 쓰고 rename으로 바꾼다. 읽는 쪽은 옛 내용이나 새 내용만 본다.
pub fn write_file_atomic(path: &Path, content: &str) -> KairosResult<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = dir.join(format!(".{:x}-{sequence:x}.tmp", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).create_mode(FILE_MODE).open(&temporary)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    Ok(written?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::mode_of;

    #[test]
    fn 원자적_쓰기는_0600이고_임시_파일을_남기지_않는다() {
        let dir = std::env::temp_dir().join(format!("kairos-home-{}", std::process::id()));
        let home = KairosHome::at(&dir);
        home.ensure().unwrap();
        write_file_atomic(&home.file("a.json"), "{}\n").unwrap();
        write_file_atomic(&home.file("a.json"), "{\"b\":1}\n").unwrap();
        assert_eq!(fs::read_to_string(home.file("a.json")).unwrap(), "{\"b\":1}\n");
        if cfg!(unix) {
            assert_eq!(mode_of(&fs::metadata(home.file("a.json")).unwrap()), Some(0o600));
            assert_eq!(mode_of(&fs::metadata(&dir).unwrap()), Some(0o700));
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn git_작업_트리_안의_데이터_폴더를_찾는다() {
        let base = std::env::temp_dir().join(format!("kairos-worktree-{}", std::process::id()));
        let (repo, linked, user) = (base.join("repo"), base.join("linked"), base.join("user"));
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(linked.join("data")).unwrap();
        // 연결된 worktree는 `.git`이 포인터 파일이다.
        fs::write(linked.join(".git"), "gitdir: /synthetic/.git/worktrees/x\n").unwrap();
        fs::create_dir_all(&user).unwrap();

        let inside = KairosHome::at(repo.join("state").join("kairos"));
        assert_eq!(inside.enclosing_git_worktree(&user), Some(repo.clone()));
        assert_eq!(KairosHome::at(linked.join("data")).enclosing_git_worktree(&user), Some(linked.clone()));
        assert_eq!(KairosHome::at(user.join("kairos")).enclosing_git_worktree(&user), None);
        // 홈 자체가 저장소(dotfiles)면 보지 않는다.
        assert_eq!(inside.enclosing_git_worktree(&repo), None);
        // 홈 아래의 링크가 저장소 안을 가리키면 실제 위치로 본다. 아직 없는 하위 폴더도 같다.
        // 가리키는 곳이 없는 링크는 ensure가 폴더를 만들지 못해 쓰지 않는다.
        fs::create_dir_all(repo.join("state")).unwrap();
        std::os::unix::fs::symlink(repo.join("state"), user.join("linked")).unwrap();
        let real_repo = fs::canonicalize(&repo).unwrap();
        for data in [user.join("linked"), user.join("linked").join("new").join("kairos")] {
            assert_eq!(KairosHome::at(data).enclosing_git_worktree(&user), Some(real_repo.clone()));
        }
        fs::remove_dir_all(&base).unwrap();
    }
}
