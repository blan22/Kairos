//! `git` 세션 호스트. Orca 없이 로컬 클론에 worktree를 만들고, 띄울 명령을 돌려준다.
//! 터미널은 호스트(VS Code 확장, Claude Code 플러그인)나 사용자가 연다. kairos-run이 뜨면 run이 시작으로 바뀐다.
//!
//! git은 인자 배열로 부르고 `core.hooksPath=/dev/null`로 저장소 훅을 끈다. 클론 경로와 worktree 폴더는 설정에서 오고
//! 검증한 절대 경로만 받는다(kairos-config). worktree는 데이터 폴더 밖이어야 한다. 세션은 데이터 폴더에 쓰지 못한다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kairos_core::process::{ProcessEnd, ProcessRequest, ProcessRunner};
use kairos_core::profile::{self, PlaybookTarget};

use crate::launch::worktree_name;

/// 시험은 `KAIROS_GIT`으로 가짜를 준다.
pub const GIT_BIN: &str = "/usr/bin/git";
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const WORKTREE_TIMEOUT: Duration = Duration::from_secs(60);
/// run의 worktree 선택자 앞에 붙는다. Orca 선택자(`id:`, `path:`)와 겹치지 않는다.
pub const GIT_SELECTOR_PREFIX: &str = "git:";

/// 만든 worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GitWorktree {
    pub path: PathBuf,
    pub branch: String,
    /// fetch에 실패해 로컬의 원격 추적 브랜치로 만들었다.
    pub stale_base: bool,
}

/// 만들지 못한 까닭. 문구는 고정이다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitHostError {
    NoTarget,
    NoClone,
    InsideDataFolder,
    WorktreeFailed,
    GitMissing,
}

impl GitHostError {
    pub const fn message(self) -> &'static str {
        match self {
            Self::NoTarget => "플레이북 대상이 아닌 저장소입니다",
            Self::NoClone => "로컬 클론을 찾지 못했습니다. config.toml의 [hosts.git.repos]를 확인하세요",
            Self::InsideDataFolder => "worktree 폴더가 데이터 폴더 안에 있습니다. 세션이 쓸 수 없습니다",
            Self::WorktreeFailed => "worktree를 만들지 못했습니다",
            Self::GitMissing => "git을 찾지 못했습니다",
        }
    }
}

/// 있는 조상까지 링크를 풀고 나머지 이름을 붙인다. 아직 없는 worktree 폴더도 실제 자리로 비교한다.
/// 풀 수 없는 모양(`..`로 끝나는 경로)은 그대로 둔다.
fn resolved(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = std::fs::canonicalize(existing) {
            return rest.iter().rev().fold(real, |acc, name| acc.join(name));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// worktree 폴더가 데이터 폴더 안인가. 적힌 경로와 실제 경로를 모두 본다.
/// macOS의 `/var`와 `/private/var`처럼 링크로 다른 이름을 주어도 같은 폴더면 안으로 본다.
fn inside(root: &Path, data_folder: &Path) -> bool {
    let (real_root, real_data) = (resolved(root), resolved(data_folder));
    [root, real_root.as_path()].iter().any(|path| path.starts_with(data_folder) || path.starts_with(&real_data))
}

#[derive(Clone)]
pub struct GitWorkspaces {
    runner: Arc<dyn ProcessRunner>,
    bin: String,
    env: BTreeMap<String, String>,
    data_folder: PathBuf,
}

impl GitWorkspaces {
    pub fn new(runner: Arc<dyn ProcessRunner>, bin: impl Into<String>, home: &str, data_folder: &Path) -> Self {
        let env = BTreeMap::from([
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
            ("HOME".to_owned(), home.to_owned()),
            ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
            ("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned()),
        ]);
        Self { runner, bin: bin.into(), env, data_folder: data_folder.to_path_buf() }
    }

    fn git(&self, clone: &Path, args: &[&str], timeout: Duration) -> ProcessEnd {
        let mut argv = vec!["-C", clone.to_str().unwrap_or_default(), "-c", "core.hooksPath=/dev/null"];
        argv.extend_from_slice(args);
        let request = ProcessRequest::new(self.bin.clone(), argv, timeout).env(self.env.clone()).max_output(64 * 1024);
        self.runner.run(&request).end
    }

    fn root_for(clone: &Path) -> PathBuf {
        if let Some(root) = &profile::current().git.worktree_root {
            return root.clone();
        }
        let name = clone.file_name().and_then(|name| name.to_str()).unwrap_or("repo");
        clone.with_file_name(format!("{name}-kairos"))
    }

    /// 이슈 URL의 대상 저장소 클론에 `kairos/<이름>` 브랜치로 worktree를 만든다. 기준은 대상의 기준 브랜치다.
    pub fn create(&self, url: &str, run_id: &str) -> Result<GitWorktree, GitHostError> {
        let profile = profile::current();
        let target: &PlaybookTarget = profile.playbooks.target_for_url(url).ok_or(GitHostError::NoTarget)?;
        let clone = profile.git.repos.get(&target.workspace_repo).ok_or(GitHostError::NoClone)?;
        if !clone.join(".git").exists() {
            return Err(GitHostError::NoClone);
        }
        let root = Self::root_for(clone);
        if inside(&root, &self.data_folder) {
            return Err(GitHostError::InsideDataFolder);
        }
        std::fs::create_dir_all(&root).map_err(|_| GitHostError::WorktreeFailed)?;
        let name = worktree_name(run_id);
        let path = root.join(&name);
        let branch = format!("kairos/{name}");
        let remote_branch = target.base_branch.strip_prefix("origin/").unwrap_or(&target.base_branch);
        let fetched = self.git(clone, &["fetch", "--quiet", "--no-tags", "origin", remote_branch], FETCH_TIMEOUT);
        if fetched == ProcessEnd::Missing {
            return Err(GitHostError::GitMissing);
        }
        let path_text = path.to_str().ok_or(GitHostError::WorktreeFailed)?;
        let added = self.git(
            clone,
            &["worktree", "add", "--quiet", "-b", &branch, path_text, &target.base_branch],
            WORKTREE_TIMEOUT,
        );
        match added {
            ProcessEnd::Exited(0) => Ok(GitWorktree { path, branch, stale_base: fetched != ProcessEnd::Exited(0) }),
            ProcessEnd::Missing => Err(GitHostError::GitMissing),
            _ => Err(GitHostError::WorktreeFailed),
        }
    }
}

/// run에 남기는 선택자.
pub fn git_selector(path: &Path) -> String {
    format!("{GIT_SELECTOR_PREFIX}{}", path.display())
}

pub fn is_git_selector(selector: Option<&str>) -> bool {
    selector.is_some_and(|selector| selector.starts_with(GIT_SELECTOR_PREFIX))
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;

    #[test]
    fn 링크로_다른_이름을_주어도_데이터_폴더_안이면_안으로_본다() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&data, &alias).unwrap();
        assert!(inside(&alias.join("worktrees/new"), &data), "없는 하위 폴더도 실제 자리로 본다");
        assert!(inside(&data.join("w"), &alias));
        assert!(!inside(&dir.path().join("elsewhere/w"), &data));
    }
}
