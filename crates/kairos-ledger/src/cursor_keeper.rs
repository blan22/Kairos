use kairos_core::model::SourceResult;
use kairos_core::{Instant, KairosResult};
use kairos_store::Connection;

use crate::cleaning::canonical_time;
use crate::repo::{CursorRepo, ThreadReadRepo};

/// 커서 전진, 진행 표시, 후속 읽기 기록.
pub(crate) struct CursorKeeper<'a> {
    cursors: CursorRepo<'a>,
    thread_reads: ThreadReadRepo<'a>,
}

/// 후보가 저장된 값보다 앞서 있지 않다. 어느 쪽이든 해석할 수 없으면 NaN이라 거짓이다.
fn not_ahead(candidate: Option<Instant>, current: Option<&str>) -> bool {
    match (candidate, current.and_then(Instant::parse)) {
        (Some(candidate), Some(current)) => candidate <= current,
        _ => false,
    }
}

impl<'a> CursorKeeper<'a> {
    pub(crate) const fn new(connection: &'a Connection) -> Self {
        Self { cursors: CursorRepo::new(connection), thread_reads: ThreadReadRepo::new(connection) }
    }

    /// 저장한 첫 실행의 시각을 남긴다. 첫 ok가 늦어져도 그 사이의 사건이 baseline으로 묻히지 않는다.
    pub(crate) fn mark_first_run(&self, result: &SourceResult, at: &str) -> KairosResult<()> {
        self.cursors.mark_first_run(result.source, &result.query, result.overlap_seconds, at)
    }

    pub(crate) fn advance_progress(&self, result: &SourceResult) -> KairosResult<()> {
        let Some(progress) = result.progress_at.as_deref() else {
            return Ok(());
        };
        let cursor = self.cursors.get(result.source, &result.query)?;
        let current = cursor.as_ref().and_then(|cursor| cursor.progress_at.as_deref());
        if not_ahead(Instant::parse(progress), current) {
            return Ok(());
        }
        self.cursors.set_progress(result.source, &result.query, &canonical_time(progress)?)
    }

    pub(crate) fn record_thread_reads(&self, result: &SourceResult, now: &str) -> KairosResult<()> {
        for read in &result.thread_reads {
            self.thread_reads.record(result.source, &result.query, read, now)?;
        }
        Ok(())
    }

    /// ok 실행의 전진. watermark가 움직였으면 참이다. 과거로는 움직이지 않는다.
    pub(crate) fn advance(&self, result: &SourceResult, t0: &str) -> KairosResult<bool> {
        self.cursors.settle(result.source, &result.query, result.overlap_seconds, t0)?;
        let Some(watermark) = result.watermark.as_deref() else {
            return Ok(false);
        };
        let cursor = self.cursors.get(result.source, &result.query)?;
        let current = cursor.as_ref().and_then(|cursor| cursor.watermark.as_deref());
        if not_ahead(Instant::parse(watermark), current) {
            return Ok(false);
        }
        self.cursors.set_watermark(result.source, &result.query, &canonical_time(watermark)?)?;
        Ok(true)
    }
}
