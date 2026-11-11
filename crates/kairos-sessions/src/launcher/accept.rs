//! 수락의 단계. Orca로 넘어가는 값은 고정 낱말, run ID에서 만든 이름, 정규식을 통과한 이슈 URL, Orca가 준 선택자뿐이다.

use kairos_agenda::{
    ItemRepo, ItemState, ItemTransitions, ReadOnlyRunRepo, ReadyKind, RunView, issue_url_matches, playbook_of, safe_url,
};
use kairos_core::{Instant, KairosResult};
use kairos_ledger::ObjectRepo;
use kairos_store::Connection;

use super::{SessionLaunch, SessionLauncher, SessionOutcome, SessionRefusal, SessionStatus};
use crate::git_host::{GitWorkspaces, git_selector};
use crate::launch::{RUN_ID, launch_command, shell_quote, terminal_title, worktree_name};
use crate::orca::{
    LaunchedRun, OrcaLiveAgent, OrcaReply, OrcaTerminal, OrcaWorktree, PsSnapshot, find_workspace_named,
};
use crate::run::{Adoption, RunLifecycle, RunRepo, RunState};
use crate::sync::WORKING_WARN;
use kairos_core::profile::SessionHost;

const NOT_ELIGIBLE: &str = "세션을 열 수 없는 항목입니다. 원래 앱에서 확인하세요";
const NO_ORCA: &str = "Orca를 부르지 못했습니다";
const NO_WORKTREE: &str = "worktree를 만들지 못했습니다";

enum Claim {
    Lost(SessionOutcome),
    Won { url: String },
}

/// worktree create의 끝. 실패면 항목을 되돌린 결과가 들어 있다.
enum Created {
    Selector(String),
    Failed(SessionOutcome),
}

fn run_view(id: &str, state: RunState) -> RunView {
    RunView { id: id.to_owned(), state: state.as_str().to_owned() }
}

/// 선점 뒤에 사용자가 끝이나 취소를 눌렀는가. 그렇다면 그 결과를 돌려주고 외부 쓰기를 더 하지 않는다.
fn stopped(connection: &Connection, run_id: &str, id: i64) -> KairosResult<Option<SessionOutcome>> {
    let run = RunRepo::new(connection).get(run_id)?;
    let item = ItemRepo::new(connection).get(id)?;
    let state = item.map(|item| item.state);
    if run.is_some_and(|run| run.state.is_active()) && state == Some(ItemState::Accepted) {
        return Ok(None);
    }
    if state == Some(ItemState::Closed) {
        return Ok(Some(SessionOutcome::of(id, SessionStatus::AlreadyClosed, state, "이미 처리됨")));
    }
    let message = "수락을 취소해서 세션을 열지 않았습니다";
    Ok(Some(SessionOutcome::of(id, SessionStatus::Aborted, state, message)))
}

/// 선점한 run 하나의 수락.
struct Attempt<'l, 'a> {
    launcher: &'l SessionLauncher<'a>,
    id: i64,
    run_id: &'l str,
    url: &'l str,
}

impl Attempt<'_, '_> {
    fn connection(&self) -> &Connection {
        self.launcher.database.connection()
    }

    fn fail(&self, status: SessionStatus, message: &str) -> KairosResult<SessionOutcome> {
        let launcher = self.launcher;
        RunLifecycle::new(launcher.database).fail(self.run_id, self.id, launcher.clock.now(), None)?;
        let state = ItemRepo::new(self.connection()).get(self.id)?.map(|item| item.state);
        Ok(SessionOutcome::of(self.id, status, state, message))
    }

