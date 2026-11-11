//! 수집 모드가 부르는 kairos ingest와 page-info의 몸통. 입출력은 kairos-cli가 맡는다.
//! 어느 쪽도 외부 문자열을 출력하지 않는다. 호출 인자와 건수, 코드만 나간다.

use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use kairos_core::model::{
    CollectContext, McpResponse, McpSource, Outcome, Source, SourceMode, SourceName, SourceResult,
};
use kairos_core::ports::Clock;
use kairos_core::{Instant, KairosError, KairosResult, OffDates, js};
use kairos_ledger::{IngestOptions, Ledger};
use kairos_source_mcp::shared::ResultFrame;
use kairos_store::Database;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::planner::TickPlanner;

pub const MAX_PAGE_VALUE: usize = 4096;
const PAGE_FIELDS: [&str; 2] = ["nextPageToken", "pagination_info"];

/// kairos ingest의 입력. `is_ingest_payload`를 통과한 값만 만들어진다.
#[derive(Clone, Debug, PartialEq)]
pub struct IngestPayload {
    pub now: Instant,
    pub source: SourceName,
    pub responses: Vec<McpResponse>,
}

fn valid_response(response: &Value) -> bool {
    response.get("id").is_some_and(Value::is_string) && response.get("isError").is_some_and(Value::is_boolean)
}

/// 문자열이 아니거나 시각으로 읽히지 않는 now, 모르는 소스, 형식이 다른 응답을 거른다.
pub fn is_ingest_payload(value: &Value) -> bool {
    IngestPayload::from_value(value).is_some()
}

impl IngestPayload {
    pub fn from_value(value: &Value) -> Option<Self> {
        let now = Instant::parse(value.get("now")?.as_str()?)?;
        let source = value.get("source")?.as_str()?.parse().ok()?;
        let responses = value.get("responses")?.as_array()?;
        if !responses.iter().all(valid_response) {
            return None;
        }
        let responses = serde_json::from_value(Value::Array(responses.clone())).ok()?;
        Some(Self { now, source, responses })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IngestRow {
    pub query: String,
    pub outcome: Outcome,
    pub error_code: Option<String>,
    pub objects: usize,
    pub events: usize,
    pub closed: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestSummary {
    pub source: SourceName,
    pub results: Vec<IngestRow>,
}

/// 소스 1개의 응답을 파싱하고 저장한다.
pub struct ResponseIngestor<'a> {
    database: &'a Database,
    clock: Arc<dyn Clock>,
}

impl<'a> ResponseIngestor<'a> {
    pub fn new(database: &'a Database, clock: Arc<dyn Clock>) -> Self {
        Self { database, clock }
    }

    /// 파서가 예상 밖으로 멈춰도 기록 없이 사라지지 않게 질의마다 실패 행을 만든다.
    /// 패닉도 원본의 throw와 같다. catch가 받던 모든 실패를 받는다.
    pub fn parse_or_fail(
        &self,
        source: &dyn Source,
        mcp: &dyn McpSource,
        responses: &[McpResponse],
        context: &CollectContext,
    ) -> Vec<SourceResult> {
        let parsed = catch_unwind(AssertUnwindSafe(|| mcp.parse(responses, context)));
        if let Ok(Ok(results)) = parsed {
            return results;
        }
        let ids: BTreeSet<&str> =
            responses.iter().map(|response| response.id.split('#').next().unwrap_or(&response.id)).collect();
        source
            .queries()
            .iter()
            .filter(|spec| ids.contains(format!("{}:{}", source.name(), spec.id).as_str()))
            .map(|spec| {
                ResultFrame {
                    source: source.name(),
                    spec,
                    now: context.now,
                    pages: 0,
                    window_from: None,
                    window_to: None,
                    clock: self.clock.as_ref(),
                }
                .empty_result(Outcome::ParseError, Some("parser_threw"))
            })
            .collect()
    }

    /// MCP 소스가 아니면 `not_mcp_source`다. CLI는 이 코드를 사용법 오류로 낸다.
    pub fn ingest_responses(
        &self,
        source: &dyn Source,
        payload: &IngestPayload,
        extra_off_dates: &OffDates,
    ) -> KairosResult<IngestSummary> {
        let SourceMode::Mcp(mcp) = source.mode() else {
            return Err(KairosError::new("not_mcp_source"));
        };
        let context = TickPlanner::new(self.database.connection()).context_for(source, mcp, payload.now)?;
        let results = self.parse_or_fail(source, mcp, &payload.responses, &context);
        let options = IngestOptions { now: payload.now, extra_off_dates: extra_off_dates.clone() };
        let ledger = Ledger::new(self.database);
        let mut rows = Vec::with_capacity(results.len());
        for result in results {
            let query = result.query.clone();
            let report = ledger.ingest(result, &options)?;
            rows.push(IngestRow {
                query,
                outcome: report.outcome,
                error_code: report.error_code,
                objects: report.objects_stored,
                events: report.events_inserted,
                closed: report.closed,
            });
        }
        Ok(IngestSummary { source: source.name(), results: rows })
    }
}

/// 쪽 넘김에 쓰는 두 값만 고른다. 본문을 훅으로 돌려보내지 않는다.
pub fn page_info_of(text: Option<&str>) -> Map<String, Value> {
    let mut picked = Map::new();
    let Some(body) = text.and_then(|text| js::parse_json(text).ok()) else {
        return picked;
    };
    for field in PAGE_FIELDS {
        if let Some(value) = body.get(field).and_then(Value::as_str)
            && js::utf16_len(value) <= MAX_PAGE_VALUE
        {
            picked.insert(field.to_owned(), Value::from(value));
        }
    }
    picked
}
