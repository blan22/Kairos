//! run의 타입과 수명. SQL은 `RunRepo`에만 있다.

mod lifecycle;
mod repo;

use kairos_agenda::{Playbook, ReadyKind, RunView};
use kairos_core::string_enum;

pub(crate) use lifecycle::RunLifecycle;
pub(crate) use repo::Adoption;
pub use repo::RunRepo;

string_enum! {
    pub enum RunState {
        Claimed => "claimed",
        Started => "started",
        WaitingUser => "waiting_user",
        NeedsAttention => "needs_attention",
        Ended => "ended",
        Failed => "failed",
    }
}

impl RunState {
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Claimed | Self::Started | Self::WaitingUser | Self::NeedsAttention)
    }
}

string_enum! {
    /// 사용자가 응답을 받지 못하는 경로의 실패. 패널의 실패 카드가 된다.
    pub(crate) enum FailCode {
        ClaimStale => "claim_stale",
        SessionLost => "session_lost",
        NoClaude => "no_claude",
        SessionFailed => "session_failed",
        /// 띄우기 직전에 다시 읽은 글에 T0, T1이 아닌 사람의 글이 있었다.
        OutsiderWriting => "outsider_writing",
        /// 띄우기 직전에 글 전체를 다시 읽지 못했다. 다 봤다고 보일 수 없으면 띄우지 않는다.
        LiveUnverified => "live_unverified",
    }
}

/// runs 표의 한 행. 시각 열은 SQL이 문자열로 비교하는 저장 원문이다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run {
    pub id: String,
    pub item_id: i64,
    pub state: RunState,
    pub worktree_identity: Option<String>,
    pub worktree_id: Option<String>,
    pub pane_key: Option<String>,
    pub terminal_handle: Option<String>,
    pub adopted: bool,
    /// 수락 때 고른 일. 세션은 이 값으로 프롬프트를 고른다.
    pub playbook: Playbook,
    pub created_at: String,
    pub ended_at: Option<String>,
    pub launched_at: Option<String>,
    pub session_started_at: Option<String>,
    pub seen_at: Option<String>,
    pub ready_kind: Option<ReadyKind>,
    pub ready_at: Option<String>,
    pub ready_seen_at: Option<String>,
    pub fail_code: Option<String>,
}

impl Run {
    pub fn view(&self) -> RunView {
        RunView { id: self.id.clone(), state: self.state.as_str().to_owned() }
    }
}
