use kairos_agenda::{ItemState, RunView};
use kairos_core::string_enum;
use serde::Serialize;

string_enum! {
    /// 앞의 다섯은 `ActionStatus`와 같은 값이다. 뒤의 값은 수락과 이동에만 나온다.
    /// not_eligible     플레이북 대상이 아니거나 차단 시험 전이다. URL 카드만 준다.
    /// gate_closed      표시 게이트가 닫혀 있다. 사용자가 보지 못한 제안이다.
    /// aborted          수락 도중에 사용자가 끝이나 취소를 눌러 멈췄다.
    /// orca_unavailable Orca를 부르지 못했다.
    /// worktree_failed, checkout_failed, terminal_failed  그 단계에서 실패했고 항목은 되돌렸다.
    /// no_session       이 항목에 이동할 세션이 없다.
    /// session_gone     세션의 터미널이 없어졌다.
    /// move_failed      Orca가 이동하지 않았다.
    pub enum SessionStatus {
        Changed => "changed",
        Unchanged => "unchanged",
        AlreadyClosed => "already_closed",
        NotFound => "not_found",
        InvalidState => "invalid_state",
        NotEligible => "not_eligible",
        GateClosed => "gate_closed",
        Aborted => "aborted",
        OrcaUnavailable => "orca_unavailable",
        WorktreeFailed => "worktree_failed",
        CheckoutFailed => "checkout_failed",
        TerminalFailed => "terminal_failed",
        NoSession => "no_session",
        SessionGone => "session_gone",
        MoveFailed => "move_failed",
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionResult {
    pub id: i64,
    pub status: SessionStatus,
    pub state: Option<ItemState>,
    pub message: String,
}

/// `git` 세션 호스트가 돌려주는 띄울 명령. 호스트가 이 폴더에서 이 인자로 터미널을 연다. 셸을 거치지 않는다.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionLaunch {
    pub cwd: String,
    pub argv: Vec<String>,
    /// 사람이 붙여 넣을 한 줄. 경로는 작은따옴표로 감싼다.
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionOutcome {
    pub result: SessionResult,
    pub run: Option<RunView>,
    /// 표시만 한다. 인자나 프롬프트에 넣지 않는다.
    pub url: Option<String>,
    /// 한 줄 경고.
    pub warning: Option<String>,
    /// `git` 세션 호스트의 띄울 명령. 없으면 키를 내지 않는다(계약 v1의 바이트를 지킨다).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch: Option<SessionLaunch>,
}

impl SessionOutcome {
    pub(crate) fn of(id: i64, status: SessionStatus, state: Option<ItemState>, message: impl Into<String>) -> Self {
        let result = SessionResult { id, status, state, message: message.into() };
        Self { result, run: None, url: None, warning: None, launch: None }
    }

    pub(crate) fn with_run(mut self, run: RunView) -> Self {
        self.run = Some(run);
        self
    }

    pub(crate) fn with_url(mut self, url: Option<String>) -> Self {
        self.url = url;
        self
    }

    pub(crate) fn with_launch(mut self, launch: SessionLaunch) -> Self {
        self.launch = Some(launch);
        self
    }

    pub(crate) fn with_warning(mut self, warning: Option<String>) -> Self {
        self.warning = warning;
        self
    }
}

/// 수락 전에 멈추는 사유. 선점하기 전이라 run이 남지 않는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRefusal {
    status: SessionStatus,
    message: String,
}

impl SessionRefusal {
    pub fn not_eligible(message: impl Into<String>) -> Self {
        Self { status: SessionStatus::NotEligible, message: message.into() }
    }

    pub fn gate_closed(message: impl Into<String>) -> Self {
        Self { status: SessionStatus::GateClosed, message: message.into() }
    }

    pub(crate) const fn status(&self) -> SessionStatus {
        self.status
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }
}
