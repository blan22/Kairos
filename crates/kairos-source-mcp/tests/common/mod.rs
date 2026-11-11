//! 시험의 고정 시각.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use kairos_core::Instant;
use kairos_core::model::{
    CollectContext, CursorState, McpResponse, McpSource, ParserFailed, Source, SourceName, SourceResult,
    ThreadReadState,
};
use kairos_core::ports::{CancelToken, Clock, FixedClock};
use serde_json::{Value, json};

/// 시험 프로필(`Profile::fixture`)의 본인 이름.
const SELF_NAME: &str = "김준서";

pub const T0: &str = "2026-03-02T01:00:00.000Z"; // 월요일 10:00 KST
pub const T1: &str = "2026-03-02T01:03:00.000Z";
pub const T2: &str = "2026-03-02T01:06:00.000Z";
pub const T3: &str = "2026-03-02T01:09:00.000Z";
pub const BEFORE: &str = "2026-03-01T01:00:00.000Z";

pub const ME: &str = "U0SYNTH0001";
pub const SELF_EMAIL: &str = "kairos-dev@contoso.example";
pub const END_OF_RESULTS: &str = "End of results - No more pages available.\\n";

pub fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

pub fn clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock(at(T3)))
}

/// 고정값은 합성 데이터이고 신원은 자리표시자다. 시험 때 코드의 상수로 채운다.
pub fn fixture(path: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path);
    std::fs::read_to_string(file)
        .unwrap()
        .replace("__ME__", ME)
        .replace("__SELF_EMAIL__", SELF_EMAIL)
        .replace("__SELF_NAME__", SELF_NAME)
}

pub fn json_fixture(path: &str) -> Value {
    serde_json::from_str(&fixture(path)).unwrap()
}

pub fn json_response(id: &str, body: &Value) -> McpResponse {
    McpResponse { id: id.to_owned(), is_error: false, content: json!([{ "type": "text", "text": body.to_string() }]) }
}

pub fn error_response(id: &str, text: &str) -> McpResponse {
    McpResponse { id: id.to_owned(), is_error: true, content: json!([{ "type": "text", "text": text }]) }
}

pub fn text_response(id: &str, text: &str) -> McpResponse {
    McpResponse { id: id.to_owned(), is_error: false, content: json!([{ "type": "text", "text": text }]) }
}

pub fn next_cursor(cursor: &str) -> String {
    format!("For the next page of results use cursor `{cursor}`\n")
}

pub fn slack_body(results: &str, pagination: &str) -> Value {
    json!({ "results": results, "pagination_info": pagination })
}

pub fn cursor(source: SourceName, query: &str, watermark: &str, overlap_seconds: i64, t0: &str) -> CursorState {
    CursorState {
        source,
        query: query.to_owned(),
        watermark: Some(watermark.to_owned()),
        overlap_seconds,
        t0: Some(t0.to_owned()),
        first_run_at: None,
        progress_at: None,
    }
}

pub fn context_of(source: &dyn Source, now: &str, cursors: Vec<CursorState>) -> CollectContext {
    let mut map: BTreeMap<String, Option<CursorState>> =
        source.queries().iter().map(|query| (query.id.clone(), None)).collect();
    for state in cursors {
        map.insert(state.query.clone(), Some(state));
    }
    CollectContext { now: at(now), cursors: map, thread_reads: None, cancel: CancelToken::new() }
}

pub fn with_reads(mut context: CollectContext, reads: Vec<ThreadReadState>) -> CollectContext {
    context.thread_reads = Some(BTreeMap::from([("mail".to_owned(), reads)]));
    context
}

pub fn one(results: Result<Vec<SourceResult>, ParserFailed>) -> SourceResult {
    let mut results = results.unwrap();
    assert_eq!(results.len(), 1);
    results.remove(0)
}

pub fn parse_one(source: &dyn McpSource, responses: &[McpResponse], context: &CollectContext) -> SourceResult {
    one(source.parse(responses, context))
}

/// 결과 전체를 글자로 편다. 저장되면 안 되는 글자가 소스 밖으로 나가는지 본다.
pub fn dump(result: &SourceResult) -> String {
    format!("{result:?}")
}
