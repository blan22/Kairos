use std::collections::BTreeSet;
use std::panic::{self, AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use kairos_agenda::{ControlFile, KairosControl, StateBuilder};
use kairos_core::model::{Outcome, Source, SourceMode, SourceResult};
use kairos_core::ports::{Clock, Notifier, PanelOpener};
use kairos_core::process::ProcessRunner;
use kairos_core::{Instant, KairosResult, OffDates};
use kairos_ledger::{IngestOptions, Ledger};
use kairos_ops::{
    Maintenance, ResumeFile, SLEEP_INTERRUPTED, SleepDetector, StallWatch, ToolResults, ToolResultsSweep, slept_during,
};
use kairos_policy::{tick_end_minutes, tick_start_minutes};
use kairos_store::Database;

use super::auth::AuthFile;
use super::direct::DirectWork;
use super::fingerprint::{read_fingerprint, write_fingerprint};
use super::guard::{GuardedNotifier, caught};
use super::mode_run::ModeRun;
use super::report::{CollectReport, CollectStatus, ModeReport, OutcomeRow};
use super::settle::Settler;
use super::term::{Heartbeat, HeldGate, TermGate, TermWatch};
use super::{EngineItems, ItemRefresher, LiveSleepProbe, SleepProbe, TOTAL_LIMIT_MS, TickConfig};
use crate::halt::{Halt, HaltFlag};
use crate::lock::CollectLock;
use crate::mode::{ClaudeProbe, SystemClaudeProbe};
use crate::registry::not_connected_results;
use crate::repo::TickRunRepo;
use crate::watch::WatchFile;

fn in_tick_window(now: Instant) -> bool {
    OffDates::new().is_workday(now.local_date())
        && (tick_start_minutes()..tick_end_minutes()).contains(&now.local_minutes())
}

/// 틱 하나의 지휘자.
pub struct TickRunner {
    config: TickConfig,
    clock: Arc<dyn Clock>,
    runner: Arc<dyn ProcessRunner>,
    notifier: Option<Arc<dyn Notifier>>,
    panel: Option<Arc<dyn PanelOpener>>,
    sources: Vec<Arc<dyn Source>>,
    claude: Arc<dyn ClaudeProbe>,
    sleep: Option<Arc<dyn SleepProbe>>,
    items: Arc<dyn ItemRefresher>,
}

/// 잠금 안에서만 쓰는 값.
struct Locked<'a> {
    database: &'a Database,
    now: Instant,
    started: Instant,
    sleep: &'a dyn SleepProbe,
    sleeps_before: Option<i64>,
}

impl TickRunner {
    /// `notifier`와 `panel`이 없으면 알리지 않고 표시도 남기지 않는다. 실제 osascript와 Orca는 조립 지점만 넘긴다.
    pub fn new(
        config: TickConfig,
        clock: Arc<dyn Clock>,
        runner: Arc<dyn ProcessRunner>,
        notifier: Option<Arc<dyn Notifier>>,
        panel: Option<Arc<dyn PanelOpener>>,
        sources: Vec<Arc<dyn Source>>,
    ) -> Self {
        let claude = SystemClaudeProbe::new(runner.clone(), config.claude_bin.clone(), config.env.clone());
        Self {
            config,
            clock,
            runner,
            notifier,
            panel,
            sources,
            claude: Arc::new(claude),
            sleep: None,
            items: Arc::new(EngineItems),
        }
    }

    #[must_use]
    pub fn with_claude_probe(mut self, claude: Arc<dyn ClaudeProbe>) -> Self {
        self.claude = claude;
        self
    }

    #[must_use]
    pub fn with_sleep_probe(mut self, sleep: Arc<dyn SleepProbe>) -> Self {
        self.sleep = Some(sleep);
        self
    }

    #[must_use]
    pub fn with_item_refresher(mut self, items: Arc<dyn ItemRefresher>) -> Self {
        self.items = items;
        self
    }

    fn lock(&self) -> CollectLock {
        CollectLock::new(&self.config.home, self.clock.clone())
    }

    fn sweep(&self) -> KairosResult<ToolResultsSweep> {
        ToolResults::for_collector(&self.config.home, &self.config.user_home).sweep()
    }

    /// 모드를 띄우지 않는 틱의 정리. 앞 틱이 죽어 남긴 파일이 다음 근무일까지 남지 않게 한다.
    /// 잠금을 못 잡으면 진행 중인 수집의 파일일 수 있어 건너뛴다.
    fn sweep_while_idle(&self) -> KairosResult<Option<ToolResultsSweep>> {
        let lock = self.lock();
        if !lock.acquire()? {
            return Ok(None);
        }
        let swept = caught(|| self.sweep()).and_then(Result::ok);
        lock.release()?;
        Ok(swept)
    }

