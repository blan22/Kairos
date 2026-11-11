//! 수락과 이동(설계 7.2~7.4).
//!
//! 순서: run 선점, reference find, worktree create, 체크아웃 확인, terminal create. 항목 조작과 달리 외부 쓰기가 있다.
//! 이동은 `SessionLauncher::move_to`만 한다. 사용자가 눌렀을 때만 불린다.

mod accept;
mod outcome;

use kairos_agenda::{IdSource, ItemRepo, ItemState, ItemTransitions, RandomIds, ReadOnlyRunRepo};
use kairos_core::KairosResult;
use kairos_core::ports::Clock;
use kairos_store::Database;

pub use outcome::{SessionLaunch, SessionOutcome, SessionRefusal, SessionResult, SessionStatus};

use crate::git_host::{GitWorkspaces, is_git_selector};
use crate::orca::{OrcaClient, OrcaPane, OrcaReply, focus_navigated};
use crate::run::{RunLifecycle, RunRepo};

static RANDOM_IDS: RandomIds = RandomIds;

/// 표시 게이트와 차단 시험 잠금을 본다. 통과면 None이다. 판정은 호출자가 쥔다.
pub trait SessionAdmission {
    fn admit(&self) -> Option<SessionRefusal>;
}

impl<F: Fn() -> Option<SessionRefusal>> SessionAdmission for F {
    fn admit(&self) -> Option<SessionRefusal> {
        self()
    }
}

/// 시작, 취소, 이동. 터미널 명령은 `run_binary`와 run ID로만 만든다.
#[derive(Clone, Copy)]
pub struct SessionLauncher<'a> {
    database: &'a Database,
    orca: &'a OrcaClient,
    clock: &'a dyn Clock,
    admission: &'a dyn SessionAdmission,
    run_binary: &'a str,
    ids: &'a dyn IdSource,
    git: Option<&'a GitWorkspaces>,
}

impl<'a> SessionLauncher<'a> {
    pub fn new(
        database: &'a Database,
        orca: &'a OrcaClient,
        clock: &'a dyn Clock,
        admission: &'a dyn SessionAdmission,
        run_binary: &'a str,
    ) -> Self {
        Self { database, orca, clock, admission, run_binary, ids: &RANDOM_IDS, git: None }
    }

    /// `git` 세션 호스트. 프로필의 `sessions.host`가 git이면 수락이 Orca 대신 이것을 쓴다.
    #[must_use]
    pub fn with_git(mut self, git: &'a GitWorkspaces) -> Self {
        self.git = Some(git);
        self
    }

    /// run ID의 출처를 바꾼다. 시험이 고정값을 넣는다.
    #[must_use]
    pub fn with_ids(mut self, ids: &'a dyn IdSource) -> Self {
        self.ids = ids;
        self
    }

    /// 4.2의 사용자 취소. 뜬 세션을 끄지는 않는다. kairos가 터미널을 닫는 경로는 없다.
    pub fn cancel(&self, id: i64) -> KairosResult<SessionOutcome> {
        let now = self.clock.now();
        self.database.transaction(|connection| {
            let Some(item) = ItemRepo::new(connection).get(id)? else {
                return Ok(SessionOutcome::of(id, SessionStatus::NotFound, None, "항목 없음"));
            };
            match item.state {
                ItemState::Closed => {
                    let state = Some(ItemState::Closed);
                    Ok(SessionOutcome::of(id, SessionStatus::AlreadyClosed, state, "이미 처리됨"))
                }
                ItemState::Accepted => {
                    RunRepo::new(connection).mark_item_failed(id, &now.to_iso())?;
                    ItemTransitions::new(connection).release(id, now)?;
                    Ok(SessionOutcome::of(id, SessionStatus::Changed, Some(ItemState::Open), "되돌렸습니다"))
                }
                state => Ok(SessionOutcome::of(id, SessionStatus::InvalidState, Some(state), "수락한 항목이 아닙니다")),
            }
        })
    }

    /// 사용자가 이동을 눌렀을 때만 부른다. handle은 paneKey로 다시 찾는다(설계 7.4). 같은 이슈의 다른 항목이 연 세션으로도 간다.
    pub fn move_to(&self, id: i64) -> KairosResult<SessionOutcome> {
        let connection = self.database.connection();
        let Some(item) = ItemRepo::new(connection).get(id)? else {
            return Ok(SessionOutcome::of(id, SessionStatus::NotFound, None, "항목 없음"));
        };
        let state = Some(item.state);
        let runs = RunRepo::new(connection);
        let run = match ReadOnlyRunRepo::new(connection).active_on_object(&item.object_key)? {
            Some(sibling) => runs.get(&sibling.id)?,
            None => runs.latest(id)?,
        };
        let none = || SessionOutcome::of(id, SessionStatus::NoSession, state, "이동할 세션이 없습니다");
        let Some((run, selector)) = run.and_then(|run| run.worktree_identity.clone().map(|selector| (run, selector)))
        else {
            return Ok(none());
        };
        let moving = |status, message| SessionOutcome::of(id, status, state, message).with_run(run.view());
        if is_git_selector(Some(&selector)) {
            let message = "이 세션은 터미널에서 열었습니다. 그 터미널로 가세요";
            return Ok(moving(SessionStatus::MoveFailed, message));
        }
        let unavailable = "Orca를 부르지 못했습니다";
        let mut handle = run.terminal_handle.clone();
        if let Some(pane_key) = &run.pane_key {
            let listed = self.orca.terminal_list(&selector);
            let Some(result) = listed.result() else {
                return Ok(moving(SessionStatus::OrcaUnavailable, unavailable));
            };
            handle = OrcaPane::find(result, pane_key).map(|pane| pane.handle);
            if handle.is_none() {
                // 없어진 사실을 확인했다. run을 끝내 두어야 패널에 되돌리기가 나온다.
                RunLifecycle::new(self.database).end(&run.id, self.clock.now())?;
                return Ok(moving(SessionStatus::SessionGone, "세션의 터미널이 없습니다"));
            }
        }
        let Some(handle) = handle else {
            return Ok(none());
        };
        Ok(match self.orca.terminal_switch(&handle) {
            OrcaReply::Lost | OrcaReply::Missing => moving(SessionStatus::OrcaUnavailable, unavailable),
            OrcaReply::Ok(result) if focus_navigated(&result) => moving(SessionStatus::Changed, "이동했습니다"),
            _ => moving(SessionStatus::MoveFailed, "이동하지 못했습니다"),
        })
    }
}
