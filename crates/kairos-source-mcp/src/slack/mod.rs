//! 설계 3.4. 응답은 JSON에 감싼 마크다운이다(측정).
//!
//! ```text
//! {"results": "# Search Results for: ...\n\n## Messages (N results)\n### Result 1 of N\n...",
//!  "pagination_info": "For the next page of results use cursor `...`"}
//! ```
//!
//! N은 쪽 단위이고 쪽마다 Result 1부터 다시 센다(측정). 0건은 `No results found.`다(측정).

mod events;
mod page;

use kairos_core::connector::SLACK_SEARCH;
use std::sync::Arc;

use kairos_core::Instant;
use kairos_core::model::{
    CollectContext, CursorState, McpCall, McpResponse, McpSource, Outcome, ParserFailed, QuerySpec, Source, SourceMode,
    SourceName, SourceResult,
};
use kairos_core::ports::Clock;
use serde_json::{Map, json};

use crate::shared::{JsonObject, ResultFrame, call_id, parse_queries, read_pages, self_identity};
use events::{dm_event, dm_objects, mention_event, own_message_event};
pub use page::{SlackPage, SlackRecord, parse_search_page};

/// 본인 Slack 사용자 ID. 검색어에 들어가므로 설정은 `U` + 영대문자, 숫자만 받는다(kairos-config).
pub(crate) fn me() -> &'static str {
    self_identity(SOURCE)
}
const SOURCE: SourceName = SourceName::Slack;
const INTERVAL_SECONDS: i64 = 180;
const OVERLAP_SECONDS: i64 = 600;
/// 첫 실행의 조회 범위. T0 이전이라 전부 baseline이다.
const FIRST_RUN_LOOKBACK_MS: i64 = 24 * 60 * 60 * 1000;
const PAGE_LIMIT: i64 = 20;
/// 이어 읽기는 읽은 마지막 건보다 이만큼 앞에서 시작한다. after는 초 단위이고 경계를 포함하는지 몰라 1초를 겹친다.
const RESUME_MARGIN_MS: i64 = 1000;

const MENTIONS: &str = "mentions";
const TO_ME: &str = "to_me";
const FROM_ME: &str = "from_me";

const PAGE_STALLED: &str = "page_cap_stalled";

fn query_text(query: &str) -> String {
    match query {
        MENTIONS => format!("<@{}>", me()),
        TO_ME => "to:me".to_owned(),
        _ => format!("from:<@{}>", me()),
    }
}

/// 결과가 오름차순이어야 쪽 상한의 계산을 믿을 수 있다.
fn is_ascending(records: &[SlackRecord]) -> bool {
    records.windows(2).all(|pair| number(&pair[1].ts) >= number(&pair[0].ts))
}

/// `Number(ts)`. ts는 `\d+\.\d+`로 확인한 값이다.
fn number(ts: &str) -> f64 {
    ts.parse().unwrap_or(f64::INFINITY)
}

/// watermark - overlap부터 읽되, 상한에 잘려 이어 읽는 중이면 그 자리(progress_at)부터 읽는다.
fn search_start(context: &CollectContext, query: &str) -> Instant {
    let cursor = context.cursor(query);
    let base = cursor.and_then(CursorState::window_start);
    let base = base.unwrap_or_else(|| context.now.minus_ms(FIRST_RUN_LOOKBACK_MS));
    let progress = cursor.and_then(|cursor| cursor.progress_at.as_deref()).and_then(Instant::parse);
    progress.map_or(base, |progress| base.max(progress))
}

fn search_args(query: &str, context: &CollectContext) -> Map<String, serde_json::Value> {
    let after = search_start(context, query).millis().div_euclid(1000);
    let mut args = Map::new();
    args.insert("query".to_owned(), json!(query_text(query)));
    args.insert("response_format".to_owned(), json!("detailed"));
    args.insert("include_bots".to_owned(), json!(false));
    args.insert("include_context".to_owned(), json!(false));
    args.insert("sort".to_owned(), json!("timestamp"));
    args.insert("sort_dir".to_owned(), json!("asc"));
    args.insert("after".to_owned(), json!(after.to_string()));
    args.insert("limit".to_owned(), json!(PAGE_LIMIT));
    args.insert("content_types".to_owned(), json!("messages"));
    args
}

/// 멘션, DM, 내 메시지를 검색으로 읽는 소스.
pub struct SlackSource {
    queries: Vec<QuerySpec>,
    clock: Arc<dyn Clock>,
}

