//! 틱의 포트를 대신하는 가짜. 실제 claude, gh, orca를 띄우지 않는다.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use kairos_agenda::{EngineReport, RefreshInput};
use kairos_core::model::{
    CollectContext, DirectSource, McpCall, McpResponse, McpSource, ParserFailed, QuerySpec, Source, SourceMode,
    SourceName, SourceResult,
};
use kairos_core::ports::{Alert, FixedClock, Notifier, PanelOpener};
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_core::{KairosError, KairosHome, KairosResult};
use kairos_store::Database;
use kairos_tick::{
    ClaudeProbe, CollectReport, DROP_RESULT, Doctor, DoctorConfig, DoctorReport, ItemRefresher, SleepProbe, TickConfig,
    TickRunner,
};
use serde_json::{Value, json};

use super::at;

pub type Work = Box<dyn Fn(&CollectContext) -> Vec<SourceResult> + Send + Sync>;

/// 합성 직접 소스.
pub struct DirectFake {
    pub name: SourceName,
    pub queries: Vec<QuerySpec>,
    pub work: Work,
}

impl DirectSource for DirectFake {
    fn collect(&self, context: &CollectContext) -> Vec<SourceResult> {
        (self.work)(context)
    }
}

impl Source for DirectFake {
    fn name(&self) -> SourceName {
        self.name
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::Direct(self)
    }
}

pub fn direct_with(
    name: SourceName,
    queries: Vec<QuerySpec>,
    work: impl Fn(&CollectContext) -> Vec<SourceResult> + Send + Sync + 'static,
) -> Arc<dyn Source> {
    Arc::new(DirectFake { name, queries, work: Box::new(work) })
}

pub fn tick_query() -> Vec<QuerySpec> {
    vec![QuerySpec::new("tick", 180, 300)]
}

pub fn direct(
    name: SourceName,
    work: impl Fn(&CollectContext) -> Vec<SourceResult> + Send + Sync + 'static,
) -> Arc<dyn Source> {
    direct_with(name, tick_query(), work)
}

/// `new Promise(() => {})`. 취소 신호도 무시한다.
pub fn hung(name: SourceName) -> Arc<dyn Source> {
    direct(name, |_| {
        loop {
            std::thread::park();
        }
    })
}

/// 합성 수집 모드 소스. plan과 parse가 빈 값이다.
pub struct McpFake {
    pub name: SourceName,
    pub queries: Vec<QuerySpec>,
}

impl McpSource for McpFake {
    fn plan(&self, _context: &CollectContext) -> Vec<McpCall> {
        Vec::new()
    }

    fn parse(&self, _responses: &[McpResponse], _context: &CollectContext) -> Result<Vec<SourceResult>, ParserFailed> {
        Ok(Vec::new())
    }
}

impl Source for McpFake {
    fn name(&self) -> SourceName {
        self.name
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::Mcp(self)
    }
}

pub fn mcp_fake(name: SourceName, queries: Vec<QuerySpec>) -> Arc<dyn Source> {
    Arc::new(McpFake { name, queries })
}

/// 연결 없는 합성 소스.
pub struct Unconnected {
    pub name: SourceName,
    pub queries: Vec<QuerySpec>,
}

impl Source for Unconnected {
    fn name(&self) -> SourceName {
        self.name
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::NotConnected { reason: "연결 없음" }
    }
}

pub fn unconnected(name: SourceName, queries: Vec<QuerySpec>) -> Arc<dyn Source> {
    Arc::new(Unconnected { name, queries })
}

/// claude와 pmset을 대신하는 러너. 그 밖의 호출은 없는 실행 파일이다.
pub struct FnRunner(pub Box<dyn Fn(&ProcessRequest) -> ProcessOutput + Send + Sync>);

impl ProcessRunner for FnRunner {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        (self.0)(request)
    }
}

pub fn runner(work: impl Fn(&ProcessRequest) -> ProcessOutput + Send + Sync + 'static) -> Arc<dyn ProcessRunner> {
    Arc::new(FnRunner(Box::new(work)))
}

pub fn missing_runner() -> Arc<dyn ProcessRunner> {
    runner(|_| ProcessOutput::ended(ProcessEnd::Missing))
}

/// claude의 결과 JSON. `over`의 키를 덮어쓴다.
pub fn claude_stdout(over: Value) -> String {
    let mut body = json!({ "is_error": false, "num_turns": 0, "total_cost_usd": 0, "result": DROP_RESULT });
    for (key, value) in over.as_object().unwrap() {
        body[key] = value.clone();
    }
    body.to_string()
}

