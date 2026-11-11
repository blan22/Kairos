//! 직접 소스를 스레드에서 돌린다. 소스는 서로 격리한다. 한 소스의 예외나 시간 초과가 다른 소스를 막지 않는다.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant as Monotonic};

use kairos_core::model::{CollectContext, Outcome, Source, SourceMode, SourceResult};
use kairos_core::ports::Clock;
use kairos_core::{Instant, KairosResult};
use kairos_store::Connection;

use super::guard::caught;
use crate::planner::TickPlanner;

/// 벽시계를 다시 보는 간격. macOS의 monotonic 시계는 잠자기 동안 멈춰서, 이것만 보면 깨어난 뒤에도 남은 시간을 더 기다린다.
const WAIT_SLICE_MS: i64 = 500;

pub(super) fn empty_result(
    source: &dyn Source,
    query_id: &str,
    outcome: Outcome,
    error_code: &str,
    now: Instant,
    clock: &dyn Clock,
) -> SourceResult {
    let overlap = source.queries().iter().find(|query| query.id == query_id).map_or(0, |query| query.overlap_seconds);
    SourceResult {
        source: source.name(),
        query: query_id.to_owned(),
        outcome,
        started_at: now.to_iso(),
        finished_at: clock.now().to_iso(),
        pages: 0,
        expected_count: None,
        parsed_count: 0,
        identity: None,
        window_from: None,
        window_to: None,
        watermark: None,
        progress_at: None,
        thread_reads: Vec::new(),
        overlap_seconds: overlap,
        snapshot: false,
        error_code: Some(error_code.to_owned()),
        objects: Vec::new(),
        events: Vec::new(),
    }
}

pub(super) fn failure_results(
    source: &dyn Source,
    outcome: Outcome,
    error_code: &str,
    now: Instant,
    clock: &dyn Clock,
) -> Vec<SourceResult> {
    source.queries().iter().map(|query| empty_result(source, &query.id, outcome, error_code, now, clock)).collect()
}

enum Waited {
    Finished(Vec<SourceResult>),
    Threw,
    TimedOut,
}

/// 한도 안에서 기다린다. 한도를 넘긴 뒤 도착한 결과는 받지 않는다.
fn wait_within(results: &Receiver<Option<Vec<SourceResult>>>, limit_ms: i64, clock: &dyn Clock) -> Waited {
    let begun = Monotonic::now();
    let begun_at = clock.now();
    loop {
        let used = (begun.elapsed().as_millis() as i64).max(clock.now().ms_since(begun_at));
        let left = limit_ms - used;
        if left <= 0 {
            return Waited::TimedOut;
        }
        let slice = Duration::from_millis(left.min(WAIT_SLICE_MS) as u64);
        match results.recv_timeout(slice) {
            Ok(Some(results)) => return Waited::Finished(results),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => return Waited::Threw,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

/// 원본의 `withLimit`. 한도에 걸리면 취소하고, 소스가 신호를 무시해도 약속한 시간에 돌려준다. 그 스레드는 버린다.
/// 취소한 뒤의 결과는 보지 않으므로 GitHub 소스가 내는 gh_timeout은 기록에 남지 않고 source_timeout만 남는다.
fn collect_within(
    source: Arc<dyn Source>,
    context: CollectContext,
    limit_ms: i64,
    now: Instant,
    clock: Arc<dyn Clock>,
) -> Vec<SourceResult> {
    let cancel = context.cancel.clone();
    let (sender, results) = mpsc::channel();
    let worker = source.clone();
    thread::spawn(move || {
        let collected = caught(|| match worker.mode() {
            SourceMode::Direct(direct) => direct.collect(&context),
            _ => Vec::new(),
        });
        let _ = sender.send(collected);
    });
    match wait_within(&results, limit_ms, clock.as_ref()) {
        Waited::Finished(results) => results,
        Waited::TimedOut => {
            cancel.cancel();
            failure_results(source.as_ref(), Outcome::Timeout, "source_timeout", now, clock.as_ref())
        }
        // 오류 메시지에 외부 문자열이 섞일 수 있어 코드만 남긴다.
        Waited::Threw => failure_results(source.as_ref(), Outcome::ParseError, "source_threw", now, clock.as_ref()),
    }
}

/// 돌고 있는 직접 소스들.
pub(super) struct DirectWork {
    running: Vec<JoinHandle<Vec<SourceResult>>>,
}

impl DirectWork {
    /// 커서는 여기서 미리 읽어 넘긴다. 연결은 메인 스레드 밖으로 나가지 않는다.
    pub(super) fn start(
        connection: &Connection,
        sources: &[Arc<dyn Source>],
        now: Instant,
        limit_ms: impl Fn() -> i64,
        clock: &Arc<dyn Clock>,
    ) -> KairosResult<Self> {
        let planner = TickPlanner::new(connection);
        let mut running = Vec::new();
        for source in sources {
            if !matches!(source.mode(), SourceMode::Direct(_)) {
                continue;
            }
            let context = planner.cursors_at(source.as_ref(), now)?;
            let (source, clock, limit) = (source.clone(), clock.clone(), limit_ms());
            running.push(thread::spawn(move || collect_within(source, context, limit, now, clock)));
        }
        Ok(Self { running })
    }

    /// 지키는 스레드는 한도 안에 끝난다. 소스 순서를 유지한다.
    pub(super) fn finish(self) -> Vec<SourceResult> {
        self.running.into_iter().flat_map(|work| work.join().unwrap_or_default()).collect()
    }
}
