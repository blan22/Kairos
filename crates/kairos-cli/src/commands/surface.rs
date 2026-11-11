//! 표시면 계약. 호스트(Orca, Claude Code, VS Code)는 이 명령들과 `next`, `list`, 항목 조작만 부른다.
//! 계약은 `docs/surface-protocol.md`에 있다. 내용은 담지 않고, 호스트는 신호를 받으면 `next --json`을 다시 부른다.

use std::fs;
use std::io::{self, Write};
use std::thread;
use std::time::{Duration, SystemTime};

use kairos_agenda::{CONTRACT_VERSION, REVEAL_FILE, SESSION_APPROVAL_FILE, SessionApproval, SessionLock};
use kairos_core::KairosResult;
use kairos_core::model::SourceMode;
use kairos_ops::VERSION;
use serde_json::json;

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::reply::Reply;

/// 호스트가 `next --surface`로 넘기는 이름. `panel`은 Orca 패널의 옛 이름이다.
pub const SURFACES: [&str; 4] = ["panel", "orca", "claude-code", "vscode"];
/// 항목에 붙는 조작. 호스트는 이 이름과 항목 ID만 넘긴다.
const ITEM_ACTIONS: [&str; 9] = ["start", "later", "done", "keep", "keep_deploy", "ack", "move", "cancel", "pr"];
/// 바뀌면 호스트가 다시 읽어야 하는 파일.
const WATCHED: [&str; 5] = ["state.json", "control.json", REVEAL_FILE, SESSION_APPROVAL_FILE, "engine.json"];
const DEFAULT_INTERVAL_MS: u64 = 2000;
const MIN_INTERVAL_MS: u64 = 250;

pub struct Hello;

impl Command for Hello {
    fn name(&self) -> &'static str {
        "hello"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos hello [--json]")
    }

    /// 호스트가 시작할 때 부른다. 계약 판과 이 컴퓨터의 구성을 준다. 상태를 바꾸지 않는다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&[flag("json")]).parse(args)?;
        let profile = kairos_core::profile::current();
        let lock = SessionApproval::new(context.home()).lock(&context.control());
        let sources: Vec<_> = context
            .sources()
            .iter()
            .map(|source| {
                let connected = !matches!(source.mode(), SourceMode::NotConnected { .. });
                json!({ "name": source.name().as_str(), "connected": connected })
            })
            .collect();
        let body = json!({
            "contract": CONTRACT_VERSION,
            "engine": VERSION,
            "surfaces": SURFACES,
            "actions": ITEM_ACTIONS,
            "hosts": profile.hosts.enabled,
            "sessionHost": profile.session_host.as_str(),
            "sessions": { "enabled": lock == SessionLock::Open, "configChanged": lock == SessionLock::ConfigChanged },
            "sources": sources,
            "home": context.home().root().to_string_lossy(),
        });
        let reply = Reply::new(context, parsed.flag("json"));
        if reply.json() {
            return reply.success(body);
        }
        context
            .console()
            .print(&format!("kairos {VERSION}, 계약 {CONTRACT_VERSION}, 호스트 {}", profile.hosts.enabled.join(", ")));
        Ok(0)
    }
}

fn signature(context: &AppContext) -> Vec<Option<SystemTime>> {
    WATCHED.iter().map(|name| fs::metadata(context.home().file(name)).and_then(|meta| meta.modified()).ok()).collect()
}

pub struct Watch;

impl Command for Watch {
    fn name(&self) -> &'static str {
        "watch"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos watch --json [--interval <ms>] [--count <n>]")
    }

    /// 상태 파일이 바뀔 때마다 `{"kind":"changed","at":..}` 한 줄을 낸다. 내용은 담지 않는다.
    /// 첫 줄은 `{"kind":"ready"}`다. 받는 쪽이 닫으면 끝난다. `--count`는 시험이 쓴다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        const OPTIONS: [OptionSpec; 3] = [flag("json"), text("interval"), text("count")];
        let parsed = ArgParser::new(&OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        if !reply.json() {
            return Ok(reply.usage("사용법: kairos watch --json [--interval <ms>]"));
        }
        let interval =
            parsed.text("interval").and_then(|value| value.parse::<u64>().ok()).unwrap_or(DEFAULT_INTERVAL_MS);
        let interval = Duration::from_millis(interval.max(MIN_INTERVAL_MS));
        let limit = parsed.text("count").and_then(|value| value.parse::<usize>().ok());
        let emit = |line: serde_json::Value| -> bool {
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{line}").and_then(|()| stdout.flush()).is_ok()
        };
        if !emit(json!({ "schema": CONTRACT_VERSION, "kind": "ready", "at": context.now() })) {
            return Ok(0);
        }
        let mut last = signature(context);
        let mut sent = 0usize;
        while limit.is_none_or(|limit| sent < limit) {
            thread::sleep(interval);
            let now = signature(context);
            if now == last {
                continue;
            }
            last = now;
            sent += 1;
            if !emit(json!({ "schema": CONTRACT_VERSION, "kind": "changed", "at": context.now() })) {
                return Ok(0);
            }
        }
        Ok(0)
    }
}
