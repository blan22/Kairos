use kairos_agenda::{DisplayGate, KairosControl, SessionApproval, SessionLock};
use kairos_core::KairosResult;
use kairos_core::platform::Os;
use kairos_core::ports::FixedClock;
use kairos_sessions::{OrcaClient, RunSynchronizer, SessionAdmission, SessionLauncher, SessionRefusal};
use kairos_store::Database;
use serde_json::json;

use super::shared::one_id;
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::reply::Reply;

const JSON_ONLY: [OptionSpec; 1] = [flag("json")];
const ACCEPT_OPTIONS: [OptionSpec; 3] = [flag("json"), flag("cancel"), text("from")];

/// 수락의 사용자 쪽 조건. 패널은 게이트가 닫히면 버튼을 그리지 않지만, 낡은 화면과 직접 호출은 여기서 막는다.
/// `control`은 `next_context`가 준 것이라 설정 지문이 다르면 이미 세션이 꺼져 있다.
fn admission(gate: &DisplayGate, control: &KairosControl) -> Option<SessionRefusal> {
    if !gate.open {
        return Some(SessionRefusal::gate_closed(gate.message.as_deref().unwrap_or("표시 게이트가 닫혀 있습니다")));
    }
    if control.sessions_allowed() {
        return None;
    }
    Some(SessionRefusal::not_eligible("차단 시험 전이라 세션을 열지 않습니다. 원래 앱에서 확인하세요"))
}

/// 세션이나 Orca 터미널 안에서 부른 호출. 패널의 워커에는 이 값이 없다. 세션이 세션을 열면 컨펌 없이 일이 번진다.
/// 같은 OS 사용자 안의 일이라 완전한 차단은 아니다. 환경값을 지운 호출은 거부 목록과 설계 8절의 잔여 위험으로 넘긴다.
///
/// Claude Code 호스트(`--from claude-code`)는 사용자의 Claude Code 세션에서 부르므로 `CLAUDECODE`가 있다.
/// 설정에서 그 호스트를 켰을 때만 받는다. kairos 세션 안(`KAIROS_SESSION`)은 어떤 경우에도 거절한다.
fn inside_session(context: &AppContext, from: Option<&str>) -> bool {
    if context.env_set("KAIROS_SESSION") {
        return true;
    }
    let host_session = context.env_set("CLAUDECODE") || context.env_set("ORCA_TERMINAL_HANDLE");
    let claude_code = from == Some("claude-code") && kairos_core::profile::current().hosts.is_enabled("claude-code");
    host_session && !claude_code
}

/// 설치본에서 돌면 `run_binary`가 설치 폴더의 kairos-run이다.
fn launcher<'a>(
    context: &'a AppContext,
    database: &'a Database,
    orca: &'a OrcaClient,
    admit: &'a dyn SessionAdmission,
    run_binary: &'a str,
) -> SessionLauncher<'a> {
    SessionLauncher::new(database, orca, context.clock(), admit, run_binary)
}

fn run_binary(context: &AppContext) -> KairosResult<String> {
    Ok(context.layout()?.run_binary().to_string_lossy().into_owned())
}

pub struct Accept;

impl Command for Accept {
    fn name(&self) -> &'static str {
        "accept"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos accept <항목 ID> [--cancel] [--from claude-code] [--json]")
    }

    /// 시작 버튼. 같은 항목을 몇 번 눌러도 세션은 하나다. --cancel은 4.2의 사용자 취소다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&ACCEPT_OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let id = match one_id(parsed.positionals(), reply) {
            Ok(id) => id,
            Err(code) => return Ok(code),
        };
        let cancel = parsed.flag("cancel");
        if !cancel && inside_session(context, parsed.text("from")) {
            return Ok(reply.fail("session_caller", "세션 안에서는 수락할 수 없습니다. 패널에서 누르세요", 1));
        }
        if !cancel && !Os::current().supports_sessions() {
            return Ok(reply.fail("sessions_unsupported", kairos_sessions::SESSIONS_UNSUPPORTED, 1));
        }
        let database = context.open_db()?;
        let orca = context.orca();
        let binary = run_binary(context)?;
        let value = if cancel {
            launcher(context, &database, &orca, &|| None, &binary).cancel(id)?
        } else {
            let next = context.next_context(&database, context.now())?;
            let admit = || admission(&next.gate, &next.control);
            let git = context.git_workspaces();
            launcher(context, &database, &orca, &admit, &binary).with_git(&git).accept(id)?
        };
        drop(database);
        reply.session(&value)
    }
}

pub struct Move;

impl Command for Move {
    fn name(&self) -> &'static str {
        "move"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos move <항목 ID> [--json]")
    }

    /// 이동 버튼. 시작 버튼이 연 세션 창의 첫 표시를 빼면 이 명령 말고는 어떤 경로도 사용자를 세션으로 옮기지 않는다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let id = match one_id(parsed.positionals(), reply) {
            Ok(id) => id,
            Err(code) => return Ok(code),
        };
        let database = context.open_db()?;
        let orca = context.orca();
        let value = launcher(context, &database, &orca, &|| None, "").move_to(id)?;
        drop(database);
        reply.session(&value)
    }
}

