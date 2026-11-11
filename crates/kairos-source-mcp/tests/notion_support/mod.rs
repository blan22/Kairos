//! Notion 시험의 도우미.
#![allow(dead_code)]

use kairos_core::model::{CursorState, McpResponse, SourceName, SourceResult, ThreadReadState};
use kairos_source_mcp::NotionSource;
use kairos_source_mcp::notion::{MailBlock, parse_mail};
use serde_json::{Map, Value, json};

use crate::common::*;

pub const SENDER: &str = "notify@mail.notion.com";
pub const SEARCH: &str = "notion:mail";
pub const BACKLOG: &str = "notion:mail~old";
pub const DIGEST_SUBJECT: &str = "합성 사용자 외 2명이 Contoso에서 업데이트함";
pub const DATE: &str = "2026-03-02T00:30:00.000Z";
pub const LINK: &str = "https://mg.mail.notion.com/c/synthetic";
pub const EDIT: &str = "이합성 이 페이지 페이지를 편집했습니다.";
pub const DAY_MS: i64 = 86_400_000;

pub fn source() -> NotionSource {
    NotionSource::new(clock())
}

pub fn digest() -> String {
    fixture("notion/digest.txt")
}

pub fn search_thread(id: &str, sender: &str, message_count: i64) -> Value {
    json!({
        "id": id,
        "messageCount": message_count,
        "messages": [{ "id": format!("m-{id}"), "sender": sender, "threadId": id, "toRecipients": ["synthetic@example.com"] }],
    })
}

fn threads_of(ids: &[&str]) -> Vec<Value> {
    ids.iter().map(|id| search_thread(id, SENDER, 1)).collect()
}

pub fn search(ids: &[&str]) -> McpResponse {
    search_with(ids, json!({}))
}

pub fn search_with(ids: &[&str], extra: Value) -> McpResponse {
    let mut body = json!({ "resultCountEstimate": ids.len(), "threads": threads_of(ids) });
    merge(&mut body, extra);
    json_response(SEARCH, &body)
}

pub fn old_search(ids: &[&str]) -> McpResponse {
    old_search_with(ids, json!({}))
}

pub fn old_search_with(ids: &[&str], extra: Value) -> McpResponse {
    let mut body = json!({ "threads": threads_of(ids) });
    merge(&mut body, extra);
    json_response(BACKLOG, &body)
}

fn merge(body: &mut Value, extra: Value) {
    if let (Some(body), Value::Object(extra)) = (body.as_object_mut(), extra) {
        body.extend(extra);
    }
}

pub fn message(body: &str) -> Value {
    message_with(body, json!({}))
}

pub fn message_with(body: &str, over: Value) -> Value {
    let mut value = json!({
        "id": "m",
        "sender": SENDER,
        "subject": DIGEST_SUBJECT,
        "date": DATE,
        "internalDate": at(DATE).millis().to_string(),
        "plaintextBody": body,
        "snippet": "합성 미리보기 저장금지",
        "viewUrl": "https://mail.google.com/",
    });
    merge(&mut value, over);
    value
}

pub fn thread_response_with(id: &str, messages: Vec<Value>) -> McpResponse {
    let count = messages.len();
    json_response(
        &format!("{SEARCH}!{id}"),
        &json!({ "id": id, "messageCount": count, "messages": messages, "viewUrl": "https://mail.google.com/" }),
    )
}

pub fn thread_response(id: &str) -> McpResponse {
    thread_response_with(id, vec![message(&digest())])
}

pub fn state(external_id: &str) -> ThreadReadState {
    ThreadReadState {
        external_id: external_id.to_owned(),
        message_count: Some(1),
        failures: 0,
        first_failed_at: None,
        quarantined_at: None,
    }
}

pub fn reads(known: &[&str]) -> Vec<ThreadReadState> {
    known.iter().map(|id| state(id)).collect()
}

pub fn parse_at(
    responses: &[McpResponse],
    states: Vec<ThreadReadState>,
    cursor: Option<CursorState>,
    now: &str,
) -> SourceResult {
    let source = source();
    let context = with_reads(context_of(&source, now, cursor.into_iter().collect()), states);
    parse_one(&source, responses, &context)
}

pub fn parse_known(responses: &[McpResponse], known: &[&str], cursor: Option<CursorState>) -> SourceResult {
    parse_at(responses, reads(known), cursor, T3)
}

pub fn parse(responses: &[McpResponse]) -> SourceResult {
    parse_known(responses, &[], None)
}

pub fn blocks() -> Vec<MailBlock> {
    parse_mail(DIGEST_SUBJECT, &digest())
}

pub fn cursor_at(days: i64) -> CursorState {
    let watermark = at(T3).minus_ms(days * DAY_MS).to_iso();
    cursor(SourceName::Notion, "mail", &watermark, 0, BEFORE)
}

pub fn head(title: &str) -> String {
    format!("[| | {title} |]({LINK})")
}

pub fn row(cell: &str) -> String {
    format!("| | {cell}[]({LINK}) |")
}

pub fn preview() -> String {
    format!("[| | || | 합성 미리보기 저장금지 || || 값 |]({LINK})")
}

pub fn object_of(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}
