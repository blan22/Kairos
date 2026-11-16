//! kairos-run의 조립. 몸통은 `kairos_sessions::SessionRunner`다.

use std::path::{Path, PathBuf};
use std::{env, fs};

use kairos_core::KairosResult;
use kairos_core::platform::Os;
use kairos_core::text::{sanitize, sanitize_text};
use kairos_sessions::{
    AfterSession, PrStatus, Run, SESSIONS_UNSUPPORTED, SessionProfile, SessionRunner, SessionSpawner, SessionWorkspace,
    build_path,
};
use kairos_source_github::{GithubConfig, LiveAuthorReader};
use kairos_store::Database;

use crate::app::AppContext;

/// 예전 세션 토큰 파일의 이름. `<KAIROS_HOME>` 바로 아래에 있었다.
const LEGACY_TOKEN_FILE: &str = "session-gh-token";

fn session(context: &AppContext, argv: &[String], spawner: &dyn SessionSpawner) -> KairosResult<i32> {
    // 샌드박스 없이는 띄우지 않는다. 수락과 `sessions on`도 막지만 마지막 관문은 여기다.
    let os = Os::current();
    if !os.supports_sessions() {
        context.console().error(SESSIONS_UNSUPPORTED);
        return Ok(1);
    }
    // 예전 버전이 세션에 넘기던 토큰 파일. 이제 읽지 않으니 남겨 두지 않는다. 링크는 따라가지 않고 일반 파일만 지운다.
    let legacy = context.home().file(LEGACY_TOKEN_FILE);
    if fs::symlink_metadata(&legacy).is_ok_and(|metadata| metadata.is_file()) {
        let _ = fs::remove_file(&legacy);
    }
    let mut profile =
        SessionProfile::new(context.layout()?.root(), context.home().root(), context.user_home()).for_os(os);
    // 세션 터미널은 worktree에서 뜨고 claude는 이 폴더를 물려받는다. 확인하지 못하면 고치는 세션은 뜨지 않는다.
    // `.git`이 있는데 확인하지 못하면 읽는 세션도 git 자리를 막지 못하므로 뜨지 않는다.
    // 세션이 쓸 수 있는 폴더. 세션 뒤의 자동 올리기는 이 아래의 실행 파일을 부르지 않는다.
    let mut guarded = vec![profile.brief_dir("")];
    if let Ok(cwd) = env::current_dir() {
        guarded.push(cwd.clone());
        match SessionWorkspace::discover(&cwd, context.user_home()) {
            Ok(workspace) => {
                guarded.extend(workspace.git_dirs().iter().map(PathBuf::from));
                profile = profile.with_workspace(workspace);
            }
            Err(_) if fs::symlink_metadata(cwd.join(".git")).is_ok() => profile = profile.with_unverified_workspace(),
            Err(_) => {}
        }
    }
    // 수집과 같은 gh 설정이다. 세션 밖에서 사용자 gh로 글을 읽어 세션 입력으로 넘긴다. 시험은 `KAIROS_GH`로 가짜 gh를 준다.
    let github = GithubConfig::from_vars(|key| context.env(key).map(str::to_owned));
    let live = LiveAuthorReader::new(context.runner(), github);
    // gh는 세션을 띄우기 전에 정한다. 세션이 바꿀 수 있는 폴더의 gh는 고르지 않는다.
    let gh = context.gh_bin().filter(|gh| !guarded.iter().any(|root| inside(Path::new(gh), root)));
    let publish = Publish { context, gh, guarded };
    SessionRunner::new(context.home().clone(), context.claude_bin(), &profile, spawner, &live, context.clock())
        .with_after(&publish)
        .run(argv)
}

/// 고치는 세션이 끝나면 `kairos pr`과 같은 경로로 커밋을 검사하고, 사용자가 확인하면 올린다. 사용자 git과 gh 로그인으로 세션 밖에서 돈다.
/// 올리기 전 검사는 `PullRequestPublisher`가 모두 한다. 실패하거나 거절하면 문구만 남기고 패널의 PR 올리기로 다시 하게 한다.
struct Publish<'a> {
    context: &'a AppContext,
    gh: Option<String>,
    guarded: Vec<PathBuf>,
}

/// 확인할 때 보여 주는 커밋 제목과 파일 수.
const SHOWN_SUBJECTS: usize = 10;
const SHOWN_FILES: usize = 20;

/// 세션 뒤의 자동 경로가 쓰는 PATH. 터미널의 PATH는 셸 설정이 worktree 안을 넣었을 수 있어 쓰지 않는다.
/// 시스템 폴더와, 세션 전에 정한 gh의 폴더(gh를 git 자격 증명 도우미로 쓰는 경우)만 둔다.
const FIXED_PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// `path`가 `root` 아래인가. 링크를 풀고, APFS처럼 대소문자를 가리지 않는 파일 시스템에 맞춰 소문자로 비교한다.
fn inside(path: &Path, root: &Path) -> bool {
    let real = |path: &Path| fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let lower = |path: PathBuf| PathBuf::from(path.to_string_lossy().to_lowercase());
    lower(real(path)).starts_with(lower(real(root))) || lower(path.to_path_buf()).starts_with(lower(root.to_path_buf()))
}

impl Publish<'_> {
    fn path(&self) -> String {
        let gh_dir = self.gh.as_deref().and_then(|gh| Path::new(gh).parent()).and_then(Path::to_str);
        match gh_dir {
            Some(dir) if !FIXED_PATH.split(':').any(|entry| entry == dir) => format!("{FIXED_PATH}:{dir}"),
            _ => FIXED_PATH.to_owned(),
        }
    }
}