    /// 확인과 쓰기를 한 트랜잭션으로 묶는다. 사이에 취소가 끼면 취소한 run을 되살리지 않는다.
    fn commit(&self, write: impl FnOnce(RunRepo<'_>) -> KairosResult<()>) -> KairosResult<Option<SessionOutcome>> {
        self.launcher.database.transaction(|connection| {
            let halted = stopped(connection, self.run_id, self.id)?;
            if halted.is_none() {
                write(RunRepo::new(connection))?;
            }
            Ok(halted)
        })
    }

    fn changed(&self, message: &str, state: RunState) -> SessionOutcome {
        SessionOutcome::of(self.id, SessionStatus::Changed, Some(ItemState::Accepted), message)
            .with_run(run_view(self.run_id, state))
    }

    fn adopt(&self, live: &OrcaLiveAgent) -> KairosResult<SessionOutcome> {
        let identity = format!("id:{}", live.worktree_id);
        let seen_at = self.launcher.clock.now().to_iso();
        let halted = self.commit(|runs| {
            runs.adopt(
                self.run_id,
                &Adoption {
                    worktree_identity: &identity,
                    worktree_id: &live.worktree_id,
                    pane_key: &live.pane_key,
                    terminal_handle: live.handle.as_deref(),
                    seen_at: &seen_at,
                },
            )
        })?;
        Ok(halted.unwrap_or_else(|| self.changed("이미 열린 세션이 있습니다. 이동으로 가세요", RunState::Started)))
    }

    fn create_worktree(&self) -> KairosResult<Created> {
        let orca = self.launcher.orca;
        let name = worktree_name(self.run_id);
        let created = orca.worktree_create(&name, self.url);
        let selector = match &created {
            OrcaReply::Missing => return Ok(Created::Failed(self.fail(SessionStatus::OrcaUnavailable, NO_ORCA)?)),
            OrcaReply::Refused { .. } => {
                return Ok(Created::Failed(self.fail(SessionStatus::WorktreeFailed, NO_WORKTREE)?));
            }
            OrcaReply::Ok(result) => OrcaWorktree::read(result).map(|worktree| worktree.selector),
            OrcaReply::Lost => None,
        };
        if let Some(selector) = selector {
            return Ok(Created::Selector(selector));
        }
        // 응답을 잃어도 만들어졌을 수 있다. 이름을 찾고, 없으면 실패로 둔다.
        let again = orca.reference_find(self.url);
        match again.result().and_then(|result| find_workspace_named(result, &name)) {
            Some(worktree_id) => Ok(Created::Selector(format!("id:{worktree_id}"))),
            None => Ok(Created::Failed(self.fail(SessionStatus::WorktreeFailed, NO_WORKTREE)?)),
        }
    }

    fn launch(&self, worktree: &OrcaWorktree) -> KairosResult<SessionOutcome> {
        let launcher = self.launcher;
        let orca = launcher.orca;
        let command = launch_command(launcher.run_binary, self.run_id)?;
        let title = terminal_title(self.run_id);
        let launched = orca.terminal_create(&worktree.selector, &title, &command);
        let created = launched.result().and_then(OrcaTerminal::read_created);
        let terminal = created.or_else(|| {
            let run = LaunchedRun {
                id: self.run_id,
                worktree_identity: Some(&worktree.selector),
                worktree_id: Some(&worktree.id),
            };
            orca.find_launched(run, None)
        });
        if let Some(terminal) = terminal {
            // kairos-run이 취소한 run을 거절하므로 터미널만 남는다.
            let halted = self.commit(|runs| {
                runs.attach_terminal(self.run_id, terminal.pane_key.as_deref(), terminal.handle.as_deref())
            })?;
            if let Some(halted) = halted {
                return Ok(halted);
            }
            let warning = self.working_warning();
            return Ok(self.changed("세션을 여는 중입니다", RunState::Claimed).with_warning(warning));
        }
        if matches!(launched, OrcaReply::Refused { .. } | OrcaReply::Missing) {
            return self.fail(SessionStatus::TerminalFailed, "터미널을 열지 못했습니다");
        }
        // 떠 있을 수도 있고 늦게 뜰 수도 있다. 다시 만들지 않고 추적에 넘긴다.
        let at = launcher.clock.now().to_iso();
        if let Some(halted) = self.commit(|runs| runs.flag_attention(self.run_id))? {
            return Ok(halted);
        }
        RunRepo::new(self.connection()).mark_ready(self.run_id, ReadyKind::Attention, &at)?;
        Ok(self.changed("세션 확인이 필요합니다", RunState::NeedsAttention))
    }

    fn working_warning(&self) -> Option<String> {
        let reply = self.launcher.orca.worktree_ps();
        let working = PsSnapshot::read(reply.result()?).working;
        (working >= WORKING_WARN).then(|| format!("일하는 세션이 {working}개입니다"))
    }

    fn run(&self) -> KairosResult<SessionOutcome> {
        let launcher = self.launcher;
        // 세션 프롬프트와 --reference에 들어가는 유일한 외부 값이다. 쓰기 직전에 다시 본다.
        if !RUN_ID.is_match(self.run_id) || !issue_url_matches(self.url) {
            return self.fail(SessionStatus::NotEligible, NOT_ELIGIBLE);
        }
        let found = launcher.orca.reference_find(self.url);
        let Some(result) = found.result() else {
            return self.fail(SessionStatus::OrcaUnavailable, NO_ORCA);
        };
        if let Some(live) = OrcaLiveAgent::find(result) {
            return self.adopt(&live);
        }
        if let Some(halted) = stopped(self.connection(), self.run_id, self.id)? {
            return Ok(halted);
        }
        let selector = match self.create_worktree()? {
            Created::Selector(selector) => selector,
            Created::Failed(failed) => return Ok(failed),
        };
        let shown = launcher.orca.worktree_show(&selector);
        let worktree = shown.result().and_then(OrcaWorktree::read);
        let Some(worktree) = worktree.filter(|worktree| worktree.path.is_some() && worktree.head.is_some()) else {
            return self.fail(SessionStatus::CheckoutFailed, "체크아웃을 확인하지 못했습니다");
        };
        // 터미널을 만들면 되돌릴 수 없다. 이 뒤의 worktree는 남지만 세션은 뜨지 않는다.
        let launched_at = launcher.clock.now().to_iso();
        let halted = self.commit(|runs| runs.locate(self.run_id, &worktree.selector, &worktree.id, &launched_at))?;
        if let Some(halted) = halted {
            return Ok(halted);
        }
        self.launch(&worktree)
    }
}

impl Attempt<'_, '_> {
    /// `git` 세션 호스트. worktree를 만들고 띄울 명령을 돌려준다. 터미널은 호스트나 사용자가 연다.
    fn run_git(&self, git: &GitWorkspaces) -> KairosResult<SessionOutcome> {
        if let Some(halted) = stopped(self.connection(), self.run_id, self.id)? {
            return Ok(halted);
        }
        let worktree = match git.create(self.url, self.run_id) {
            Ok(worktree) => worktree,
            Err(error) => return self.fail(SessionStatus::WorktreeFailed, error.message()),
        };
        let launched_at = self.launcher.clock.now().to_iso();
        let selector = git_selector(&worktree.path);
        if let Some(halted) = self.commit(|runs| runs.locate(self.run_id, &selector, "git", &launched_at))? {
            return Ok(halted);
        }
        let cwd = worktree.path.to_string_lossy().into_owned();
        let launch = SessionLaunch {
            command: format!("cd {} && {}", shell_quote(&cwd), launch_command(self.launcher.run_binary, self.run_id)?),
            argv: vec![self.launcher.run_binary.to_owned(), self.run_id.to_owned()],
            cwd,
        };
        let warning = worktree.stale_base.then(|| "기준 브랜치를 받아오지 못해 로컬 값으로 만들었습니다".to_owned());
        Ok(self
            .changed("작업 공간을 만들었습니다. 터미널에서 세션을 여세요", RunState::Claimed)
            .with_launch(launch)
            .with_warning(warning))
    }
}

impl SessionLauncher<'_> {
    /// BEGIN IMMEDIATE 안에서 판정하므로 이긴 쪽은 하나다. runs_active가 마지막 방어선이다.
    fn claim(&self, id: i64, run_id: &str, now: Instant) -> KairosResult<Claim> {
        self.database.transaction(|connection| {
            let lost = |value: SessionOutcome| Ok(Claim::Lost(value));
            let Some(item) = ItemRepo::new(connection).get(id)? else {
                return lost(SessionOutcome::of(id, SessionStatus::NotFound, None, "항목 없음"));
            };
            let state = Some(item.state);
            match item.state {
                ItemState::Closed => {
                    return lost(SessionOutcome::of(id, SessionStatus::AlreadyClosed, state, "이미 처리됨"));
                }
                ItemState::Accepted => {
                    let current = RunRepo::new(connection).latest(id)?;
                    let running = current.as_ref().is_some_and(|run| run.state.is_active());
                    let message = if running {
                        "이미 세션이 있습니다"
                    } else {
                        "세션이 끝났습니다. 되돌리기를 누른 뒤 다시 시작하세요"
                    };
                    let mut value = SessionOutcome::of(id, SessionStatus::Unchanged, state, message);
                    value.run = current.map(|run| run.view());
                    return lost(value);
                }
                ItemState::Parked => {
                    let message = "열린 항목만 수락할 수 있습니다";
                    return lost(SessionOutcome::of(id, SessionStatus::InvalidState, state, message));
                }
                ItemState::Open => {}
            }
            let object = ObjectRepo::new(connection).get(&item.object_key)?;
            let Some(playbook) = playbook_of(connection, item.reason, item.state, &item.object_key, object.as_ref())?
            else {
                let url = safe_url(object.and_then(|object| object.permalink).as_deref());
                return lost(SessionOutcome::of(id, SessionStatus::NotEligible, state, NOT_ELIGIBLE).with_url(url));
            };
            if let Some(sibling) = ReadOnlyRunRepo::new(connection).active_on_object(&item.object_key)? {
                let message = "이 이슈의 세션이 이미 있습니다. 이동으로 가세요";
                let view = RunView { id: sibling.id, state: sibling.state };
                return lost(SessionOutcome::of(id, SessionStatus::Unchanged, state, message).with_run(view));
            }
            RunRepo::new(connection).insert_claimed(run_id, id, playbook, &now.to_iso())?;
            ItemTransitions::new(connection).accept(id, now)?;
            Ok(Claim::Won { url: object.and_then(|object| object.permalink).unwrap_or_default() })
        })
    }