pub struct Pr;

impl Command for Pr {
    fn name(&self) -> &'static str {
        "pr"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos pr <항목 ID> [--json]")
    }

    /// 'PR 올리기' 버튼. 고치는 세션이 로컬에 커밋한 브랜치를 사용자의 git, gh 로그인으로 push하고 draft PR을 만든다.
    /// 세션은 제안만 한다(커밋 제목과 본문). 올리는 일은 사용자가 누를 때 세션 밖에서만 한다. 세션 안에서는 부르지 못한다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let id = match one_id(parsed.positionals(), reply) {
            Ok(id) => id,
            Err(code) => return Ok(code),
        };
        if inside_session(context, None) {
            return Ok(reply.fail("session_caller", "세션 안에서는 PR을 올릴 수 없습니다. 패널에서 누르세요", 1));
        }
        let database = context.open_db()?;
        let orca = context.orca();
        let outcome = context.pull_requests(&database, &orca).publish(id)?;
        drop(database);
        if !outcome.status.is_ok() {
            return Ok(reply.fail(outcome.status.as_str(), &outcome.message, 1));
        }
        if reply.json() {
            return reply.success(json!({ "pr": outcome }));
        }
        let mut line = format!("{} {}", outcome.message, outcome.url.as_deref().unwrap_or_default());
        if outcome.uncommitted {
            line.push_str(" (커밋하지 않은 변경은 들지 않았습니다)");
        }
        context.console().print(&line);
        Ok(0)
    }
}

pub struct Runs;

impl Command for Runs {
    fn name(&self) -> &'static str {
        "runs"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos runs [--json]")
    }

    /// 세션 상태를 worktree ps로 맞춘다. next --json의 runs.active가 0보다 클 때 패널이 부른다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&JSON_ONLY).parse(args)?;
        let at = context.now();
        let database = context.open_db()?;
        let orca = context.orca();
        let clock = FixedClock(at);
        let report = RunSynchronizer::new(&database, &orca, &clock).sync()?;
        drop(database);
        context.refresh_state()?;
        if parsed.flag("json") {
            let fields = json!({ "at": at, "orca": report.reachable, "runs": report.runs });
            return Reply::new(context, true).success(fields);
        }
        let console = context.console();
        if !report.reachable {
            console.print("Orca를 부르지 못했습니다");
        }
        if report.runs.is_empty() {
            console.print("진행 중인 세션 없음");
        }
        for run in &report.runs {
            console.print(&format!("#{} {}", run.item_id, run.state));
        }
        Ok(0)
    }
}

pub struct Sessions;

impl Command for Sessions {
    fn name(&self) -> &'static str {
        "sessions"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos sessions [on|off] [--json]")
    }

    /// 세션을 여는 잠금. 차단 시험(S3, S4)을 본 뒤에 사용자가 켠다. 세션 안에서는 켜지 못한다.
    /// 쓰기는 사용자가 터미널에서 직접 하는 일이라 패널에 버튼을 두지 않는다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let mode = parsed.first();
        if mode.is_some_and(|mode| mode != "on" && mode != "off") {
            return Ok(reply.usage("사용법: kairos sessions [on|off]"));
        }
        if mode == Some("on") && context.env_set("CLAUDECODE") {
            return Ok(reply.fail("session_caller", "세션 안에서는 켤 수 없습니다", 1));
        }
        if mode == Some("on") && !Os::current().supports_sessions() {
            return Ok(reply.fail("sessions_unsupported", kairos_sessions::SESSIONS_UNSUPPORTED, 1));
        }
        let approval = SessionApproval::new(context.home());
        if let Some(mode) = mode {
            let at = context.now().to_iso();
            let passed_at = (mode == "on").then_some(at.as_str());
            // 지문을 먼저 남긴다. control만 쓰고 죽으면 지문이 없어 잠긴 채로 남는다(안전한 쪽).
            if mode == "on" {
                approval.record()?
            } else {
                approval.clear()?
            }
            context.control_file().write(&context.control().with_sessions(passed_at))?;
            context.refresh_state()?;
        }
        let lock = approval.lock(&context.control());
        let enabled = lock == SessionLock::Open;
        if reply.json() {
            let changed = lock == SessionLock::ConfigChanged;
            return reply.success(json!({ "sessions": { "enabled": enabled, "configChanged": changed } }));
        }
        context.console().print(match lock {
            SessionLock::Open => "세션 열림",
            SessionLock::Locked => "세션 잠김. 차단 시험 뒤 kairos sessions on",
            SessionLock::ConfigChanged => "세션 잠김. 설정이 바뀌었습니다. 차단 시험을 다시 본 뒤 kairos sessions on",
        });
        Ok(0)
    }
}
