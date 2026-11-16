//! 지운 TS 원본이 남긴 출력과의 바이트 비교. tests/fixtures/golden은 원본을 지우기 전에 마지막으로 돌려 얼린 값이다.
//! 다시 만들 수 없으니 이 시험이 깨지면 고정값을 고치기 전에 출력이 바뀌는 것이 의도인지부터 본다.
//! 시나리오는 golden_scenarios.json이다. orca argv와 시간 제한, 결과 JSON, 표 덤프를 단계마다 맞춘다.
//! `launchCommand`만 지시받은 대로 다르다. Node 쪽도 node 인자를 뺀 문자열을 넘긴다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use common::{RUN_BINARY, SYNTHETIC_TOKEN, later, session_token, write_session_token};
use kairos_agenda::{IdSource, ItemActions, Playbook};
use kairos_core::KairosHome;
use kairos_core::ports::FixedClock;
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_sessions::{
    OrcaClient, OrcaLiveAgent, OrcaPane, OrcaReply, OrcaResult, OrcaTerminal, OrcaWorktree, PsSnapshot, RunRepo,
    RunSynchronizer, SessionClaim, SessionCommand, SessionLauncher, SessionProfile, SessionRefusal, SessionRunner,
    SessionSpawner, SessionWorkspace, find_workspace_named, focus_navigated,
};
use kairos_store::Database;
use kairos_store::rusqlite::types::ValueRef;
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};

const FRESH_ID: &str = "[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}";

struct Script {
    expected: Vec<(String, String)>,
    cursor: usize,
}

impl Script {
    fn load() -> Self {
        let root = repo_root();
        let text = include_str!("fixtures/golden/expected.json").replace("__ROOT__", &root.to_string_lossy());
        let rows: Vec<Value> = serde_json::from_str(&text).unwrap();
        let expected = rows
            .iter()
            .map(|row| (row["name"].as_str().unwrap().to_owned(), row["text"].as_str().unwrap().to_owned()))
            .collect();
        Self { expected, cursor: 0 }
    }

    fn check(&mut self, name: &str, actual: &str) {
        let (expected_name, expected) = &self.expected[self.cursor];
        self.cursor += 1;
        assert_eq!(expected_name, name, "단계 순서가 다르다");
        assert_eq!(actual, expected, "{name}");
    }

    fn put<T: Serialize>(&mut self, name: &str, value: &T) {
        self.check(name, &serde_json::to_string(value).unwrap());
    }

    fn finish(&self) {
        assert_eq!(self.cursor, self.expected.len(), "남은 단계가 있다");
    }
}

fn dump(script: &mut Script, db: &Database, name: &str) {
    let fresh = Regex::new(FRESH_ID).unwrap();
    for table in ["items", "suggestions", "runs", "events"] {
        let mut statement = db.connection().prepare(&format!("SELECT * FROM {table} ORDER BY rowid")).unwrap();
        let columns: Vec<String> = statement.column_names().map(str::to_owned).collect();
        let mut rows = statement.query([]).unwrap();
        let mut lines = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            let mut object = Map::new();
            for (index, column) in columns.iter().enumerate() {
                let value = match row.get_ref(index).unwrap() {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(number) => json!(number),
                    ValueRef::Text(text) => json!(String::from_utf8_lossy(text)),
                    other => panic!("{other:?}"),
                };
                object.insert(column.clone(), value);
            }
            let text = serde_json::to_string(&object).unwrap();
            let replaced = fresh.replace_all(&text, |found: &regex::Captures<'_>| {
                if found[0].starts_with("00000000") { found[0].to_owned() } else { "UUID".to_owned() }
            });
            lines.push(replaced.into_owned());
        }
        script.check(&format!("{name}:{table}"), &lines.join("\n"));
    }
}

