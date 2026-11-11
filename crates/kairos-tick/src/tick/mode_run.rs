//! 수집 모드가 필요한 소스를 한 번의 claude 실행으로 모은다.
//! 실제 저장은 모드가 `kairos ingest`로 직접 한다. 여기서는 실행 계약과 기록 유무만 검증한다.

use std::collections::BTreeMap;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kairos_agenda::ControlFile;
use kairos_core::home::DIR_MODE;
use kairos_core::model::{Outcome, QuerySpec, Source, SourceResult};
use kairos_core::ports::Clock;
use kairos_core::process::ProcessRunner;
use kairos_core::{Instant, KairosResult};
use kairos_ledger::OffDateBook;
use kairos_ops::collect_cwd;
use kairos_store::Connection;

use super::TickConfig;
use super::direct::{empty_result, failure_results};
use super::term::HeldGate;
use crate::mode::{ClaudeMode, RunVerdict, judge_claude_run};
use crate::planner::TickPlanner;
use crate::repo::TickRunRepo;

#[derive(Default)]
pub(super) struct McpCollection {
    pub results: Vec<SourceResult>,
    pub verdict: Option<RunVerdict>,
    pub elapsed_ms: i64,
}

pub(super) struct ModeRun<'a> {
    pub connection: &'a Connection,
    pub config: &'a TickConfig,
    pub runner: &'a dyn ProcessRunner,
    pub clock: &'a Arc<dyn Clock>,
    pub now: Instant,
    pub contract_test: bool,
}

type DueQueries<'s> = Vec<(&'s dyn Source, Vec<QuerySpec>)>;

impl ModeRun<'_> {
    fn fail(&self, sources: &[&dyn Source], outcome: Outcome, code: &str) -> McpCollection {
        let results = sources
            .iter()
            .flat_map(|source| failure_results(*source, outcome, code, self.now, self.clock.as_ref()))
            .collect();
        McpCollection { results, ..McpCollection::default() }
    }

    fn due<'s>(&self, sources: &[&'s dyn Source]) -> KairosResult<DueQueries<'s>> {
        // 주기는 plan-tick이 정한다. 이번 틱에 차례가 아닌 질의는 기록이 없는 것이 정상이다.
        let control = ControlFile::new(&self.config.home).read();
        let off_dates = OffDateBook::new(self.connection).load_watch_off_dates(&control.off_date_set())?;
        let planner = TickPlanner::new(self.connection);
        sources
            .iter()
            .map(|source| Ok((*source, planner.due_queries(*source, self.now, &off_dates, self.contract_test)?)))
            .collect()
    }

    /// 차례인 질의 중 기록이 없는 것만 실패로 남긴다. 차례가 아닌 질의는 시도하지 않았고, 이미 기록한 결과는 덮지 않는다.
    fn unrecorded(
        &self,
        due: &DueQueries<'_>,
        before: i64,
        outcome: Outcome,
        code: &str,
    ) -> KairosResult<Vec<SourceResult>> {
        let runs = TickRunRepo::new(self.connection);
        let mut results = Vec::new();
        for (source, queries) in due {
            for query in queries {
                if !runs.recorded_since(source.name(), &query.id, before)? {
                    results.push(empty_result(*source, &query.id, outcome, code, self.now, self.clock.as_ref()));
                }
            }
        }
        Ok(results)
    }

    pub(super) fn collect(
        &self,
        sources: &[&dyn Source],
        remaining_ms: impl Fn() -> i64,
        gate: &mut HeldGate<'_>,
    ) -> KairosResult<McpCollection> {
        if sources.is_empty() {
            return Ok(McpCollection::default());
        }
        let Some(bin) = self.config.claude_bin.as_deref().filter(|bin| !bin.is_empty()) else {
            return Ok(self.fail(sources, Outcome::ToolMissing, "claude_missing"));
        };
        let plugin = self.config.plugin_dir.as_deref().filter(|dir| !dir.is_empty() && Path::new(dir).exists());
        let Some(plugin) = plugin else {
            return Ok(self.fail(sources, Outcome::ToolMissing, "mode_plugin_missing"));
        };
        let due = self.due(sources)?;
        if due.iter().all(|(_, queries)| queries.is_empty()) {
            return Ok(McpCollection::default());
        }

        let cwd = collect_cwd(&self.config.home);
        DirBuilder::new().recursive(true).mode(DIR_MODE).create(&cwd)?;
        let before = TickRunRepo::new(self.connection).last_run_id()?;
        let started = self.clock.now();
        let extra = BTreeMap::from([
            ("KAIROS_HOME".to_owned(), self.config.home.root().to_string_lossy().into_owned()),
            ("KAIROS_CONTRACT_TEST".to_owned(), if self.contract_test { "1" } else { "0" }.to_owned()),
            ("KAIROS_TICK_STARTED_AT".to_owned(), started.millis().to_string()),
        ]);
        let env = ClaudeMode::env(&self.config.env, extra);
        let limit_ms = self.config.source_limit_ms.min(remaining_ms()).max(1);
        let timeout = Duration::from_millis(limit_ms as u64);
        let args = ClaudeMode::args(plugin);
        let run = gate.outside(|| ClaudeMode::launch(self.runner, bin, args, env, &cwd, timeout));
        let elapsed_ms = self.clock.now().ms_since(started);
        let verdict = judge_claude_run(&run);
        if !verdict.ok {
            let code = verdict.error_code.as_deref().unwrap_or("claude_failed");
            let results = self.unrecorded(&due, before, verdict.outcome, code)?;
            return Ok(McpCollection { results, verdict: Some(verdict), elapsed_ms });
        }
        // 결과 JSON만으로 성공을 믿지 않는다. 소스 질의마다 새 기록이 있는지 본다.
        let outcome = if self.contract_test { Outcome::ContractBreak } else { Outcome::ParseError };
        let results = self.unrecorded(&due, before, outcome, "no_result_recorded")?;
        Ok(McpCollection { results, verdict: Some(verdict), elapsed_ms })
    }
}
