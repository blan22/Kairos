//! Orca CLI 호출과 응답 읽기. 쓰기 명령은 `OrcaClient`의 고정 인자 배열뿐이다.
//!
//! 응답 형태는 Orca 1.4.248의 CLI 코드에서 읽은 값이다. 모르는 모양은 None으로 내고 호출한 쪽이 실패로 다룬다.

mod client;
mod replies;

pub(crate) use client::LaunchedRun;
pub use client::{ORCA_READ, ORCA_TERMINAL, ORCA_WORKTREE, OrcaClient, OrcaReply, OrcaResult};
pub use replies::{
    OrcaLiveAgent, OrcaPane, OrcaTerminal, OrcaWorktree, PsSnapshot, find_workspace_named, focus_navigated,
};
