//! 소스 목록. 소스를 붙이는 방법은 `standard`에 한 줄을 더하는 것이다.
//!
//! 구현이 없는 소스도 목록에서 사라지지 않는다. not_connected로 남아 doctor와 source_run에 드러난다.
//! TS의 폴더 동적 로드는 명시적 등록으로 바뀌었다. 불러오기 실패는 이제 컴파일 오류다.

use std::sync::Arc;

use kairos_core::Instant;
use kairos_core::catalog::{self, Readiness};
use kairos_core::model::{NotConnectedSource, Outcome, Source, SourceName, SourceResult};
use kairos_core::ports::Clock;
use kairos_core::process::ProcessRunner;
use kairos_source_github::{GithubConfig, GithubSource};
use kairos_source_mcp::{CalendarSource, GmailSource, NotionSource, SlackSource};

const NO_IMPLEMENTATION: &str = "소스 구현 없음";
const DISABLED: &str = "설정에서 끔";
const NOT_CONFIGURED: &str = "본인 식별자 설정 없음(kairos connect)";

/// 불러오기에 실패한 소스 파일. 이름과 오류 코드만 둔다. doctor가 드러낸다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFailure {
    pub file: String,
    pub error_code: String,
}

/// 소스 이름 순서를 지키는 소스 목록.
#[derive(Clone)]
pub struct SourceRegistry {
    sources: Vec<Arc<dyn Source>>,
}

impl SourceRegistry {
    /// 이름 순서대로 줄을 세운다. 구현이 없는 이름은 `NotConnectedSource`가 채운다. 같은 이름은 뒤의 것이 이긴다.
    pub fn new(implemented: Vec<Arc<dyn Source>>) -> Self {
        let sources = SourceName::ALL
            .iter()
            .map(|name| {
                let found = implemented.iter().rev().find(|source| source.name() == *name).cloned();
                found.unwrap_or_else(|| Arc::new(NotConnectedSource::new(*name, NO_IMPLEMENTATION)))
            })
            .collect();
        Self { sources }
    }

    /// 운영에서 쓰는 소스. 켜고 끈 상태와 필요한 값은 소스 목록(`kairos_core::catalog`)이 정한다.
    /// 끈 소스와 값이 모자란 소스는 이유를 단 연결 없음으로 남긴다.
    pub fn standard(clock: Arc<dyn Clock>, runner: Arc<dyn ProcessRunner>, github: GithubConfig) -> Self {
        let profile = kairos_core::profile::current();
        let implemented: Vec<Arc<dyn Source>> = vec![
            Arc::new(CalendarSource::new(clock.clone())),
            Arc::new(GithubSource::new(runner, clock.clone(), github)),
            Arc::new(SlackSource::new(clock.clone())),
            Arc::new(GmailSource::new(clock.clone())),
            Arc::new(NotionSource::new(clock)),
        ];
        let sources = implemented
            .into_iter()
            .map(|source| -> Arc<dyn Source> {
                let name = source.name();
                match catalog::spec(name).readiness(profile) {
                    Readiness::Ready => source,
                    Readiness::Off => Arc::new(NotConnectedSource::new(name, DISABLED)),
                    Readiness::Missing(_) => Arc::new(NotConnectedSource::new(name, NOT_CONFIGURED)),
                }
            })
            .collect();
        Self::new(sources)
    }

    pub fn sources(&self) -> &[Arc<dyn Source>] {
        &self.sources
    }

    pub fn find(&self, name: SourceName) -> Option<&Arc<dyn Source>> {
        self.sources.iter().find(|source| source.name() == name)
    }
}

/// 연결이 없는 소스의 실행 기록. 조용히 빠지지 않도록 틱마다 ingest에 넘긴다.
pub fn not_connected_results(source: &dyn Source, now: Instant) -> Vec<SourceResult> {
    let at = now.to_iso();
    source
        .queries()
        .iter()
        .map(|query| SourceResult {
            source: source.name(),
            query: query.id.clone(),
            outcome: Outcome::NotConnected,
            started_at: at.clone(),
            finished_at: at.clone(),
            pages: 0,
            expected_count: None,
            parsed_count: 0,
            identity: None,
            window_from: None,
            window_to: None,
            watermark: None,
            progress_at: None,
            thread_reads: Vec::new(),
            overlap_seconds: query.overlap_seconds,
            snapshot: false,
            error_code: None,
            objects: Vec::new(),
            events: Vec::new(),
        })
        .collect()
}
