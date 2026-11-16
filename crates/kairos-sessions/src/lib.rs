//! 수락 세션. 사용자가 패널에서 시작을 누르면 Orca에 worktree와 터미널을 만들고 제한된 claude 세션을 띄운다(설계 7.2~7.4, 8절).
//!
//! - `OrcaClient` Orca CLI 호출과 응답 읽기. 쓰기 명령의 고정 인자 배열은 여기에만 있다.
//! - `RunRepo`, `Run` runs 표의 쓰기.
//! - `SessionLauncher` 시작, 취소, 이동. `RunSynchronizer` 세션 상태 추적.
//! - `SessionProfile` kairos-run이 쓰는 claude 인자, 환경, 프롬프트. 보안 경계다. qa_issue는 고치는 세션, 나머지는 읽는 세션이다.
//! - `PullRequestPublisher` 고치는 세션이 커밋한 일을 draft PR로 올린다. 세션이 끝난 뒤 kairos-run이 사용자에게 물은 뒤, 또는 사용자가 누르면 부른다. 세션 밖의 검사용 저장소에서 검사하고 올린다.
//! - `SessionWorkspace` 고치는 세션의 작업 공간과 세션이 고치지 못하는 git 경로.
//! - `SessionRunner` 세션 터미널에서 도는 kairos-run 본체. 띄우기 직전에 `LiveAuthorCheck`로 글의 작성자를 다시 보고 그 글을 세션 입력으로 넘긴다.
//!
//! 외부로 나가는 값은 고정 낱말, run ID에서 만든 이름, 정규식을 통과한 이슈 URL, Orca가 준 선택자뿐이다.
//! 이슈 제목과 본문은 인자와 프롬프트에 들어가지 않고 세션 입력 파일로만 간다. SQL은 `RunRepo` 안에만 있다.

mod git_host;
mod keychain;
mod launch;
mod launcher;
mod orca;
mod profile;
mod pull_request;
mod run;
mod runner;
mod sync;
mod workspace;

pub use git_host::{GIT_BIN, GitHostError, GitWorkspaces, GitWorktree, is_git_selector};
pub use keychain::{KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE, Keychain, KeychainRead, SECRET_TOOL_BIN, SECURITY_BIN};
pub use launch::{RUN_ID, launch_command, shell_quote, terminal_title, worktree_name};
pub use launcher::{
    SessionAdmission, SessionLaunch, SessionLauncher, SessionOutcome, SessionRefusal, SessionResult, SessionStatus,
};
pub use orca::{
    ORCA_READ, ORCA_TERMINAL, ORCA_WORKTREE, OrcaClient, OrcaLiveAgent, OrcaPane, OrcaReply, OrcaResult, OrcaTerminal,
    OrcaWorktree, PsSnapshot, find_workspace_named, focus_navigated,
};
pub use profile::{
    CLEARED_ENV, FIX_TOOLS, SESSION_BRIEF_FILE, SESSION_BRIEF_ROOT, SESSION_MARKER_ENV, SESSION_SETTINGS_DIR,
    SESSION_TOOLS, SESSIONS_UNSUPPORTED, SessionCommand, SessionProfile, WrittenBrief, WrittenSettings, session_tools,
};
pub use pull_request::{PrDraft, PrOutcome, PrStatus, PreparedPr, PullRequestPublisher, build_path};
pub use run::{Run, RunRepo, RunState};
pub use runner::{
    AfterSession, ClaimRefusal, LiveAuthorCheck, SessionClaim, SessionRunner, SessionSpawner, SystemSessionSpawner,
};
pub use sync::{
    CLAIM_STALE_MS, RunSynchronizer, SESSION_LOST_MS, SESSION_VISIBLE_MS, SyncReport, SyncedRun, WORKING_WARN,
};
pub use workspace::SessionWorkspace;
