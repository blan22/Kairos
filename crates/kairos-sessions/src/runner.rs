//! kairos-run: 수락한 run의 Claude 세션을 플레이북의 프로필로 띄운다(설계 7.3, 8절).
//! 인자는 run UUID 하나다. 이슈 URL은 터미널 명령줄을 거치지 않고 DB에서 읽어 다시 검사한다.

use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use kairos_agenda::{ItemRepo, ItemState, Playbook, playbook_of};
use kairos_core::ports::Clock;
#[cfg(unix)]
use kairos_core::process::{ProcessRequest, ProcessRunner, SystemProcessRunner};
use kairos_core::text::sanitize;
use kairos_core::{Instant, KairosHome, KairosResult, string_enum};
use kairos_ledger::{ObjectRepo, OutsiderMarkRepo};
use kairos_source_github::{LiveAuthorReader, LiveAuthors, LiveRead};
use kairos_store::Database;
#[cfg(unix)]
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
#[cfg(unix)]
use signal_hook::iterator::Signals;

use crate::launch::RUN_ID;
use crate::profile::{SessionCommand, SessionProfile};
use crate::run::{FailCode, Run, RunLifecycle, RunRepo, RunState};

const CHILD_POLL: Duration = Duration::from_millis(20);
#[cfg(unix)]
const SIGNAL_TIMEOUT: Duration = Duration::from_secs(5);
const NOT_LAUNCHED: i32 = 127;
/// `--settings` 파일을 안전하게 쓰지 못했거나 작업 공간을 확인하지 못해 띄우지 않았다. claude가 없는 것과 구분해 session_failed가 된다.
const NOT_PREPARED: i32 = 126;

const BAD_ID: &str = "kairos-run: run ID 형식이 아닙니다";
const NO_CLAUDE: &str = "kairos-run: claude를 찾지 못했습니다";
const OUTSIDER: &str = "kairos-run: 외부 사람의 글이 있어 세션을 열지 않았습니다";
const UNVERIFIED: &str = "kairos-run: 이슈의 글을 모두 확인하지 못해 세션을 열지 않았습니다";
const UNKNOWN_AUTHOR: &str = "kairos-run: 작성자를 확인할 수 없는 글이 있어 세션을 열지 않았습니다";
const NO_WORKSPACE: &str = "kairos-run: 작업 공간을 확인하지 못해 세션을 시작하지 않습니다";
const BAD_SETTINGS: &str = "kairos-run: 세션 설정 파일을 안전하게 쓰지 못해 세션을 시작하지 않습니다";
const BAD_BRIEF: &str = "kairos-run: 세션 입력 파일을 안전하게 쓰지 못해 세션을 시작하지 않습니다";
const OTHER_FIX_RUNNING: &str = "kairos-run: 다른 고치는 세션이 열려 있어 PR을 자동으로 올리지 않았습니다. 그 세션을 닫은 뒤 패널의 PR 올리기를 누르세요";

string_enum! {
    pub enum ClaimRefusal {
        NoRun => "no_run",
        Used => "used",
        InvalidState => "invalid_state",
        BadUrl => "bad_url",
    }
}

