//! 회수된 응답의 쪽 상한. 파서와 원장을 함께 쓴다
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{McpSource, Outcome, SourceName, SourceResult};
use kairos_source_mcp::calendar::deploy_calendar;
use kairos_source_mcp::{CalendarSource, GmailSource, SlackSource};
use serde_json::{Value, json};

fn next_cursor(cursor: &str) -> String {
    format!("For the next page of results use cursor `{cursor}`\n")
}

fn slack_body(results: &str, pagination: &str) -> Value {
    json!({ "results": results, "pagination_info": pagination })
}

#[test]
fn 회수된_gmail_응답에_다음_쪽이_있으면_partial이고_전진하지_않는다() {
    let gmail = GmailSource::new(clock());
    let page = json_response("gmail:recent", &json!({ "threads": [], "nextPageToken": "more" }));
    let parsed = gmail.parse(&[page], &context_of(&gmail, T3)).unwrap().remove(0);
    assert_eq!((parsed.outcome, parsed.error_code.as_deref()), (Outcome::Partial, Some("page_cap")));
    assert_eq!(parsed.watermark, None);
}

#[test]
fn 회수된_slack_응답에_다음_쪽이_있으면_읽은_데까지만_전진하고_page_cap을_남긴다() {
    let slack = SlackSource::new(clock());
    let body = slack_body(&fixture("slack/mentions-page.md"), &next_cursor("more"));
    let page = json_response("slack:mentions", &body);
    let parsed = slack.parse(&[page], &context_of(&slack, T3)).unwrap().remove(0);
    assert_eq!(parsed.error_code.as_deref(), Some("page_cap"));
    assert_ne!(parsed.watermark.as_deref(), Some(T3));
    assert_eq!(parsed.window_to, parsed.watermark);
}

#[test]
fn 회수된_calendar_응답에_다음_쪽이_있으면_partial이고_못_읽은_일정이_취소로_오판되지_않는다() {
    let test = TestHome::new();
    let db = test.open_db();
    let calendar = CalendarSource::new(clock());
    let body = json!({ "accessRole": "reader", "summary": deploy_calendar(), "nextPageToken": "more" });
    let deploy = json_response("calendar:deploy", &body);
    let parsed = calendar.parse(&[deploy], &context_of(&calendar, T3)).unwrap().remove(0);
    assert_eq!((parsed.outcome, parsed.error_code.as_deref()), (Outcome::Partial, Some("page_cap")));
    assert_eq!(parsed.watermark, None);

    let seen = SourceResult {
        source: SourceName::Calendar,
        query: "personal".into(),
        snapshot: true,
        objects: vec![meeting("kept")],
        ..result()
    };
    ingest(&db, seen, T0);
    let body = json!({ "accessRole": "owner", "summary": "kairos-dev@contoso.example", "nextPageToken": "more" });
    let personal = json_response("calendar:personal", &body);
    let personal = calendar.parse(&[personal], &context_of(&calendar, T3)).unwrap().remove(0);
    for when in [T1, T2, T3] {
        ingest(&db, SourceResult { started_at: when.into(), finished_at: when.into(), ..personal.clone() }, when);
    }
    let missing: i64 = db
        .connection()
        .query_row("SELECT missing_runs FROM objects WHERE object_key = 'gcal:kept'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(missing, 0);
    let withdrawn: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM events WHERE type = 'object_withdrawn'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(withdrawn, 0);
}