fn output_of(spec: &Value) -> ProcessOutput {
    if spec == "lost" {
        return ProcessOutput::ended(ProcessEnd::TimedOut);
    }
    if spec == "missing" {
        return ProcessOutput::ended(ProcessEnd::Missing);
    }
    if let Some(code) = spec.get("refused") {
        return ProcessOutput::exited(1, json!({ "ok": false, "error": { "code": code } }).to_string());
    }
    ProcessOutput::exited(0, json!({ "ok": true, "result": spec["ok"] }).to_string())
}

#[derive(Default)]
struct Scenario {
    replies: HashMap<String, Value>,
    calls: Vec<Value>,
    during: Option<(String, String)>,
}

/// 원본 시험의 runner와 같다.
struct ScenarioOrca {
    home: KairosHome,
    state: Mutex<Scenario>,
}

impl ProcessRunner for ScenarioOrca {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        let mut state = self.state.lock().unwrap();
        let mut call = vec![json!(request.timeout.as_millis() as u64)];
        call.extend(request.args.iter().map(|arg| json!(arg)));
        state.calls.push(Value::Array(call));
        let command = format!("{} {}", request.args[0], request.args[1]);
        if state.during.as_ref().is_some_and(|(during, _)| *during == command) {
            let (_, op) = state.during.take().unwrap();
            let other = Database::open(&self.home).unwrap();
            if op == "cancel" {
                let (client, clock) = (OrcaClient::new(Arc::new(Silent), None, "/"), FixedClock(later(1)));
                SessionLauncher::new(&other, &client, &clock, &no_refusal, RUN_BINARY).cancel(1).unwrap();
            } else {
                ItemActions::new(&other).done(&[1], later(1), false).unwrap();
            }
        }
        let spec = state.replies.get_mut(&command).unwrap();
        if let Value::Array(sequence) = spec {
            let next = if sequence.len() > 1 { sequence.remove(0) } else { sequence[0].clone() };
            return output_of(&next);
        }
        output_of(spec)
    }
}

/// 취소는 Orca를 부르지 않는다.
struct Silent;

impl ProcessRunner for Silent {
    fn run(&self, _request: &ProcessRequest) -> ProcessOutput {
        panic!("부르면 안 된다")
    }
}

fn no_refusal() -> Option<SessionRefusal> {
    None
}

struct NewId(String);

impl IdSource for NewId {
    fn unique(&self) -> String {
        self.0.clone()
    }
}

struct Spawned {
    seen: Mutex<Vec<Value>>,
    exit: i32,
}

impl SessionSpawner for Spawned {
    fn spawn(&self, command: &SessionCommand) -> i32 {
        let mut row = vec![json!(command.file)];
        row.extend(command.args.iter().map(|arg| json!(arg)));
        self.seen.lock().unwrap().push(Value::Array(row));
        self.exit
    }
}

fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn refusal_of(spec: &Value) -> Option<SessionRefusal> {
    let message = spec["message"].as_str()?;
    Some(if spec["status"] == "gate_closed" {
        SessionRefusal::gate_closed(message)
    } else {
        SessionRefusal::not_eligible(message)
    })
}