impl ClaimRefusal {
    const fn message(self) -> &'static str {
        match self {
            Self::NoRun => "kairos-run: 없는 run입니다",
            Self::Used => "kairos-run: 이미 시작한 run입니다",
            Self::InvalidState => "kairos-run: 시작할 수 없는 상태입니다",
            Self::BadUrl => "kairos-run: 대상 이슈가 조건을 통과하지 못했습니다",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionClaim {
    Won { url: String, run: Box<Run> },
    Refused(ClaimRefusal),
}

/// claude를 띄우기 직전에 세션이 읽을 글의 작성자를 GitHub에서 다시 보고, 그 글을 세션 입력으로 받는다.
/// `playbook_of`는 DB만 본다. 처음 읽을 때 최근 20개 밖에 있던 글과 마지막 수집 뒤의 글은 모른다.
/// 세션은 GitHub에 닿지 않으므로 여기서 판정한 글만 읽는다.
pub trait LiveAuthorCheck {
    /// 글 전체를 읽었다고 보일 수 없으면 오류다. 오류도 세션을 막는다.
    fn live_read(&self, url: &str) -> KairosResult<LiveRead>;
}

impl LiveAuthorCheck for LiveAuthorReader {
    fn live_read(&self, url: &str) -> KairosResult<LiveRead> {
        self.read_brief(url)
    }
}

/// 세션이 끝난 뒤 세션 밖에서 하는 일. 고치는 세션의 커밋을 사용자 자격으로 올린다.
/// 돌려준 문구는 세션 터미널에 찍는다. 실패해도 run은 이미 끝으로 적혀 있고, 패널의 PR 올리기로 다시 할 수 있다.
pub trait AfterSession {
    fn after(&self, database: &Database, run: &Run) -> Option<String>;
}

/// 세션 자식을 띄우고 끝날 때까지 기다려 종료 코드를 준다. 띄우지 못하면 127이다.
/// `command.settings_path()`와 `command.brief_dir()`이 있으면 띄우기 직전에 그 파일을 쓰고, 자식이 끝나거나 띄우지 못하면 지운다. 쓰지 못하면 띄우지 않는다.
pub trait SessionSpawner {
    fn spawn(&self, command: &SessionCommand) -> i32;
}

/// 세션 터미널의 대화형 프로세스라서 stdio를 물려준다. 출력을 파이프로 받는 `ProcessRunner`는 맞지 않는다.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemSessionSpawner;

/// `unsafe` 금지라 kill(2)을 직접 부르지 못한다. 표준 라이브러리의 신호 보내기는 불안정 기능이다.
#[cfg(unix)]
fn forward(pid: u32, signal: i32) {
    let name = if signal == SIGHUP { "HUP" } else { "TERM" };
    let pid = pid.to_string();
    SystemProcessRunner.run(&ProcessRequest::new("/bin/kill", ["-s", name, pid.as_str()], SIGNAL_TIMEOUT));
}

impl SessionSpawner for SystemSessionSpawner {
    fn spawn(&self, command: &SessionCommand) -> i32 {
        // 터미널이 닫혀도 끝을 적을 수 있게 신호를 넘기고 자식을 기다린다.
        // 처리기는 exec에서 기본값으로 돌아가므로 자식의 SIGINT는 그대로 동작한다. SIG_IGN이면 자식이 물려받아 Ctrl-C가 듣지 않는다.
        #[cfg(unix)]
        let mut signals = Signals::new([SIGHUP, SIGTERM, SIGINT]).ok();
        // 버리면 파일을 지운다. 이 함수가 끝날 때까지 쥔다.
        let Ok(_settings) = command.write_settings() else {
            return refuse(BAD_SETTINGS, NOT_PREPARED);
        };
        let Ok(_brief) = command.write_brief() else {
            return refuse(BAD_BRIEF, NOT_PREPARED);
        };
        let spawned = Command::new(&command.file)
            .args(&command.args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn();
        let Ok(mut child) = spawned else {
            return NOT_LAUNCHED;
        };
        loop {
            // 거두기 전의 자식은 pid를 쥐고 있어 다른 프로세스로 신호가 가지 않는다. 그래서 한 스레드에서 번갈아 본다.
            #[cfg(unix)]
            for signal in signals.iter_mut().flat_map(Signals::pending) {
                if signal != SIGINT {
                    forward(child.id(), signal);
                }
            }
            match child.try_wait() {
                Ok(Some(status)) => return status.code().unwrap_or(1),
                Ok(None) => thread::sleep(CHILD_POLL),
                Err(_) => return 1,
            }
        }
    }
}

/// 호출한 쪽이 응답을 받지 못하는 뒤쪽 경로라서 실패 코드를 남긴다. 패널의 실패 카드가 된다.
const fn fail_code(exit_code: i32) -> FailCode {
    if exit_code == NOT_LAUNCHED { FailCode::NoClaude } else { FailCode::SessionFailed }
}

/// 터미널이 닫힌 뒤에도 끝을 적어야 한다. `eprintln!`은 stderr가 사라지면 패닉한다.
fn refuse(message: &str, exit: i32) -> i32 {
    let _ = writeln!(std::io::stderr(), "{message}");
    exit
}

pub struct SessionRunner<'a> {
    home: KairosHome,
    claude_bin: Option<String>,
    profile: &'a SessionProfile,
    spawner: &'a dyn SessionSpawner,
    live: &'a dyn LiveAuthorCheck,
    clock: &'a dyn Clock,
    after: Option<&'a dyn AfterSession>,
}

impl<'a> SessionRunner<'a> {
    /// `claude_bin`은 호출자가 찾은 값이다. 없거나 빈 문자열이면 세션을 실패로 내린다.
    pub fn new(
        home: KairosHome,
        claude_bin: Option<String>,
        profile: &'a SessionProfile,
        spawner: &'a dyn SessionSpawner,
        live: &'a dyn LiveAuthorCheck,
        clock: &'a dyn Clock,
    ) -> Self {
        Self { home, claude_bin: claude_bin.filter(|bin| !bin.is_empty()), profile, spawner, live, clock, after: None }
    }

    /// 고치는 세션이 정상으로 끝나면 부를 일. 없으면 PR은 패널의 버튼으로만 올라간다.
    #[must_use]
    pub fn with_after(mut self, after: &'a dyn AfterSession) -> Self {
        self.after = Some(after);
        self
    }

    /// 같은 클론의 다른 고치는 세션은 공용 .git을 바꿀 수 있다(SECURITY.md의 남은 위험). 하나라도 열려 있으면 자동으로 올리지 않는다.
    fn after_session(&self, database: &Database, run: &Run) -> KairosResult<()> {
        let Some(after) = self.after else {
            return Ok(());
        };
        if run.playbook != Playbook::QaIssue {
            return Ok(());
        }
        // 방금 이 프로세스가 `ended`로 내린 run만 올린다. 세션 중에 취소되거나 실패로 내린 run은 사용자가 올리지 않기로 한 것이다.
        let current = RunRepo::new(database.connection()).get(&run.id)?;
        if current.is_none_or(|current| current.state != RunState::Ended) {
            return Ok(());
        }
        let others = RunRepo::new(database.connection())
            .active()?
            .iter()
            .any(|other| other.id != run.id && other.playbook == Playbook::QaIssue);
        if others {
            refuse(OTHER_FIX_RUNNING, 0);
            return Ok(());
        }
        if let Some(line) = after.after(database, run) {
            refuse(&line, 0);
        }
        Ok(())
    }

    /// 같은 run은 한 번만 뜬다. 늦게 도착한 터미널 명령이나 실패로 내린 run을 여기서 막는다.
    pub fn claim(database: &Database, run_id: &str, now: Instant) -> KairosResult<SessionClaim> {
        database.transaction(|connection| {
            let refused = |code| Ok(SessionClaim::Refused(code));
            let runs = RunRepo::new(connection);
            let Some(run) = runs.get(run_id)? else {
                return refused(ClaimRefusal::NoRun);
            };
            if run.session_started_at.is_some() {
                return refused(ClaimRefusal::Used);
            }
            if !matches!(run.state, RunState::Claimed | RunState::NeedsAttention) || run.adopted {
                return refused(ClaimRefusal::InvalidState);
            }
            let Some(item) =
                ItemRepo::new(connection).get(run.item_id)?.filter(|item| item.state == ItemState::Accepted)
            else {
                return refused(ClaimRefusal::InvalidState);
            };
            let object = ObjectRepo::new(connection).get(&item.object_key)?;
            // 수락 뒤 객체가 바뀌었을 수 있다. 시작 버튼과 같은 조건을 다시 본다.
            if playbook_of(connection, item.reason, ItemState::Open, &item.object_key, object.as_ref())?.is_none() {
                return refused(ClaimRefusal::BadUrl);
            }
            runs.mark_session_started(run_id, &now.to_iso())?;
            let url = object.and_then(|object| object.permalink).unwrap_or_default();
            Ok(SessionClaim::Won { url, run: Box::new(run) })
        })
    }

    /// ps가 한 번도 보지 못한 채 실패로 끝났으면 항목을 되돌린다. 그 밖의 끝은 결과 확인이 필요한 ended다.
    pub fn finish(database: &Database, run: &Run, exit_code: i32, now: Instant) -> KairosResult<()> {
        let current = RunRepo::new(database.connection()).get(&run.id)?;
        let lifecycle = RunLifecycle::new(database);
        if exit_code != 0 && current.is_some_and(|current| current.seen_at.is_none()) {
            lifecycle.fail(&run.id, run.item_id, now, Some(fail_code(exit_code)))
        } else {
            lifecycle.end(&run.id, now)
        }
    }

    /// 선점 뒤, claude 전에 글 전체의 작성자를 다시 본다. 통과하지 못하면 띄우지 않고 종료 코드를 준다.
    /// 시작 표시가 이미 남아 같은 run은 다시 뜨지 않는다. 항목은 다른 실패처럼 되돌린다.
    /// 외부 글을 봤으면 객체에 표시를 남겨 DB 게이트도 막는다. 되돌린 항목에 시작 버튼이 다시 나오지 않는다.
    /// 표시는 지우지 않으므로 수집과 같은 `Actor` 규칙으로 T3인 글(`LiveAuthors::Outsider`)에만 남긴다.
    /// 읽지 못한 경우와 작성자를 알 수 없는 글(지운 계정, 연관 정보 없음)은 표시하지 않고 live_unverified로 내린다. 다음 시작 때 다시 읽는다.
    /// 통과하면 세션 입력을 돌려준다. 막으면 종료 코드다.
    fn verify_live(&self, run: &Run, url: &str) -> KairosResult<Result<String, i32>> {
        let verdict = self.live.live_read(url);
        let database = Database::open(&self.home)?;
        let (code, message) = match verdict.map(|read| (read.authors, read.brief)) {
            Ok((LiveAuthors::Insiders, brief)) => {
                // 읽는 사이에 사용자가 취소했거나 추적이 내린 run은 띄우지 않는다.
                let current = RunRepo::new(database.connection()).get(&run.id)?;
                let active = current.is_some_and(|current| current.state.is_active());
                return Ok(if active { Ok(brief) } else { Err(refuse(ClaimRefusal::InvalidState.message(), 1)) });
            }
            Ok((LiveAuthors::Outsider, _)) => (FailCode::OutsiderWriting, OUTSIDER.to_owned()),
            Ok((LiveAuthors::Unknown, _)) => (FailCode::LiveUnverified, UNKNOWN_AUTHOR.to_owned()),
            Err(error) => (FailCode::LiveUnverified, format!("{UNVERIFIED} ({})", sanitize(error.code()))),
        };
        let now = self.clock.now();
        if code == FailCode::OutsiderWriting
            && let Some(item) = ItemRepo::new(database.connection()).get(run.item_id)?
        {
            OutsiderMarkRepo::new(database.connection()).mark(&item.object_key, &now.to_iso())?;
        }
        RunLifecycle::new(&database).fail(&run.id, run.item_id, now, Some(code))?;
        Ok(Err(refuse(&message, 1)))
    }

    /// `argv`는 프로그램 이름을 뺀 인자다. 반환값은 종료 코드다. 0 정상, 1 거절이나 세션 실패, 2 사용법 오류.
    pub fn run(&self, argv: &[String]) -> KairosResult<i32> {
        let [run_id] = argv else {
            return Ok(refuse(BAD_ID, 2));
        };
        if !RUN_ID.is_match(run_id) {
            return Ok(refuse(BAD_ID, 2));
        }
        // 세션이 오래 돌므로 DB를 잡아 두지 않고 끝날 때 다시 연다.
        let (url, run) = {
            let database = Database::open(&self.home)?;
            match Self::claim(&database, run_id, self.clock.now())? {
                SessionClaim::Refused(code) => return Ok(refuse(code.message(), 1)),
                SessionClaim::Won { run, .. } if self.claude_bin.is_none() => {
                    Self::finish(&database, &run, NOT_LAUNCHED, self.clock.now())?;
                    return Ok(refuse(NO_CLAUDE, 1));
                }
                SessionClaim::Won { url, run } => (url, run),
            }
        };
        // DB를 닫은 뒤에 읽는다. 네트워크를 기다리는 동안 수집 틱의 쓰기를 막지 않는다.
        // 세션에는 GitHub 자격 증명이 없다. 글은 여기서 사용자 gh로 읽어 세션 입력 파일로 넘긴다.
        let brief = match self.verify_live(&run, &url)? {
            Ok(brief) => brief,
            Err(code) => return Ok(code),
        };
        let claude = self.claude_bin.as_deref().unwrap_or_default();
        // 고치는 세션은 작업 공간의 git 경로를 알아야 뜬다. 모르면 띄우지 않고 실패로 내린다.
        // 다른 오류도 run은 실패로 내리고, 문구는 작업 공간 탓으로 돌리지 않고 오류 코드 그대로 올린다.
        let command = match self.profile.command(claude, run_id, run.playbook, &url, brief) {
            Ok(command) => command,
            Err(error) => {
                let database = Database::open(&self.home)?;
                Self::finish(&database, &run, NOT_PREPARED, self.clock.now())?;
                if error.code() == "session_workspace_missing" {
                    return Ok(refuse(NO_WORKSPACE, 1));
                }
                return Err(error);
            }
        };
        let code = self.spawner.spawn(&command);
        let database = Database::open(&self.home)?;
        Self::finish(&database, &run, code, self.clock.now())?;
        // 정상으로 끝난 고치는 세션만 올린다. 터미널이 닫히거나 실패한 세션은 사용자가 확인하고 버튼으로 올린다.
        if code == 0 {
            self.after_session(&database, &run)?;
        }
        Ok(i32::from(code != 0))
    }
}