pub fn dropped(over: Value) -> ProcessOutput {
    ProcessOutput::exited(0, claude_stdout(over))
}

pub fn drop_runner(over: Value) -> Arc<dyn ProcessRunner> {
    let output = dropped(over);
    runner(move |_| output.clone())
}

/// 고정 답을 주는 탐침.
#[derive(Clone, Default)]
pub struct Quiet {
    pub fingerprint: Option<String>,
    pub login: Option<String>,
    pub fail_fingerprint: bool,
}

impl ClaudeProbe for Quiet {
    fn fingerprint(&self) -> KairosResult<Option<String>> {
        if self.fail_fingerprint {
            return Err(KairosError::new("synthetic"));
        }
        Ok(self.fingerprint.clone())
    }

    fn login_expiry(&self) -> KairosResult<Option<String>> {
        Ok(self.login.clone())
    }
}

/// 잠자기 측정을 고정한다.
pub struct SleepFake {
    pub gap: i64,
    pub counts: Mutex<Vec<i64>>,
}

impl SleepProbe for SleepFake {
    fn gap_ms(&self) -> i64 {
        self.gap
    }

    fn count(&self) -> Option<i64> {
        let mut counts = self.counts.lock().unwrap();
        if counts.is_empty() { None } else { Some(counts.remove(0)) }
    }
}

pub fn frozen(gap: i64) -> Arc<dyn SleepProbe> {
    Arc::new(SleepFake { gap, counts: Mutex::new(Vec::new()) })
}

pub fn counted(gap: i64, counts: Vec<i64>) -> Arc<dyn SleepProbe> {
    Arc::new(SleepFake { gap, counts: Mutex::new(counts) })
}

pub struct FnNotifier(pub Box<dyn Fn(&str) -> bool + Send + Sync>);

/// 본문만 넘긴다.
impl Notifier for FnNotifier {
    fn notify(&self, alert: &Alert) -> bool {
        (self.0)(&alert.body)
    }
}

pub fn notifier(work: impl Fn(&str) -> bool + Send + Sync + 'static) -> Arc<dyn Notifier> {
    Arc::new(FnNotifier(Box::new(work)))
}

pub struct FnPanel(pub Box<dyn Fn() -> bool + Send + Sync>);

impl PanelOpener for FnPanel {
    fn open(&self) -> bool {
        (self.0)()
    }
}

pub fn panel(work: impl Fn() -> bool + Send + Sync + 'static) -> Arc<dyn PanelOpener> {
    Arc::new(FnPanel(Box::new(work)))
}

/// 항목 엔진을 대신한다.
pub struct RefreshFake(pub Box<dyn Fn() -> KairosResult<EngineReport> + Send + Sync>);

impl ItemRefresher for RefreshFake {
    fn refresh(&self, _database: &Database, _input: &RefreshInput<'_>) -> KairosResult<EngineReport> {
        (self.0)()
    }
}

/// 틱 하나의 조립.
pub struct Tick {
    home: KairosHome,
    now: String,
    sources: Vec<Arc<dyn Source>>,
    notifier: Option<Arc<dyn Notifier>>,
    panel: Option<Arc<dyn PanelOpener>>,
    runner: Arc<dyn ProcessRunner>,
    probe: Quiet,
    sleep: Option<Arc<dyn SleepProbe>>,
    items: Option<Arc<dyn ItemRefresher>>,
    claude_bin: Option<String>,
    plugin_dir: Option<String>,
    user_home: PathBuf,
    limit_ms: Option<i64>,
    env: BTreeMap<String, String>,
}

/// `collect({ home, now, sources, fingerprint: null, loginExpiry: null })`.
pub fn tick(home: &KairosHome, now: &str, sources: Vec<Arc<dyn Source>>) -> Tick {
    Tick {
        home: home.clone(),
        now: now.to_owned(),
        sources,
        notifier: None,
        panel: None,
        runner: missing_runner(),
        probe: Quiet::default(),
        sleep: None,
        items: None,
        claude_bin: None,
        plugin_dir: None,
        user_home: home.file("user"),
        limit_ms: None,
        env: BTreeMap::new(),
    }
}

impl Tick {
    pub fn notifier(mut self, notifier: Arc<dyn Notifier>) -> Self {
        self.notifier = Some(notifier);
        self
    }

    pub fn panel(mut self, panel: Arc<dyn PanelOpener>) -> Self {
        self.panel = Some(panel);
        self
    }