fn play(script: &mut Script, base: &Path, scenario: &Value, defaults: &Value) {
    let home = KairosHome::at(base.join("rust"));
    let seed = Database::open(&home).unwrap();
    seed.connection().execute_batch(include_str!("fixtures/golden/seed.sql")).unwrap();
    drop(seed);
    // 세션은 읽기 전용 토큰 없이 시작하지 않는다. 원본에 없던 전제라 합성 토큰을 둔다.
    write_session_token(&home, SYNTHETIC_TOKEN, 0o600);
    let replies = defaults.as_object().unwrap().iter().map(|(key, value)| (key.clone(), value.clone())).collect();
    let orca =
        Arc::new(ScenarioOrca { home: home.clone(), state: Mutex::new(Scenario { replies, ..Scenario::default() }) });
    let client = OrcaClient::new(orca.clone(), Some("/synthetic/orca".to_owned()), "/synthetic");
    let profile = SessionProfile::new(&repo_root(), Path::new("/synthetic/kairos-home"), Path::new("/synthetic/home"))
        .with_workspace(
            SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap(),
        );
    let db = Database::open(&home).unwrap();
    for (index, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
        let op = step["op"].as_str().unwrap();
        let name = format!("{}/{index}:{op}", scenario["name"].as_str().unwrap());
        {
            let mut state = orca.state.lock().unwrap();
            if let Some(changes) = step["orca"].as_object() {
                state.replies.extend(changes.iter().map(|(key, value)| (key.clone(), value.clone())));
            }
            state.during = step["during"]["command"]
                .as_str()
                .map(|command| (command.to_owned(), step["during"]["op"].as_str().unwrap().to_owned()));
            state.calls.clear();
        }
        let now = later(step["at"].as_i64().unwrap_or(0));
        let clock = FixedClock(now);
        let refusal = refusal_of(&step["refusal"]);
        let admission = || refusal.clone();
        let ids = NewId(step["run"].as_str().unwrap_or_default().to_owned());
        let launcher = SessionLauncher::new(&db, &client, &clock, &admission, RUN_BINARY).with_ids(&ids);
        let id = step["id"].as_i64().unwrap_or(0);
        let run_id = step["run"].as_str().unwrap_or_default();
        let value = match op {
            "accept" => serde_json::to_value(launcher.accept(id).unwrap()).unwrap(),
            "cancel" => serde_json::to_value(launcher.cancel(id).unwrap()).unwrap(),
            "move" => serde_json::to_value(launcher.move_to(id).unwrap()).unwrap(),
            "sync" => serde_json::to_value(RunSynchronizer::new(&db, &client, &clock).sync().unwrap()).unwrap(),
            "sql" => {
                db.connection().execute_batch(step["text"].as_str().unwrap()).unwrap();
                Value::Null
            }
            "claim" => match SessionRunner::claim(&db, run_id, now).unwrap() {
                SessionClaim::Won { url, .. } => json!({ "ok": true, "url": url }),
                SessionClaim::Refused(code) => json!({ "ok": false, "code": code }),
            },
            "finish" => {
                let run = RunRepo::new(db.connection()).get(run_id).unwrap().unwrap();
                SessionRunner::finish(&db, &run, step["exit"].as_i64().unwrap() as i32, now).unwrap();
                Value::Null
            }
            "run" => {
                let spawner = Spawned { seen: Mutex::new(Vec::new()), exit: step["exit"].as_i64().unwrap() as i32 };
                let claude = step["claude"].as_str().map(str::to_owned);
                let argv: Vec<String> =
                    step["argv"].as_array().unwrap().iter().map(|arg| arg.as_str().unwrap().to_owned()).collect();
                let code =
                    SessionRunner::new(home.clone(), claude, &profile, &spawner, &common::FakeLive::insiders(), &clock)
                        .run(&argv)
                        .unwrap();
                json!({ "code": code, "spawned": spawner.seen.into_inner().unwrap() })
            }
            "command" => {
                let (claude, url) = (step["claude"].as_str().unwrap(), step["url"].as_str().unwrap());
                match profile.command(claude, run_id, Playbook::QaIssue, url, session_token()) {
                    Ok(command) => json!({ "file": command.file, "args": command.args }),
                    Err(_) => json!({ "refused": true }),
                }
            }
            other => panic!("{other}"),
        };
        let calls = orca.state.lock().unwrap().calls.clone();
        script.put(&name, &json!({ "value": value, "calls": calls }));
        dump(script, &db, &name);
    }
}

fn object(value: &Value) -> OrcaResult {
    value.as_object().unwrap().clone()
}

fn worktree_view(found: Option<OrcaWorktree>) -> Value {
    found.map_or(
        Value::Null,
        |row| json!({ "id": row.id, "selector": row.selector, "path": row.path, "head": row.head }),
    )
}

