use kairos_core::Instant;
use kairos_core::model::{Outcome, QuerySpec, SourceName, SourceResult};
use kairos_core::ports::Clock;

/// 질의 1회 결과의 틀. 소스가 값을 채우기 전의 공통 값이다.
pub struct ResultFrame<'a> {
    pub source: SourceName,
    pub spec: &'a QuerySpec,
    pub now: Instant,
    pub pages: i64,
    pub window_from: Option<String>,
    pub window_to: Option<String>,
    /// `finished_at`은 틀을 만든 시각이 아니라 결과를 낸 시각이다.
    pub clock: &'a dyn Clock,
}

impl ResultFrame<'_> {
    /// 기록이 없는 결과. 실패와 0건의 바탕이다.
    pub fn empty_result(&self, outcome: Outcome, error_code: Option<&str>) -> SourceResult {
        SourceResult {
            source: self.source,
            query: self.spec.id.clone(),
            outcome,
            started_at: self.now.to_iso(),
            finished_at: self.clock.now().to_iso(),
            pages: self.pages,
            expected_count: None,
            parsed_count: 0,
            identity: None,
            window_from: self.window_from.clone(),
            window_to: self.window_to.clone(),
            watermark: None,
            progress_at: None,
            thread_reads: Vec::new(),
            overlap_seconds: self.spec.overlap_seconds,
            snapshot: false,
            error_code: error_code.map(str::to_owned),
            objects: Vec::new(),
            events: Vec::new(),
        }
    }
}