    /// 수집 모드를 띄울 조건을 채운다. `mode`는 claude 호출만 받는다. pmset은 없는 실행 파일이다.
    pub fn claude(mut self, plugin: &str, mode: Arc<dyn ProcessRunner>) -> Self {
        self.claude_bin = Some("/x/claude".to_owned());
        self.plugin_dir = Some(plugin.to_owned());
        self.runner = runner(move |request| {
            if request.program == "/x/claude" { mode.run(request) } else { ProcessOutput::ended(ProcessEnd::Missing) }
        });
        self
    }

    /// 프로세스 호출 전부를 받는다.
    pub fn process_runner(mut self, runner: Arc<dyn ProcessRunner>) -> Self {
        self.runner = runner;
        self
    }

    pub fn claude_bin(mut self, bin: Option<&str>, plugin: Option<&str>) -> Self {
        self.claude_bin = bin.map(str::to_owned);
        self.plugin_dir = plugin.map(str::to_owned);
        self
    }

    pub fn fingerprint(mut self, value: &str) -> Self {
        self.probe.fingerprint = Some(value.to_owned());
        self
    }

    pub fn probe(mut self, probe: Quiet) -> Self {
        self.probe = probe;
        self
    }

    pub fn sleep(mut self, sleep: Arc<dyn SleepProbe>) -> Self {
        self.sleep = Some(sleep);
        self
    }

    pub fn items(mut self, work: impl Fn() -> KairosResult<EngineReport> + Send + Sync + 'static) -> Self {
        self.items = Some(Arc::new(RefreshFake(Box::new(work))));
        self
    }

    pub fn limit(mut self, ms: i64) -> Self {
        self.limit_ms = Some(ms);
        self
    }

    pub fn user_home(mut self, path: &std::path::Path) -> Self {
        self.user_home = path.to_owned();
        self
    }

    pub fn env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = env;
        self
    }

    pub fn runner(self) -> TickRunner {
        let mut config = TickConfig::new(self.home.clone(), self.user_home);
        config.claude_bin = self.claude_bin;
        config.plugin_dir = self.plugin_dir;
        config.env = self.env;
        if let Some(limit) = self.limit_ms {
            config.source_limit_ms = limit;
        }
        let clock = Arc::new(FixedClock(at(&self.now)));
        let mut runner = TickRunner::new(config, clock, self.runner, self.notifier, self.panel, self.sources)
            .with_claude_probe(Arc::new(self.probe));
        if let Some(sleep) = self.sleep {
            runner = runner.with_sleep_probe(sleep);
        }
        if let Some(items) = self.items {
            runner = runner.with_item_refresher(items);
        }
        runner
    }

    pub fn try_run(self, force: bool) -> KairosResult<CollectReport> {
        self.runner().run(force)
    }

    pub fn run(self) -> CollectReport {
        self.try_run(false).unwrap()
    }

    pub fn forced(self) -> CollectReport {
        self.try_run(true).unwrap()
    }
}

pub fn doctor_at(home: &KairosHome, now: &str, sources: Vec<Arc<dyn Source>>) -> Doctor {
    let config = DoctorConfig { home: home.clone(), user_home: home.file("user"), env: BTreeMap::new() };
    Doctor::new(config, Arc::new(FixedClock(at(now))), missing_runner(), sources)
}

/// `diagnose({ home, now, sources, loginExpiry: async () => null })`.
pub fn diagnose(home: &KairosHome, now: &str, sources: Vec<Arc<dyn Source>>) -> DoctorReport {
    doctor_at(home, now, sources).with_login_probe(Arc::new(Quiet::default())).diagnose().unwrap()
}

/// `diagnose`에 로그인 만료 시각을 준다.
pub fn diagnose_login(home: &KairosHome, now: &str, sources: Vec<Arc<dyn Source>>, login: &str) -> DoctorReport {
    let probe = Quiet { login: Some(login.to_owned()), ..Quiet::default() };
    doctor_at(home, now, sources).with_login_probe(Arc::new(probe)).diagnose().unwrap()
}

/// source_run의 (소스, 결과, 코드).
pub fn run_rows(home: &KairosHome) -> Vec<(String, String, Option<String>)> {
    let db = Database::open(home).unwrap();
    let mut statement =
        db.connection().prepare("SELECT source, outcome, error_code FROM source_run ORDER BY id").unwrap();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

/// 플러그인 폴더가 있는 홈.
pub fn plugin_dir(home: &KairosHome) -> String {
    let plugin = home.file("plugin");
    std::fs::create_dir_all(&plugin).unwrap();
    plugin.to_string_lossy().into_owned()
}