impl AfterSession for Publish<'_> {
    fn after(&self, database: &Database, run: &Run) -> Option<String> {
        let orca = self.context.orca();
        let publisher = self
            .context
            .pull_requests(database, &orca)
            .with_path(&self.path())
            .without_path_under(&self.guarded)
            .with_gh(self.gh.as_deref().unwrap_or_default());
        let prepared = match publisher.prepare_run(&run.id) {
            Ok(Ok(prepared)) => prepared,
            Ok(Err(outcome)) if outcome.status == PrStatus::NoCommits => {
                return Some("kairos-run: 커밋이 없어 PR을 만들지 않았습니다".to_owned());
            }
            Ok(Err(outcome)) => {
                return Some(format!(
                    "kairos-run: PR을 자동으로 올리지 않았습니다. {} 패널의 PR 올리기로 다시 할 수 있습니다",
                    outcome.message
                ));
            }
            Err(error) => {
                return Some(format!(
                    "kairos-run: PR을 자동으로 올리지 못했습니다({}). 패널의 PR 올리기로 다시 하세요",
                    sanitize(error.code())
                ));
            }
        };
        // 올리는 것은 사용자가 정한다. 종료 코드만으로는 터미널을 닫은 것과 /exit를 가리지 못한다.
        // 세션이 남긴 프로세스는 같은 터미널에 입력을 넣을 수 있다(TIOCSTI). 그래서 y가 아니라 매번 새로 만든 코드를 받는다.
        // 그 프로세스는 터미널 출력을 읽지 못해 코드를 모르고, 한 줄만 읽으니 미리 넣은 입력은 틀린 답이 된다.
        // 터미널이 닫혀 입력이 끝나면 올리지 않는다. 제목과 파일 이름은 세션이 정한 글이라 제어 문자를 걸러 찍는다.
        // 보여 주는 내용은 검사용 저장소에서 구했다. 세션 뒤의 worktree에서 보는 git 화면은 바뀌었을 수 있다.
        let console = self.context.console();
        console.print(&format!("kairos-run: {}에 올릴 커밋 {}", prepared.branch(), prepared.sha()));
        let subjects = prepared.subjects();
        for subject in subjects.iter().take(SHOWN_SUBJECTS) {
            console.print(&format!("  - {}", sanitize_text(subject, SUBJECT_MAX_CHARS)));
        }
        if subjects.len() > SHOWN_SUBJECTS {
            console.print(&format!("  - 외 {}개", subjects.len() - SHOWN_SUBJECTS));
        }
        let changed = prepared.changed();
        console.print(&format!("바뀐 파일 {}개", changed.len()));
        for path in changed.iter().take(SHOWN_FILES) {
            let mark = if build_path(path) { "  (빌드 설정, CI가 실행할 수 있습니다)" } else { "" };
            console.print(&format!("  {}{mark}", sanitize_text(path, SUBJECT_MAX_CHARS)));
        }
        if changed.len() > SHOWN_FILES {
            console.print(&format!("  외 {}개", changed.len() - SHOWN_FILES));
        }
        console.prompt(&format!(
            "push하고 draft PR을 열려면 {}를 입력하세요. 다른 입력은 취소입니다:",
            prepared.confirmation()
        ));
        let answer = console.read_line().ok().flatten().unwrap_or_default();
        if answer.trim() != prepared.confirmation() {
            return Some("kairos-run: 올리지 않았습니다. 패널의 PR 올리기로 올릴 수 있습니다".to_owned());
        }
        let outcome = publisher.push(prepared);
        if !outcome.status.is_ok() {
            return Some(format!(
                "kairos-run: PR을 올리지 못했습니다. {} 패널의 PR 올리기로 다시 할 수 있습니다",
                outcome.message
            ));
        }
        let mut line = format!("kairos-run: {} {}", outcome.message, outcome.url.as_deref().unwrap_or_default());
        if outcome.uncommitted {
            line.push_str(" (커밋하지 않은 변경은 들지 않았습니다)");
        }
        Some(line)
    }
}

/// 찍는 커밋 제목의 길이 상한.
const SUBJECT_MAX_CHARS: usize = 120;

/// 반환값은 종료 코드다. 0 정상, 1 거절이나 세션 실패, 2 사용법 오류.
pub fn run_session(context: &AppContext, argv: &[String], spawner: &dyn SessionSpawner) -> i32 {
    session(context, argv, spawner).unwrap_or_else(|error| {
        context.console().error(&format!("kairos-run 실패: {}", sanitize(error.code())));
        1
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn 링크나_대소문자가_달라도_같은_폴더_아래로_본다() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("Work");
        fs::create_dir_all(real.join("bin")).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(inside(&link.join("bin"), &real));
        assert!(inside(&dir.path().join("work").join("bin"), &real));
        assert!(!inside(&dir.path().join("Workx"), &real));
    }

    #[test]
    fn 자동_경로의_path는_시스템_폴더와_gh_폴더뿐이다() {
        let context = AppContext::new(
            kairos_core::KairosHome::at("/synthetic/kairos-home"),
            std::sync::Arc::new(kairos_core::ports::SystemClock),
            std::sync::Arc::new(kairos_core::process::SystemProcessRunner),
            std::collections::BTreeMap::new(),
        );
        let publish = |gh: Option<&str>| Publish { context: &context, gh: gh.map(str::to_owned), guarded: Vec::new() };
        assert_eq!(publish(Some("/opt/homebrew/bin/gh")).path(), "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin");
        assert_eq!(publish(Some("/usr/bin/gh")).path(), FIXED_PATH);
        assert_eq!(publish(None).path(), FIXED_PATH);
    }
}
