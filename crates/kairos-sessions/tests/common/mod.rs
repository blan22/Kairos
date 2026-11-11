//! 고정값은 모두 합성이다
//! 호출 형태는 Orca 1.4.248의 CLI 코드에서 읽은 모양이다.
#![allow(dead_code, clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kairos_agenda::{
    DisplayGate, DisplayMode, DisplayStart, IdSource, Item, ItemEngine, ItemRepo, KairosControl, NextContext,
    RefreshInput,
};
use kairos_core::model::{
    ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult, TrustTier,
};
use kairos_core::ports::FixedClock;
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_core::{Instant, KairosError, KairosHome, KairosResult, OffDates};
use kairos_ledger::{IngestOptions, Ledger};
use kairos_sessions::{
    LiveAuthorCheck, OrcaClient, Run, RunRepo, RunSynchronizer, SESSION_GH_TOKEN_FILE, SessionLauncher, SessionOutcome,
    SessionRefusal, SessionToken, SyncReport,
};
use kairos_source_github::LiveAuthors;
use kairos_store::Database;
use serde_json::{Value, json};
use tempfile::TempDir;

/// 시험 프로필(`Profile::fixture`)의 플레이북 대상.
pub const PLAYBOOK_ORG: &str = "contoso";
pub const PLAYBOOK_REPOS: [&str; 2] = ["atlas", "atlas-specs"];

pub const WORKTREE_ID: &str = "repo-synth::/synthetic/worktrees/t-1";
pub const SELECTOR: &str = "identity:wt-synth-key";
pub const PANE: &str = "tab-synth:leaf-synth";
pub const AT: &str = "2026-03-02T01:00:00.000Z"; // 월요일 10:00 KST
pub const BEFORE: &str = "2026-03-01T01:00:00.000Z";
pub const RUN: &str = "11111111-2222-4333-8444-555555555555";
pub const OTHER_RUN: &str = "99999999-2222-4333-8444-555555555555";
pub const RUN_BINARY: &str = "/synthetic/bin/kairos-run";
/// 합성 fine-grained 토큰. 실제 GitHub 토큰이 아니다.
pub const SYNTHETIC_TOKEN: &str = "github_pat_SYNTHETIC0123456789abcdef";

pub fn session_token() -> SessionToken {
    SessionToken::parse(SYNTHETIC_TOKEN).unwrap()
}

/// 소유자가 두는 것과 같은 0600 토큰 파일을 데이터 폴더에 쓴다. 세션은 이 파일이 없으면 시작하지 않는다.
pub fn write_session_token(home: &KairosHome, content: &str, mode: u32) {
    let path = home.file(SESSION_GH_TOKEN_FILE);
    let _ = fs::remove_file(&path);
    fs::write(&path, content).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
}

pub fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

pub fn later(ms: i64) -> Instant {
    at(AT).plus_ms(ms)
}

pub fn later_iso(ms: i64) -> String {
    later(ms).to_iso()
}

pub fn issue_url(number: i64) -> String {
    issue_url_in(number, PLAYBOOK_REPOS[0])
}

pub fn issue_url_in(number: i64, repo: &str) -> String {
    format!("https://github.com/{PLAYBOOK_ORG}/{repo}/issues/{number}")
}

pub fn worktree() -> Value {
    json!({
        "id": WORKTREE_ID,
        "identity": { "key": "wt-synth-key" },
        "path": "/synthetic/worktrees/t-1",
        "head": "abc123",
        "branch": "synthetic",
    })
}

pub fn layout(title: &str) -> Value {
    json!({
        "worktreeId": WORKTREE_ID,
        "root": {
            "type": "group",
            "tabs": [{
                "tabId": "tab-synth",
                "title": title,
                "panes": { "type": "pane", "handle": "term_found", "tabId": "tab-synth", "leafId": "leaf-synth", "active": true },
            }],
        },
    })
}

pub fn pane_row() -> Value {
    json!({ "handle": "term_now", "tabId": "tab-synth", "leafId": "leaf-synth", "connected": true, "title": "합성 제목" })
}

/// `pane_row`의 값을 덮는다.
pub fn pane_with(key: &str, value: Value) -> Value {
    let mut row = pane_row();
    row[key] = value;
    row
}

