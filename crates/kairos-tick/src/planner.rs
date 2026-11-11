//! 이번 틱의 호출 목록. kairos plan-tick이 stdout JSON으로 낸다. DB에 쓰지 않는다.
//! 출력에는 호출 인자만 나간다. 외부 문자열을 내지 않는다.

use std::collections::{BTreeMap, BTreeSet};

use kairos_core::model::{CollectContext, McpCall, McpSource, QuerySpec, Source, SourceMode, SourceName};
use kairos_core::ports::CancelToken;
use kairos_core::{Instant, KairosResult, OffDates};
use kairos_ledger::{CursorRepo, SourceRunRepo, ThreadReadRepo};
use kairos_policy::{tick_end_minutes, tick_start_minutes};
use kairos_store::Connection;
use serde::Serialize;

/// 09:00~20:00 밖과 비근무일에는 소스 주기가 이보다 짧아도 이 간격으로만 호출한다.
pub const OFF_HOURS_INTERVAL_SECONDS: i64 = 3600;
/// launchd는 3분 격자로 돈다. 앞 실행의 끝 시각 기준이라 180초가 조금 안 된 틱도 제 차례다.
pub const DUE_TOLERANCE_SECONDS: i64 = 45;
pub const PLAN_VERSION: i64 = 1;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TickPlan {
    pub version: i64,
    pub now: Instant,
    pub calls: Vec<McpCall>,
}

#[derive(Clone, Copy, Debug)]
pub struct TickPlanner<'a> {
    connection: &'a Connection,
}

impl<'a> TickPlanner<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub fn is_collecting_hours(now: Instant, off_dates: &OffDates) -> bool {
        off_dates.is_workday(now.local_date())
            && (tick_start_minutes()..tick_end_minutes()).contains(&now.local_minutes())
    }

    /// 근무 시간에는 마지막 ok로부터 잰다. 실패한 질의는 주기를 기다리지 않고 다음 틱에 다시 시도한다.
    /// 근무 시간 밖에는 시도 자체를 줄이므로 마지막 시도로부터 잰다.
    fn is_due(&self, source: SourceName, query: &QuerySpec, now: Instant, collecting: bool) -> KairosResult<bool> {
        let Some(last) = SourceRunRepo::new(self.connection).latest(source, &query.id, collecting)? else {
            return Ok(true);
        };
        let interval =
            if collecting { query.interval_seconds } else { query.interval_seconds.max(OFF_HOURS_INTERVAL_SECONDS) };
        // 해석할 수 없는 시각은 NaN이라 비교가 거짓이다.
        let elapsed = Instant::parse(&last.finished_at).map(|finished| now.ms_since(finished));
        Ok(elapsed.is_some_and(|elapsed| elapsed >= (interval - DUE_TOLERANCE_SECONDS) * 1000))
    }

    pub fn due_queries(
        &self,
        source: &dyn Source,
        now: Instant,
        off_dates: &OffDates,
        force: bool,
    ) -> KairosResult<Vec<QuerySpec>> {
        if force {
            return Ok(source.queries().to_vec());
        }
        let collecting = Self::is_collecting_hours(now, off_dates);
        let mut due = Vec::new();
        for query in source.queries() {
            if self.is_due(source.name(), query, now, collecting)? {
                due.push(query.clone());
            }
        }
        Ok(due)
    }

    /// 소스가 받는 맥락. 소스는 DB를 직접 열지 않는다.
    pub fn context_for(&self, source: &dyn Source, mcp: &dyn McpSource, now: Instant) -> KairosResult<CollectContext> {
        let mut context = self.cursors_at(source, now)?;
        if mcp.needs_thread_reads() {
            let reads = ThreadReadRepo::new(self.connection);
            let mut by_query = BTreeMap::new();
            for query in source.queries() {
                by_query.insert(query.id.clone(), reads.list(source.name(), &query.id)?);
            }
            context.thread_reads = Some(by_query);
        }
        Ok(context)
    }

    /// 커서만 든 맥락. 직접 소스는 이것을 들고 다른 스레드로 간다.
    pub(crate) fn cursors_at(&self, source: &dyn Source, now: Instant) -> KairosResult<CollectContext> {
        let repo = CursorRepo::new(self.connection);
        let mut cursors = BTreeMap::new();
        for query in source.queries() {
            cursors.insert(query.id.clone(), repo.get(source.name(), &query.id)?);
        }
        Ok(CollectContext { now, cursors, thread_reads: None, cancel: CancelToken::new() })
    }

    /// `force`는 계약 시험(설계 9.5)이다. 주기와 무관하게 질의마다 호출 1회를 확인한다.
    pub fn plan_tick(
        &self,
        sources: &[&dyn Source],
        now: Instant,
        off_dates: &OffDates,
        force: bool,
    ) -> KairosResult<TickPlan> {
        let mut calls = Vec::new();
        for source in sources {
            let SourceMode::Mcp(mcp) = source.mode() else {
                continue;
            };
            let due: BTreeSet<String> =
                self.due_queries(*source, now, off_dates, force)?.into_iter().map(|query| query.id).collect();
            let context = self.context_for(*source, mcp, now)?;
            calls.extend(mcp.plan(&context).into_iter().filter(|call| due.contains(&call.query)));
        }
        Ok(TickPlan { version: PLAN_VERSION, now, calls })
    }
}