    /// kairos collect 본체. 소스를 모두 한 번씩 훑고 state.json을 갱신한다. `force`는 근무 시간과 주말 판정을 건너뛴다.
    pub fn run(&self, force: bool) -> KairosResult<CollectReport> {
        let home = &self.config.home;
        let now = self.clock.now();
        let started = self.clock.now();
        let mut report = CollectReport::new();

        home.ensure()?;
        if !force && !in_tick_window(now) {
            report.status = CollectStatus::Skipped;
            report.reason = Some("수집 시간(근무일 09:00~20:00) 밖".to_owned());
            report.tool_results = self.sweep_while_idle()?;
            return Ok(report);
        }
        if let Some(halt) = HaltFlag::new(home, self.clock.clone()).read() {
            report.status = CollectStatus::Halted;
            report.reason = Some(format!("수집 중단 중: {}", halt.reason));
            report.halted = true;
            report.tool_results = self.sweep_while_idle()?;
            return Ok(report);
        }
        let lock = self.lock();
        if !lock.acquire()? {
            report.status = CollectStatus::Busy;
            report.reason = Some("다른 수집이 실행 중".to_owned());
            return Ok(report);
        }

        let gate = TermGate::default();
        let term = TermWatch::install(gate.clone(), home, &self.config.user_home, lock.clone());
        let mut held = gate.hold();
        let stall = Arc::new(StallWatch::new(self.clock.clone()));
        let beats = Heartbeat::start(stall.clone());
        let live = LiveSleepProbe { stall, detector: SleepDetector::new(self.runner.clone()) };
        let sleep: &dyn SleepProbe = self.sleep.as_deref().unwrap_or(&live);
        let sleeps_before = held.outside(|| sleep.count());

        // 원본도 여기는 try 밖이다. 열지 못하면 잠금 파일이 남고 다음 틱이 죽은 pid로 보고 넘겨받는다.
        let database = match Database::open(home) {
            Ok(database) => database,
            Err(error) => {
                drop(held);
                return Err(error);
            }
        };
        let inner = Locked { database: &database, now, started, sleep, sleeps_before };
        let outcome = catch_unwind(AssertUnwindSafe(|| self.collect_locked(&inner, &mut report, &mut held)));

        drop(beats);
        drop(held);
        drop(term);
        // 위에서 던진 경우의 정리. 죽어서 못 지운 파일은 다음 틱이 시간대와 무관하게 지운다.
        if report.tool_results.is_none() {
            self.sweep_into(&mut report);
        }
        drop(database);
        let released = lock.release();
        match outcome {
            Ok(result) => result?,
            Err(payload) => panic::resume_unwind(payload),
        }
        released?;
        Ok(report)
    }

    /// 못 지우면 None으로 남고 doctor의 남은 건수에 드러난다.
    fn sweep_into(&self, report: &mut CollectReport) {
        if let Some(Ok(swept)) = caught(|| self.sweep()) {
            report.tool_results = Some(swept);
        }
    }

    fn record(
        &self,
        ledger: &Ledger<'_>,
        options: &IngestOptions,
        report: &mut CollectReport,
        results: Vec<SourceResult>,
    ) -> KairosResult<()> {
        for result in results {
            let (source, query) = (result.source, result.query.clone());
            let saved = ledger.ingest(result, options)?;
            report.outcomes.push(OutcomeRow { source, query, outcome: saved.outcome, error_code: saved.error_code });
        }
        Ok(())
    }