/// Orca의 답 하나. 실제 프로세스가 내는 출력으로 바뀌어 `OrcaClient`의 해석을 거친다.
#[derive(Clone, Debug)]
pub enum Canned {
    Ok(Value),
    Refused(&'static str),
    Lost,
    Missing,
}

pub fn ok(result: Value) -> Canned {
    Canned::Ok(result)
}

pub fn ps_with(states: &[&str]) -> Canned {
    let agents: Vec<Value> = states
        .iter()
        .enumerate()
        .map(|(index, state)| {
            let pane_key = if index == 0 { PANE.to_owned() } else { format!("tab-other:leaf-{index}") };
            json!({ "paneKey": pane_key, "state": state })
        })
        .collect();
    ok(json!({ "worktrees": [{ "worktreeId": WORKTREE_ID, "agents": agents }], "truncated": false }))
}

fn output_of(reply: Canned) -> ProcessOutput {
    match reply {
        Canned::Ok(result) => {
            ProcessOutput::exited(0, json!({ "id": "req", "ok": true, "result": result, "_meta": {} }).to_string())
        }
        Canned::Refused(code) => {
            ProcessOutput::exited(1, json!({ "ok": false, "error": { "code": code } }).to_string())
        }
        Canned::Lost => ProcessOutput::ended(ProcessEnd::TimedOut),
        Canned::Missing => ProcessOutput::ended(ProcessEnd::Missing),
    }
}

type Script = Arc<dyn Fn(&[String]) -> Canned + Send + Sync>;

#[derive(Clone)]
enum Reply {
    Fixed(Canned),
    Scripted(Script),
}

/// 받은 요청 한 건.
#[derive(Clone, Debug)]
pub struct Call {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub cwd: Option<PathBuf>,
    pub env: Option<BTreeMap<String, String>>,
}

/// 실제 orca를 부르지 않는다. 모르는 명령은 시험을 깨뜨린다.
pub struct FakeOrca {
    replies: Mutex<HashMap<String, Reply>>,
    calls: Mutex<Vec<Call>>,
}

fn command_of(args: &[String]) -> String {
    format!("{} {}", args[0], args[1])
}

impl FakeOrca {
    pub fn new() -> Arc<Self> {
        let terminal = json!({
            "handle": "term_made", "tabId": "tab-synth", "paneKey": PANE, "worktreeId": WORKTREE_ID, "surface": "background",
        });
        let defaults = [
            ("reference find", json!({ "kind": "reference_matches", "matches": [], "truncated": false })),
            ("worktree create", json!({ "worktree": worktree() })),
            ("worktree show", json!({ "worktree": worktree() })),
            ("terminal create", json!({ "terminal": terminal })),
            ("terminal list", json!({ "terminals": [pane_row()], "totalCount": 1, "truncated": false })),
            ("worktree ps", json!({ "worktrees": [], "truncated": false })),
            ("terminal switch", json!({ "focus": { "handle": "term_now", "tabId": "tab-synth" } })),
        ];
        let replies = defaults.map(|(command, result)| (command.to_owned(), Reply::Fixed(ok(result))));
        Arc::new(Self { replies: Mutex::new(HashMap::from(replies)), calls: Mutex::new(Vec::new()) })
    }

    pub fn with(command: &str, reply: Canned) -> Arc<Self> {
        let orca = Self::new();
        orca.set(command, reply);
        orca
    }

    pub fn set(&self, command: &str, reply: Canned) {
        self.replies.lock().unwrap().insert(command.to_owned(), Reply::Fixed(reply));
    }

    pub fn script(&self, command: &str, reply: impl Fn(&[String]) -> Canned + Send + Sync + 'static) {
        self.replies.lock().unwrap().insert(command.to_owned(), Reply::Scripted(Arc::new(reply)));
    }

    pub fn requests(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.requests().into_iter().map(|call| call.args).collect()
    }

    pub fn clear(&self) {
        self.calls.lock().unwrap().clear();
    }

    pub fn count(&self, command: &str) -> usize {
        self.calls().iter().filter(|call| command_of(call) == command).count()
    }

    pub fn args(&self, command: &str) -> Vec<String> {
        self.calls().into_iter().find(|call| command_of(call) == command).unwrap_or_default()
    }