impl SlackSource {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let queries =
            Vec::from([MENTIONS, TO_ME, FROM_ME].map(|id| QuerySpec::new(id, INTERVAL_SECONDS, OVERLAP_SECONDS)));
        Self { queries, clock }
    }

    fn parse_query(&self, spec: &QuerySpec, pages: &[&McpResponse], context: &CollectContext) -> SourceResult {
        let window_from = search_start(context, &spec.id);
        let mut frame = ResultFrame {
            source: SOURCE,
            spec,
            now: context.now,
            pages: pages.len() as i64,
            window_from: Some(window_from.to_iso()),
            window_to: Some(context.now.to_iso()),
            clock: self.clock.as_ref(),
        };
        let read = read_pages(pages, |body: JsonObject| parse_search_page(&body));
        let (parsed, failure) = (read.parsed, read.failure);
        let records: Vec<SlackRecord> = parsed.iter().flat_map(|page| page.records.iter().cloned()).collect();
        let declared: i64 = parsed.iter().map(|page| page.declared).sum();
        let quarantined: i64 = parsed.iter().map(|page| page.quarantined).sum();

        if let (Some(failure), true) = (failure, parsed.is_empty()) {
            return frame.empty_result(failure.outcome, Some(failure.error_code));
        }
        if !is_ascending(&records) {
            return frame.empty_result(Outcome::ParseError, Some("not_ascending"));
        }

        let stranger = records.iter().find(|record| record.user_id != me()).or(records.first());
        let identity = if spec.id == FROM_ME { stranger.map(|record| record.user_id.clone()) } else { None };

        let last_read = records.last();
        // 틱 마감으로 뒤쪽을 못 읽은 경우도 쪽 상한과 같다. 다른 실패는 읽은 범위를 믿지 않는다.
        let deadline = last_read.is_some() && failure.is_some_and(|failure| failure.error_code == "mode_deadline");
        // 쪽 상한에 걸리면 남은 건을 다음 틱에 읽는다. 오름차순이라 읽은 데까지는 빠짐이 없다.
        let capped = deadline || (failure.is_none() && parsed.last().is_some_and(|page| page.has_more));
        let window_to = match last_read {
            Some(record) if capped => record.at,
            _ => context.now,
        };
        // 다음 조회는 window_to에서 겹침만큼 앞이다. 읽은 데가 그 겹침 안이면 watermark로는 앞으로 가지 못한다.
        let overlapped = capped && window_to.minus_ms(spec.overlap_seconds * 1000) <= window_from;
        // 그때는 partial로 progress_at을 남겨 다음 틱이 읽은 마지막 건 바로 앞에서 이어 읽는다. 그 뒤의 건은 빠지지 않는다.
        let resume = capped.then(|| window_to.minus_ms(RESUME_MARGIN_MS)).filter(|resume| *resume > window_from);
        // 이어 읽을 자리마저 앞으로 가지 않으면(쪽 예산보다 많은 건이 같은 초에 몰림) 정말 멈춘 것이다. 공백으로 알린다.
        let stalled = overlapped && resume.is_none();
        let notice = if quarantined > 0 { Some("record_quarantined") } else { capped.then_some("page_cap") };
        let error_code =
            if stalled { Some(PAGE_STALLED) } else { failure.map(|failure| failure.error_code).or(notice) };
        let events = match spec.id.as_str() {
            MENTIONS => records.iter().map(mention_event).collect(),
            TO_ME => records.iter().map(dm_event).collect(),
            _ => records.iter().map(own_message_event).collect(),
        };
        let objects = if spec.id == TO_ME { dm_objects(&records) } else { Vec::new() };

        frame.window_to = Some(window_to.to_iso());
        let outcome = if failure.is_some() || overlapped { Outcome::Partial } else { Outcome::Ok };
        SourceResult {
            expected_count: Some(declared),
            parsed_count: records.len() as i64 + quarantined,
            identity,
            watermark: ((failure.is_none() || deadline) && !overlapped).then(|| window_to.to_iso()),
            progress_at: resume.filter(|_| outcome == Outcome::Partial).map(Instant::to_iso),
            objects,
            events,
            ..frame.empty_result(outcome, error_code)
        }
    }
}

impl Source for SlackSource {
    fn name(&self) -> SourceName {
        SOURCE
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::Mcp(self)
    }
}

impl McpSource for SlackSource {
    fn plan(&self, context: &CollectContext) -> Vec<McpCall> {
        self.queries
            .iter()
            .map(|spec| McpCall {
                id: call_id(SOURCE, &spec.id),
                query: spec.id.clone(),
                server: SLACK_SEARCH.server().to_owned(),
                tool: SLACK_SEARCH.tool().to_owned(),
                args: search_args(&spec.id, context),
                expand: None,
            })
            .collect()
    }

    fn parse(&self, responses: &[McpResponse], context: &CollectContext) -> Result<Vec<SourceResult>, ParserFailed> {
        Ok(parse_queries(SOURCE, &self.queries, responses, |spec, pages| self.parse_query(spec, pages, context)))
    }
}
