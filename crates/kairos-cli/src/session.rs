//! kairos-run의 조립. 몸통은 `kairos_sessions::SessionRunner`다.

use std::{env, fs};

use kairos_core::KairosResult;
use kairos_core::platform::Os;
use kairos_core::text::sanitize;
use kairos_sessions::{SESSIONS_UNSUPPORTED, SessionProfile, SessionRunner, SessionSpawner, SessionWorkspace};
use kairos_source_github::{GithubConfig, LiveAuthorReader};

use crate::app::AppContext;

fn session(context: &AppContext, argv: &[String], spawner: &dyn SessionSpawner) -> KairosResult<i32> {
    // 샌드박스 없이는 띄우지 않는다. 수락과 `sessions on`도 막지만 마지막 관문은 여기다.
    let os = Os::current();
    if !os.supports_sessions() {
        context.console().error(SESSIONS_UNSUPPORTED);
        return Ok(1);
    }
    let mut profile =
        SessionProfile::new(context.layout()?.root(), context.home().root(), context.user_home()).for_os(os);
    // 세션 터미널은 worktree에서 뜨고 claude는 이 폴더를 물려받는다. 확인하지 못하면 고치는 세션은 뜨지 않는다.
    // `.git`이 있는데 확인하지 못하면 읽는 세션도 git 자리를 막지 못하므로 뜨지 않는다.
    if let Ok(cwd) = env::current_dir() {
        match SessionWorkspace::discover(&cwd, context.user_home()) {
            Ok(workspace) => profile = profile.with_workspace(workspace),
            Err(_) if fs::symlink_metadata(cwd.join(".git")).is_ok() => profile = profile.with_unverified_workspace(),
            Err(_) => {}
        }
    }
    // 수집과 같은 gh 설정이다. 시험은 `KAIROS_GH`로 가짜 gh를 준다.
    let github = GithubConfig::from_vars(|key| context.env(key).map(str::to_owned));
    let live = LiveAuthorReader::new(context.runner(), github);
    SessionRunner::new(context.home().clone(), context.claude_bin(), &profile, spawner, &live, context.clock())
        .with_keychain(context.keychain())
        .run(argv)
}

/// 반환값은 종료 코드다. 0 정상, 1 거절이나 세션 실패, 2 사용법 오류.
pub fn run_session(context: &AppContext, argv: &[String], spawner: &dyn SessionSpawner) -> i32 {
    session(context, argv, spawner).unwrap_or_else(|error| {
        context.console().error(&format!("kairos-run 실패: {}", sanitize(error.code())));
        1
    })
}