    /// 인자 배열 전체의 평탄한 목록.
    pub fn flat(&self) -> Vec<String> {
        self.calls().into_iter().flatten().collect()
    }
}

impl ProcessRunner for FakeOrca {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        self.calls.lock().unwrap().push(Call {
            program: request.program.clone(),
            args: request.args.clone(),
            timeout: request.timeout,
            cwd: request.cwd.clone(),
            env: request.env.clone(),
        });
        let command = command_of(&request.args);
        let reply = self.replies.lock().unwrap().get(&command).cloned();
        match reply.unwrap_or_else(|| panic!("시험에 없는 orca 명령: {}", request.args.join(" "))) {
            Reply::Fixed(canned) => output_of(canned),
            Reply::Scripted(script) => output_of(script(&request.args)),
        }
    }
}

/// 띄우기 직전 재확인의 대역. 정한 판정을 주고 받은 URL을 적는다. 실제 gh를 부르지 않는다.
pub struct FakeLive {
    verdict: Result<LiveAuthors, &'static str>,
    pub urls: Mutex<Vec<String>>,
}

impl FakeLive {
    pub fn new(verdict: Result<LiveAuthors, &'static str>) -> Self {
        Self { verdict, urls: Mutex::new(Vec::new()) }
    }

    pub fn insiders() -> Self {
        Self::new(Ok(LiveAuthors::Insiders))
    }
}

impl LiveAuthorCheck for FakeLive {
    fn live_authors(&self, url: &str) -> KairosResult<LiveAuthors> {
        self.urls.lock().unwrap().push(url.to_owned());
        self.verdict.map_err(KairosError::new)
    }
}

pub fn client(orca: &Arc<FakeOrca>) -> OrcaClient {
    OrcaClient::new(orca.clone(), Some("/synthetic/orca".to_owned()), "/synthetic/home").with_env(BTreeMap::new())
}

pub struct FixedId(pub String);

impl IdSource for FixedId {
    fn unique(&self) -> String {
        self.0.clone()
    }
}

pub fn admit_all() -> Option<SessionRefusal> {
    None
}

pub fn accept_as(db: &Database, orca: &Arc<FakeOrca>, id: i64, run_id: &str) -> SessionOutcome {
    let (client, clock, ids) = (client(orca), FixedClock(at(AT)), FixedId(run_id.to_owned()));
    SessionLauncher::new(db, &client, &clock, &admit_all, RUN_BINARY).with_ids(&ids).accept(id).unwrap()
}

pub fn accept(db: &Database, orca: &Arc<FakeOrca>) -> SessionOutcome {
    accept_as(db, orca, 1, RUN)
}

pub fn cancel(db: &Database, id: i64, ms: i64) -> SessionOutcome {
    let (orca, clock) = (client(&FakeOrca::new()), FixedClock(later(ms)));
    SessionLauncher::new(db, &orca, &clock, &admit_all, RUN_BINARY).cancel(id).unwrap()
}

pub fn move_to(db: &Database, orca: &Arc<FakeOrca>, id: i64) -> SessionOutcome {
    let (client, clock) = (client(orca), FixedClock(at(AT)));
    SessionLauncher::new(db, &client, &clock, &admit_all, RUN_BINARY).move_to(id).unwrap()
}

pub fn sync(db: &Database, orca: &Arc<FakeOrca>, ms: i64) -> SyncReport {
    let (client, clock) = (client(orca), FixedClock(later(ms)));
    RunSynchronizer::new(db, &client, &clock).sync().unwrap()
}

pub fn test_db() -> Database {
    Database::in_memory().unwrap()
}

pub fn temp_home() -> (TempDir, KairosHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at(dir.path());
    (dir, home)
}

fn result(source: SourceName, when: &str) -> SourceResult {
    SourceResult {
        source,
        query: "tick".into(),
        outcome: Outcome::Ok,
        started_at: when.into(),
        finished_at: when.into(),
        pages: 1,
        expected_count: None,
        parsed_count: 0,
        identity: None,
        window_from: None,
        window_to: None,
        watermark: Some(when.into()),
        progress_at: None,
        thread_reads: Vec::new(),
        overlap_seconds: 300,
        snapshot: false,
        error_code: None,
        objects: Vec::new(),
        events: Vec::new(),
    }
}

fn ingest(db: &Database, run: SourceResult, when: &str) {
    Ledger::new(db).ingest(run, &IngestOptions::at(at(when))).unwrap();
}

pub fn push_objects(db: &Database, objects: Vec<NormalizedObject>) {
    ingest(db, SourceResult { objects, ..result(SourceName::Github, AT) }, AT);
}

