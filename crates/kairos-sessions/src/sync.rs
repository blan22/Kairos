//! 세션 추적. `worktree ps`로 run 상태를 맞춘다(설계 7.4).

use kairos_agenda::ReadyKind;
use kairos_core::ports::Clock;
use kairos_core::{Instant, KairosError, KairosResult};
use kairos_store::Database;
use serde::Serialize;

use crate::git_host::is_git_selector;
use crate::orca::{LaunchedRun, OrcaClient, OrcaPane, OrcaReply, PsSnapshot};
use crate::run::{FailCode, Run, RunLifecycle, RunRepo, RunState};

/// 터미널을 띄운 뒤 ps에 보여야 하는 시간(설계 7.2).
pub const SESSION_VISIBLE_MS: i64 = 30_000;
/// kairos-run조차 시작하지 않은 채 이만큼 지나면 실패로 내리고 항목을 되돌린다.
pub const SESSION_LOST_MS: i64 = 5 * 60_000;
/// 수락 프로세스가 죽어 선점만 남았다고 보는 시간. worktree create의 시간 제한보다 길다.
pub const CLAIM_STALE_MS: i64 = 5 * 60_000;
/// `git` 세션 호스트는 사람이 터미널을 연다. 이만큼 kairos-run이 뜨지 않으면 실패로 내리고 항목을 되돌린다.
pub const GIT_LAUNCH_WAIT_MS: i64 = 30 * 60_000;
/// 이만큼 일하는 세션이 있으면 한 줄 경고만 낸다(설계 7.4).
pub const WORKING_WARN: usize = 4;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncedRun {
    pub id: String,
    pub item_id: i64,
    pub state: RunState,
    pub ready: Option<ReadyKind>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SyncReport {
    /// ps를 읽지 못하면 false이고 아무 상태도 바꾸지 않는다.
    pub reachable: bool,
    pub runs: Vec<SyncedRun>,
}

/// `Date.parse` 두 값의 차. 읽지 못하는 시각이면 NaN처럼 어떤 비교도 참이 되지 않게 None이다.
fn elapsed(now: Instant, since: Option<&str>) -> Option<i64> {
    Some(now.ms_since(Instant::parse(since?)?))
}

#[derive(Clone, Copy)]
pub struct RunSynchronizer<'a> {
    database: &'a Database,
    orca: &'a OrcaClient,
    clock: &'a dyn Clock,
}

impl<'a> RunSynchronizer<'a> {
    pub const fn new(database: &'a Database, orca: &'a OrcaClient, clock: &'a dyn Clock) -> Self {
        Self { database, orca, clock }
    }