fn terminal_view(found: Option<OrcaTerminal>) -> Value {
    found.map_or(Value::Null, |row| json!({ "handle": row.handle, "paneKey": row.pane_key }))
}

fn agent_view(found: Option<OrcaLiveAgent>) -> Value {
    found.map_or(
        Value::Null,
        |row| json!({ "worktreeId": row.worktree_id, "paneKey": row.pane_key, "handle": row.handle }),
    )
}

fn pane_view(found: Option<OrcaPane>) -> Value {
    found.map_or(Value::Null, |row| json!({ "handle": row.handle, "connected": row.connected }))
}

fn reply_view(reply: OrcaReply) -> Value {
    match reply {
        OrcaReply::Ok(result) => json!({ "kind": "ok", "result": result }),
        OrcaReply::Refused { code } => json!({ "kind": "refused", "code": code }),
        OrcaReply::Lost => json!({ "kind": "lost" }),
        OrcaReply::Missing => json!({ "kind": "missing" }),
    }
}

fn ps_view(ps: PsSnapshot) -> Value {
    let mut agents: Vec<_> = ps.agents.into_iter().collect();
    agents.sort();
    let mut panes: Vec<_> = ps.panes_of.into_iter().collect();
    panes.sort();
    json!({ "agents": agents, "panesOf": panes, "working": ps.working, "truncated": ps.truncated })
}

fn replies(script: &mut Script) {
    let corpus: Value = serde_json::from_str(include_str!("fixtures/golden_replies.json")).unwrap();
    let rows = |name: &str| corpus[name].as_array().unwrap().clone();
    let each = |name: &str, read: &dyn Fn(&OrcaResult) -> Value| -> Vec<Value> {
        rows(name).iter().map(|row| read(&object(row))).collect()
    };
    let keyed = |name: &str, read: &dyn Fn(&OrcaResult, &str) -> Value| -> Vec<Value> {
        rows(name).iter().map(|row| read(&object(&row["result"]), row["key"].as_str().unwrap())).collect()
    };
    let parsed: Vec<Value> =
        rows("parseReply").iter().map(|row| reply_view(OrcaReply::parse(row.as_str().unwrap()))).collect();
    script.put("parseReply", &parsed);
    script.put("readWorktree", &each("readWorktree", &|row| worktree_view(OrcaWorktree::read(row))));
    script.put(
        "readCreatedTerminal",
        &each("readCreatedTerminal", &|row| terminal_view(OrcaTerminal::read_created(row))),
    );
    script.put("findLiveAgent", &each("findLiveAgent", &|row| agent_view(OrcaLiveAgent::find(row))));
    script.put("findWorkspaceNamed", &keyed("findWorkspaceNamed", &|row, key| json!(find_workspace_named(row, key))));
    script.put("readPs", &each("readPs", &|row| ps_view(PsSnapshot::read(row))));
    script.put(
        "findTerminalByTitle",
        &keyed("findTerminalByTitle", &|row, key| terminal_view(OrcaTerminal::find_by_title(row, key))),
    );
    script.put("findPane", &keyed("findPane", &|row, key| pane_view(OrcaPane::find(row, key))));
    script.put("focusNavigated", &each("focusNavigated", &|row| json!(focus_navigated(row))));
}

#[test]
fn 원본과_같은_시나리오의_orca_호출과_표가_바이트까지_같다() {
    let dir = tempfile::tempdir().unwrap();
    let mut script = Script::load();
    let fixture: Value = serde_json::from_str(include_str!("fixtures/golden_scenarios.json")).unwrap();
    for (index, scenario) in fixture["scenarios"].as_array().unwrap().iter().enumerate() {
        play(&mut script, &dir.path().join(index.to_string()), scenario, &fixture["defaults"]);
    }
    replies(&mut script);
    script.finish();
}