fn push_events(db: &Database, events: Vec<NormalizedEvent>) {
    ingest(db, SourceResult { events, ..result(SourceName::Github, AT) }, AT);
}

pub fn issue(number: i64) -> NormalizedObject {
    NormalizedObject {
        text_head: Some(format!("합성 이슈 {number}")),
        state: Some("open".into()),
        assigned_to_me: Some(true),
        authored_by_me: Some(false),
        ..NormalizedObject::new(
            format!("gh:acme/widget#{number}"),
            SourceName::Github,
            ObjectKind::Issue,
            number.to_string(),
        )
    }
}

pub fn pull_request(number: i64) -> NormalizedObject {
    NormalizedObject {
        kind: ObjectKind::PullRequest,
        assigned_to_me: Some(false),
        authored_by_me: Some(true),
        ..issue(number)
    }
}

fn comment(number: i64, object_key: String) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("synthetic-teammate".into()),
        object_key: Some(object_key),
        trust_tier: Some(TrustTier::T1),
        ..NormalizedEvent::new(
            format!("gh:comment:c{number}"),
            SourceName::Github,
            EventType::GhComment,
            AT,
            ActorKind::Human,
        )
    }
}

pub fn refresh(db: &Database, now: &str) {
    let (none, ok_sources) = (OffDates::new(), BTreeSet::new());
    let input = RefreshInput { now: at(now), off_dates: &none, ok_sources: &ok_sources, cards_held: false };
    ItemEngine::new(db).refresh(&input).unwrap();
}

/// 시작 버튼이 나오는 담당 이슈 하나를 연다.
pub fn seed_issue_with(db: &Database, number: i64, change: impl FnOnce(&mut NormalizedObject)) {
    ingest(db, result(SourceName::Slack, BEFORE), BEFORE);
    ingest(db, result(SourceName::Github, BEFORE), BEFORE);
    let mut object = issue(number);
    object.permalink = Some(issue_url(number));
    object.trust_tier = Some(TrustTier::T1);
    change(&mut object);
    let key = object.object_key.clone();
    push_objects(db, vec![object]);
    push_events(db, vec![comment(number, key)]);
    refresh(db, AT);
}

pub fn pull_url(number: i64) -> String {
    format!("https://github.com/{PLAYBOOK_ORG}/{}/pull/{number}", PLAYBOOK_REPOS[0])
}

/// 그 사건으로 열린 PR 항목 하나를 만든다. 행위자는 조직의 사람이다.
pub fn seed_pull_with(db: &Database, number: i64, event: EventType, change: impl FnOnce(&mut NormalizedObject)) {
    ingest(db, result(SourceName::Slack, BEFORE), BEFORE);
    ingest(db, result(SourceName::Github, BEFORE), BEFORE);
    let mut object = pull_request(number);
    object.permalink = Some(pull_url(number));
    object.trust_tier = Some(TrustTier::T1);
    change(&mut object);
    let key = object.object_key.clone();
    push_objects(db, vec![object]);
    push_events(db, vec![NormalizedEvent { event_type: event, ..comment(number, key) }]);
    refresh(db, AT);
}

pub fn seed_issue(db: &Database) {
    seed_issue_with(db, 1, |_| {});
}

pub fn seeded() -> (Database, Arc<FakeOrca>) {
    let db = test_db();
    seed_issue(&db);
    (db, FakeOrca::new())
}

pub fn count(db: &Database, table: &str) -> i64 {
    db.connection().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

pub fn item(db: &Database, id: i64) -> Item {
    ItemRepo::new(db.connection()).get(id).unwrap().unwrap_or_else(|| panic!("항목 없음: {id}"))
}

/// 가장 먼저 만든 run.
pub fn run(db: &Database) -> Run {
    let id: String = db
        .connection()
        .query_row("SELECT id FROM runs ORDER BY created_at, rowid LIMIT 1", [], |row| row.get(0))
        .unwrap();
    RunRepo::new(db.connection()).get(&id).unwrap().unwrap()
}

pub fn control_on() -> KairosControl {
    KairosControl::default().with_display_mode(DisplayMode::On)
}

pub fn context(now: Instant, control: &KairosControl) -> NextContext {
    let off_dates = OffDates::new();
    let start = DisplayStart::compute(Some(BEFORE), now, &off_dates).unwrap();
    let gate: DisplayGate = DisplayGate::evaluate(&start, control, now);
    NextContext { now, control: control.clone(), off_dates, gate, health: None }
}