    fn refused(&self, id: i64, refusal: &SessionRefusal) -> KairosResult<SessionOutcome> {
        let connection = self.database.connection();
        let item = ItemRepo::new(connection).get(id)?;
        let object = match &item {
            Some(item) => ObjectRepo::new(connection).get(&item.object_key)?,
            None => None,
        };
        let url = safe_url(object.and_then(|object| object.permalink).as_deref());
        let state = item.map(|item| item.state);
        Ok(SessionOutcome::of(id, refusal.status(), state, refusal.message()).with_url(url))
    }

    /// 시작 버튼. 같은 항목을 몇 번 눌러도 세션은 하나다.
    pub fn accept(&self, id: i64) -> KairosResult<SessionOutcome> {
        if let Some(refusal) = self.admission.admit() {
            return self.refused(id, &refusal);
        }
        let run_id = self.ids.unique();
        match self.claim(id, &run_id, self.clock.now())? {
            Claim::Lost(outcome) => Ok(outcome),
            Claim::Won { url } => {
                let attempt = Attempt { launcher: self, id, run_id: &run_id, url: &url };
                match (kairos_core::profile::current().session_host, self.git) {
                    (SessionHost::Git, Some(git)) => attempt.run_git(git),
                    _ => attempt.run(),
                }
            }
        }
    }
}
