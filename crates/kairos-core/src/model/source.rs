use std::collections::BTreeMap;

use super::{CursorState, McpCall, McpResponse, SourceName, SourceResult, ThreadReadState};
use crate::Instant;
use crate::ports::CancelToken;

/// 수집 주기는 설계 3.1, overlap은 3.2를 따른다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuerySpec {
    pub id: String,
    pub interval_seconds: i64,
    pub overlap_seconds: i64,
}

impl QuerySpec {
    pub fn new(id: impl Into<String>, interval_seconds: i64, overlap_seconds: i64) -> Self {
        Self { id: id.into(), interval_seconds, overlap_seconds }
    }
}

/// 소스가 받는 맥락. 소스는 DB를 직접 열지 않는다.
#[derive(Clone, Debug)]
pub struct CollectContext {
    pub now: Instant,
    /// 질의 id별 커서. 첫 실행이면 None이다.
    pub cursors: BTreeMap<String, Option<CursorState>>,
    /// 질의 id별 후속 읽기 기록. `McpSource::needs_thread_reads`인 소스에만 채운다.
    pub thread_reads: Option<BTreeMap<String, Vec<ThreadReadState>>>,
    /// 소스별 상한(20초)에 걸리면 취소된다.
    pub cancel: CancelToken,
}

impl CollectContext {
    pub fn cursor(&self, query: &str) -> Option<&CursorState> {
        self.cursors.get(query).and_then(Option::as_ref)
    }
}

/// 파서가 예상 밖의 입력으로 멈췄다. 호출자가 질의마다 실패 행을 만든다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParserFailed;

/// 수집 방식에 따라 나뉘는 소스의 얼굴.
pub enum SourceMode<'a> {
    /// gh처럼 직접 실행하는 소스. 내부 프로세스는 argv 방식으로만 띄운다.
    Direct(&'a dyn DirectSource),
    /// 수집 모드를 거치는 소스. plan-tick이 `plan`을 부르고, ingest 단계에서 `parse`를 부른다.
    Mcp(&'a dyn McpSource),
    /// 연결이 없는 소스. 틱마다 not_connected 실행 기록을 남긴다.
    NotConnected { reason: &'a str },
}

/// 모든 소스의 공통 얼굴. 새 소스는 이것과 `DirectSource` 또는 `McpSource`를 구현해 등록한다.
pub trait Source: Send + Sync {
    fn name(&self) -> SourceName;
    fn queries(&self) -> &[QuerySpec];
    fn mode(&self) -> SourceMode<'_>;
}

pub trait DirectSource: Send + Sync {
    fn collect(&self, context: &CollectContext) -> Vec<SourceResult>;
}

pub trait McpSource: Send + Sync {
    /// 참이면 plan-tick이 `CollectContext::thread_reads`를 DB에서 채운다.
    fn needs_thread_reads(&self) -> bool {
        false
    }
    fn plan(&self, context: &CollectContext) -> Vec<McpCall>;
    fn parse(&self, responses: &[McpResponse], context: &CollectContext) -> Result<Vec<SourceResult>, ParserFailed>;
}

/// 구현이 없는 소스의 자리표시자. 목록에서 조용히 빠지지 않게 한다.
pub struct NotConnectedSource {
    name: SourceName,
    reason: String,
    queries: Vec<QuerySpec>,
}

impl NotConnectedSource {
    pub fn new(name: SourceName, reason: impl Into<String>) -> Self {
        Self { name, reason: reason.into(), queries: vec![QuerySpec::new("default", 180, 0)] }
    }
}

impl Source for NotConnectedSource {
    fn name(&self) -> SourceName {
        self.name
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::NotConnected { reason: &self.reason }
    }
}