    fn collect_locked(
        &self,
        tick: &Locked<'_>,
        report: &mut CollectReport,
        gate: &mut HeldGate<'_>,
    ) -> KairosResult<()> {
        let Locked { database, now, started, sleep, sleeps_before } = *tick;
        let home = &self.config.home;
        let runs = TickRunRepo::new(database.connection());
        let first_run_id = runs.last_run_id()?;
        let sources: Vec<&dyn Source> = self.sources.iter().map(AsRef::as_ref).collect();
        let control = ControlFile::new(home).read();
        let ledger = Ledger::new(database);
        let options = IngestOptions { now, extra_off_dates: control.off_date_set() };
        let remaining_ms = || TOTAL_LIMIT_MS - self.clock.now().ms_since(started);

        let mcp_sources: Vec<&dyn Source> =
            sources.iter().copied().filter(|source| matches!(source.mode(), SourceMode::Mcp(_))).collect();
        let fingerprint = gate.outside(|| self.claude.fingerprint())?;
        report.contract_test =
            !mcp_sources.is_empty() && fingerprint.is_some() && fingerprint != read_fingerprint(home);

        // 직접 소스와 수집 모드는 동시에 돈다. 소스는 서로 격리한다.
        let limit_ms = || self.config.source_limit_ms.min(remaining_ms()).max(1);
        let direct = DirectWork::start(database.connection(), &self.sources, now, limit_ms, &self.clock)?;
        let mode = ModeRun {
            connection: database.connection(),
            config: &self.config,
            runner: self.runner.as_ref(),
            clock: &self.clock,
            now,
            contract_test: report.contract_test,
        };
        let mcp = mode.collect(&mcp_sources, remaining_ms, gate)?;
        let direct_results = gate.outside(|| direct.finish());

        // 이 시점까지 source_run에 쓴 것은 수집 모드뿐이다.
        report.mode_outcomes = runs.runs_since(first_run_id)?;
        if let Some(verdict) = &mcp.verdict {
            report.mode = Some(ModeReport {
                num_turns: verdict.num_turns,
                cost_usd: verdict.cost_usd,
                elapsed_ms: mcp.elapsed_ms,
            });
        }

        // 표식을 먼저 쓴다. 아래 기록이 던져도 다음 틱이 모델을 다시 부르면 안 된다.
        if let Some(verdict) = mcp.verdict.as_ref().filter(|verdict| verdict.spend) {
            report.halted = true;
            report.reason = Some("수집 모드가 모델을 호출했거나 비용이 생겼다".to_owned());
            HaltFlag::new(home, self.clock.clone()).write(&Halt {
                at: Some(now.to_iso()),
                reason: "model_called".to_owned(),
                num_turns: verdict.num_turns,
                cost_usd: verdict.cost_usd,
            })?;
        }

        self.record(&ledger, &options, report, direct_results)?;
        for source in &sources {
            if matches!(source.mode(), SourceMode::NotConnected { .. }) {
                self.record(&ledger, &options, report, not_connected_results(*source, now))?;
            }
        }
        let recorded_all = mcp.results.is_empty();
        self.record(&ledger, &options, report, mcp.results)?;

        if let Some(passed) = fingerprint.as_deref().filter(|_| {
            // 계약 시험은 모든 질의가 기록을 남겼을 때만 통과로 센다.
            recorded_all && mcp.verdict.as_ref().is_some_and(|verdict| verdict.ok)
        }) {
            write_fingerprint(home, passed, now)?;
        }

        let gap_ms = sleep.gap_ms();
        let sleeps_after = gate.outside(|| sleep.count());
        report.slept = slept_during(gap_ms, sleeps_before, sleeps_after);
        let woke_at = report.slept.then(|| now.plus_ms(self.clock.now().ms_since(started)));
        if let Some(woke_at) = woke_at {
            runs.mark_slept(first_run_id, SLEEP_INTERRUPTED)?;
            report.mark_slept();
            ResumeFile::new(home).write(woke_at)?;
        }

        let ok_sources: BTreeSet<String> = runs
            .runs_since(first_run_id)?
            .into_iter()
            .filter(|row| row.outcome == Outcome::Ok)
            .map(|row| row.source.as_str().to_owned())
            .collect();
        let opened = ControlFile::new(home).latch_display(database.connection(), &sources, now, control)?;
        self.finish(tick, report, gate, &sources, (opened, ok_sources), woke_at)
    }

    /// 기록 뒤의 단계. 항목, 알림, 패널, 인증, 정비, 정리, 감시 파일, state.json 순서다.
    fn finish(
        &self,
        tick: &Locked<'_>,
        report: &mut CollectReport,
        gate: &mut HeldGate<'_>,
        sources: &[&dyn Source],
        (opened, ok_sources): (KairosControl, BTreeSet<String>),
        woke_at: Option<Instant>,
    ) -> KairosResult<()> {
        let Locked { database, now, .. } = *tick;
        let home = &self.config.home;
        let settler = Settler { database, home, sources, now };
        report.items = settler.settle_items(&opened, &ok_sources, self.items.as_ref());
        let guarded = self.notifier.as_deref().map(GuardedNotifier);
        let notifier = guarded.as_ref().map(|notifier| notifier as &dyn Notifier);
        report.notified = settler.alert_p0(&opened, notifier);
        let p1 = settler.alert_p1(notifier, woke_at);
        report.notified_p1 = p1.sent;
        report.merged_p1 = p1.merged;
        report.dropped_p1 = p1.dropped;
        report.failed_p1 = p1.failed;
        // 알림이 실제로 나간 틱에서만 연다. 그래서 묶음과 보류 규칙을 그대로 따른다.
        // 패널을 못 열어도 알림은 이미 갔고 틱은 끝까지 돈다.
        if let Some(panel) = self.panel.as_deref().filter(|_| report.notified + report.notified_p1 > 0) {
            report.panel_opened = gate.outside(|| caught(|| panel.open())).unwrap_or(false);
        }

        gate.outside(|| AuthFile::new(home).refresh(now, self.claude.as_ref()))?;
        Maintenance::new(database.connection(), home).run_daily(now, false)?;
        // 잠금 안에서 해야 진행 중인 수집의 파일을 지우지 않는다. 남은 건수를 감시자에게 넘기려고 watch보다 먼저 한다.
        self.sweep_into(report);
        let leftover = report.tool_results.map_or(0, |sweep| sweep.remaining);
        WatchFile::new(database.connection(), home).write(sources, now, leftover)?;
        let resumed_at = ResumeFile::new(home).read().map(Instant::to_iso);
        let state = StateBuilder::new(database.connection()).build(sources, now, &opened, resumed_at.as_deref())?;
        StateBuilder::write_state(&state, home)
    }
}