    fn runs(&self) -> RunRepo<'a> {
        RunRepo::new(self.database.connection())
    }

    fn lifecycle(&self) -> RunLifecycle<'a> {
        RunLifecycle::new(self.database)
    }

    fn observe(&self, run: &Run, agent_state: &str, now: &str) -> KairosResult<()> {
        let waiting = matches!(agent_state, "waiting" | "blocked");
        let finished = agent_state == "done";
        self.database.transaction(|connection| {
            let runs = RunRepo::new(connection);
            let state = if waiting || finished { RunState::WaitingUser } else { RunState::Started };
            runs.observe(&run.id, state, now)?;
            // 늦게 떴을 뿐이다. 확인 카드를 거둬 진짜 준비됨이 나오게 한다.
            if run.ready_kind == Some(ReadyKind::Attention) {
                runs.clear_ready(&run.id)?;
            }
            if waiting {
                runs.mark_ready(&run.id, ReadyKind::Question, now)?;
            }
            if finished {
                runs.mark_ready(&run.id, ReadyKind::Done, now)?;
            }
            Ok(())
        })
    }

    fn terminal_gone(&self, run: &Run) -> bool {
        let (Some(selector), Some(pane_key)) = (&run.worktree_identity, &run.pane_key) else {
            return false;
        };
        match self.orca.terminal_list(selector) {
            OrcaReply::Refused { code } => code == "selector_not_found",
            OrcaReply::Ok(result) => OrcaPane::find(&result, pane_key).is_none_or(|pane| !pane.connected),
            _ => false,
        }
    }

    fn sync_run(&self, row: Run, ps: &PsSnapshot, now: Instant) -> KairosResult<()> {
        let at = now.to_iso();
        let mut run = row;
        if run.pane_key.is_none() {
            let launched = LaunchedRun {
                id: &run.id,
                worktree_identity: run.worktree_identity.as_deref(),
                worktree_id: run.worktree_id.as_deref(),
            };
            if let Some(terminal) = self.orca.find_launched(launched, Some(ps)) {
                let (pane_key, handle) = (terminal.pane_key.as_deref(), terminal.handle.as_deref());
                self.runs().attach_terminal(&run.id, pane_key, handle)?;
                run = self.runs().get(&run.id)?.ok_or_else(|| KairosError::new("run_missing"))?;
            }
        }
        let agent_state = run.pane_key.as_ref().and_then(|pane_key| ps.agents.get(pane_key));
        if let Some(agent_state) = agent_state {
            return self.observe(&run, agent_state, &at);
        }
        if ps.truncated {
            return Ok(());
        }
        if run.seen_at.is_none() {
            let waited = elapsed(now, run.launched_at.as_deref());
            let past = |limit: i64| waited.is_some_and(|waited| waited >= limit);
            if run.state == RunState::Claimed && past(SESSION_VISIBLE_MS) {
                self.database.transaction(|connection| {
                    let runs = RunRepo::new(connection);
                    runs.flag_attention(&run.id)?;
                    runs.mark_ready(&run.id, ReadyKind::Attention, &at)
                })?;
            } else if run.state == RunState::NeedsAttention && run.session_started_at.is_none() && past(SESSION_LOST_MS)
            {
                self.lifecycle().fail(&run.id, run.item_id, now, Some(FailCode::SessionLost))?;
            }
            return Ok(());
        }
        // 본 뒤에 사라졌다. kairos-run이 끝을 적지 못한 경우와, 직접 띄운 적 없는 기존 세션이다.
        if run.adopted || self.terminal_gone(&run) {
            self.lifecycle().end(&run.id, now)?;
        }
        Ok(())
    }

    /// 패널이 next --json의 runs.active가 0보다 클 때 부른다. 수집 틱에는 넣지 않았다.
    pub fn sync(&self) -> KairosResult<SyncReport> {
        let now = self.clock.now();
        let started = self.runs().active()?;
        for run in &started {
            let stale = elapsed(now, Some(&run.created_at)).is_some_and(|age| age >= CLAIM_STALE_MS);
            if run.launched_at.is_none() && !run.adopted && stale {
                self.lifecycle().fail(&run.id, run.item_id, now, Some(FailCode::ClaimStale))?;
            }
        }
        // git 세션은 Orca ps에 없다. kairos-run이 시작과 끝을 적고, 뜨지 않은 채 오래 지나면 내린다.
        let (git_runs, orca_runs): (Vec<Run>, Vec<Run>) =
            self.runs().active()?.into_iter().partition(|run| is_git_selector(run.worktree_identity.as_deref()));
        for run in git_runs {
            let waited = elapsed(now, run.launched_at.as_deref());
            if run.session_started_at.is_none() && waited.is_some_and(|waited| waited >= GIT_LAUNCH_WAIT_MS) {
                self.lifecycle().fail(&run.id, run.item_id, now, Some(FailCode::SessionLost))?;
            }
        }
        let tracked: Vec<Run> = orca_runs.into_iter().filter(|run| run.launched_at.is_some() || run.adopted).collect();
        let mut reachable = true;
        if !tracked.is_empty() {
            let reply = self.orca.worktree_ps();
            reachable = reply.result().is_some();
            if let Some(result) = reply.result() {
                let ps = PsSnapshot::read(result);
                for run in tracked {
                    self.sync_run(run, &ps, now)?;
                }
            }
        }
        let mut runs = Vec::with_capacity(started.len());
        for run in started {
            if let Some(run) = self.runs().get(&run.id)? {
                runs.push(SyncedRun { id: run.id, item_id: run.item_id, state: run.state, ready: run.ready_kind });
            }
        }
        Ok(SyncReport { reachable, runs })
    }
}
