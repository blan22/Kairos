//! 쪽, 실패 판정, 전진.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{CollectContext, CursorState, McpResponse, McpSource, Outcome, SourceName, SourceResult};
use kairos_source_mcp::SlackSource;

fn source() -> SlackSource {
    SlackSource::new(clock())
}

fn parse(responses: &[McpResponse]) -> SourceResult {
    let source = source();
    parse_one(&source, responses, &context_of(&source, T3, vec![]))
}

fn from_me_page(pagination: &str) -> McpResponse {
    json_response("slack:from_me", &slack_body(&fixture("slack/from-me-page.md"), pagination))
}

/// T0의 Unix 초.
const T0_SECONDS: i64 = 1_772_413_200;

fn stamp(seconds: i64, micros: i64) -> String {
    format!("{seconds}.{micros:06}")
}

/// 합성 멘션 한 쪽. ts는 오름차순으로 넘긴다.
fn mention_page(id: &str, stamps: &[String], pagination: &str) -> McpResponse {
    let total = stamps.len();
    let records: String = stamps
        .iter()
        .enumerate()
        .map(|(index, ts)| {
            let (n, link) = (index + 1, ts.replacen('.', "", 1));
            format!(
                "### Result {n} of {total}\nChannel: #합성-채널 (ID: C0SYNTH01)\n\
                 From: 합성 동료 <teammate@contoso.example> (ID: U0SYNTH01) \nMessage_ts: {ts}\n\
                 Permalink: [link](https://synthetic.slack.com/archives/C0SYNTH01/p{link})\n\
                 Text: \n<@{ME}|합성 사용자> 합성 멘션 {n}\n\n---\n\n"
            )
        })
        .collect();
    let text = format!("# Search Results for: <@{ME}>\n\n## Messages ({total} results)\n{records}");
    json_response(id, &slack_body(&text, pagination))
}

/// 쪽 상한(10쪽 x 20건)을 채우는 몰림. W0 뒤 1초부터 1초 간격이라 모두 겹침 10분 안이다.
fn burst_pages() -> Vec<McpResponse> {
    (0..10)
        .map(|page| {
            let id = if page == 0 { "slack:mentions".to_owned() } else { format!("slack:mentions#{}", page + 1) };
            let stamps: Vec<String> = (0..20).map(|n| stamp(T0_SECONDS + 1 + page * 20 + n, 100)).collect();
            mention_page(&id, &stamps, &next_cursor(&format!("p{}", page + 2)))
        })
        .collect()
}

fn mentions_after(source: &SlackSource, context: &CollectContext) -> String {
    let calls = source.plan(context);
    let call = calls.iter().find(|call| call.query == "mentions").unwrap();
    call.args["after"].as_str().unwrap().to_owned()
}

fn resuming(watermark: &str, progress_at: Option<String>) -> CursorState {
    CursorState { progress_at, ..cursor(SourceName::Slack, "mentions", watermark, 600, BEFORE) }
}

#[test]
fn 쪽마다_result_1부터_시작해도_쪽_합으로_검증한다() {
    let page = fixture("slack/from-me-page.md");
    let later =
        page.replace("1772413500.000600", "1772413600.000700").replace("p1772413500000600", "p1772413600000700");
    let result = parse(&[
        json_response("slack:from_me", &slack_body(&page, &next_cursor("A"))),
        json_response("slack:from_me#2", &slack_body(&later, END_OF_RESULTS)),
    ]);
    assert_eq!(
        (result.outcome, result.pages, result.parsed_count, result.expected_count),
        (Outcome::Ok, 2, 2, Some(2))
    );
    assert_eq!(result.watermark.as_deref(), Some(T3));
}

#[test]
fn 쪽_상한에_걸리면_읽은_데까지만_watermark가_간다() {
    let result = parse(&[from_me_page(&next_cursor("A"))]);
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.error_code.as_deref(), Some("page_cap"));
    assert_eq!(result.watermark.as_deref(), Some("2026-03-02T01:05:00.001Z"));
    assert_eq!(result.window_to, result.watermark);
}

#[test]
fn 뒤쪽_페이지가_실패하면_partial이고_watermark가_없다() {
    let result = parse(&[from_me_page(&next_cursor("A")), error_response("slack:from_me#2", "내부 오류")]);
    assert_eq!(result.outcome, Outcome::Partial);
    assert_eq!(result.watermark, None);
    assert_eq!(result.events.len(), 1);
}

#[test]
fn 뒤쪽_쪽이_틱_마감으로_실패하면_읽은_데까지_watermark가_가고_partial이다() {
    let result = parse(&[from_me_page(&next_cursor("A")), error_response("slack:from_me#2", "kairos:timeout")]);
    assert_eq!(result.outcome, Outcome::Partial);
    assert_eq!(result.error_code.as_deref(), Some("mode_deadline"));
    assert_eq!(result.watermark.as_deref(), Some("2026-03-02T01:05:00.001Z"));
    assert_eq!(result.window_to, result.watermark);
    assert_eq!(result.progress_at.as_deref(), Some("2026-03-02T01:04:59.001Z"), "partial이라 이어 읽을 자리를 남긴다");
    assert_eq!(result.events.len(), 1);
}

#[test]
fn 첫_쪽부터_틱_마감이면_읽은_것이_없어_watermark가_없다() {
    let result = parse(&[error_response("slack:from_me", "kairos:timeout")]);
    assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Timeout, Some("mode_deadline")));
    assert_eq!(result.watermark, None);
}

#[test]
fn 뒤쪽_페이지의_해석_실패는_앞쪽까지_저장하지_않는다() {
    let result = parse(&[
        from_me_page(&next_cursor("A")),
        json_response("slack:from_me#2", &slack_body("broken", END_OF_RESULTS)),
    ]);
    assert_eq!(result.outcome, Outcome::ParseError);
    assert!(result.events.is_empty());
}

#[test]
fn 오름차순이_아니면_쪽_상한_계산을_믿을_수_없어_parse_error다() {
    let swapped = fixture("slack/mentions-page.md")
        .replace("1772413320.000300", "1772413100.000300")
        .replace("p1772413320000300", "p1772413100000300");
    let result = parse(&[json_response("slack:mentions", &slack_body(&swapped, END_OF_RESULTS))]);
    assert_eq!(result.error_code.as_deref(), Some("not_ascending"));
}

#[test]
fn 도구_오류를_outcome으로_구분한다_오류_문구는_코드에_옮기지_않는다() {
    let cases = [
        ("kairos:tool_missing", Outcome::ToolMissing, "mcp_not_connected"),
        ("kairos:refused", Outcome::ContractBreak, "mode_refused"),
        ("kairos:timeout", Outcome::Timeout, "mode_deadline"),
        ("HTTP 401 invalid_auth", Outcome::Auth, "mcp_auth"),
        ("HTTP 429 ratelimited", Outcome::RateLimited, "mcp_rate_limited"),
        ("외부 문자열이 섞인 오류", Outcome::ContractBreak, "mcp_tool_error"),
    ];
    for (text, outcome, error_code) in cases {
        let result = parse(&[error_response("slack:mentions", text)]);
        assert_eq!((result.outcome, result.error_code.as_deref()), (outcome, Some(error_code)));
        assert!(result.events.is_empty());
        assert_eq!(result.watermark, None);
    }
}

#[test]
fn 쪽_상한으로_읽은_데가_겹침_안이면_이어_읽고_이어_읽을_자리도_못_옮기면_멈춤을_드러낸다() {
    let source = source();
    let page = json_response("slack:mentions", &slack_body(&fixture("slack/mentions-page.md"), &next_cursor("more")));
    let first = parse_one(&source, std::slice::from_ref(&page), &context_of(&source, T3, vec![]));
    assert_eq!((first.outcome, first.error_code.as_deref()), (Outcome::Ok, Some("page_cap")));
    let reached = first.window_to.as_deref().unwrap();

    // 겹침 안에서 잘렸다. watermark는 못 옮기지만 읽은 마지막 건 1초 앞에서 이어 읽는다.
    let overlapped =
        parse_one(&source, std::slice::from_ref(&page), &context_of(&source, T3, vec![resuming(reached, None)]));
    assert_eq!((overlapped.outcome, overlapped.error_code.as_deref()), (Outcome::Partial, Some("page_cap")));
    assert_eq!(overlapped.watermark, None);
    assert_eq!(overlapped.progress_at, Some(at(reached).minus_ms(1000).to_iso()));

    // 이어 읽은 자리에서도 같은 데까지만 읽히면 정말 멈춘 것이다. 공백으로 드러난다.
    let stuck_cursor = resuming(reached, overlapped.progress_at.clone());
    let stuck = parse_one(&source, &[page], &context_of(&source, T3, vec![stuck_cursor]));
    assert_eq!(stuck.outcome, Outcome::Partial);
    assert_eq!(stuck.error_code.as_deref(), Some("page_cap_stalled"));
    assert_eq!(stuck.watermark, None);
    assert_eq!(stuck.progress_at, None);
    assert!(!stuck.events.is_empty(), "읽은 것은 저장한다");
}

#[test]
fn 겹침_안의_몰림에_쪽_상한이_걸려도_다음_틱이_읽은_데부터_이어_읽어_뒤의_멘션을_놓치지_않는다() {
    let source = source();
    let w0 = T0;
    let w1 = at(T0).plus_ms(200_000).to_iso();
    let pages = burst_pages();

    // 틱 1: 몰림 200건을 읽고 쪽 상한에 걸린다. 읽은 마지막 건이 겹침 밖이라 watermark가 W1로 간다.
    let tick1 = parse_one(&source, &pages, &context_of(&source, T3, vec![resuming(w0, None)]));
    assert_eq!((tick1.outcome, tick1.error_code.as_deref()), (Outcome::Ok, Some("page_cap")));
    assert_eq!(tick1.watermark.as_deref(), Some(w1.as_str()));
    assert_eq!(tick1.events.len(), 200);

    // 틱 2: W1 - 600초부터 읽으니 같은 200건이 다시 나온다. 예전에는 여기서 영구히 멈췄다.
    let context2 = context_of(&source, T3, vec![resuming(&w1, None)]);
    assert_eq!(mentions_after(&source, &context2), (at(&w1).millis() / 1000 - 600).to_string());
    let tick2 = parse_one(&source, &pages, &context2);
    assert_eq!((tick2.outcome, tick2.error_code.as_deref()), (Outcome::Partial, Some("page_cap")));
    assert_eq!(tick2.watermark, None);
    assert_eq!(tick2.progress_at, Some(at(&w1).minus_ms(1000).to_iso()));

    // 틱 3: 이어 읽을 자리(W1 - 1초)부터 읽는다. 경계의 마지막 건은 다시 읽고(중복 제거), 몰림 뒤의 멘션을 읽는다.
    let context3 = context_of(&source, T3, vec![resuming(&w1, tick2.progress_at.clone())]);
    assert_eq!(mentions_after(&source, &context3), (at(&w1).millis() / 1000 - 1).to_string());
    let later = stamp(T0_SECONDS + 320, 100);
    let response = mention_page("slack:mentions", &[stamp(T0_SECONDS + 200, 100), later], END_OF_RESULTS);
    let tick3 = parse_one(&source, &[response], &context3);
    assert_eq!((tick3.outcome, tick3.error_code.as_deref()), (Outcome::Ok, None));
    assert_eq!(tick3.window_from, tick2.progress_at);
    assert_eq!(tick3.watermark.as_deref(), Some(T3));
    let later_at = at(T0).plus_ms(320_000).to_iso();
    assert!(tick3.events.iter().any(|event| event.occurred_at == later_at), "몰림 뒤의 멘션");
}

#[test]
fn 같은_초에_쪽_예산보다_많은_건이_몰리면_이어_읽어도_못_가서_page_cap_stalled로_드러난다() {
    let source = source();
    let second = T0_SECONDS + 200;
    let stamps: Vec<String> = (1..=20).map(|n| stamp(second, n * 10)).collect();
    let page = mention_page("slack:mentions", &stamps, &next_cursor("more"));
    let watermark = at(T0).plus_ms(201_000).to_iso();

    let first =
        parse_one(&source, std::slice::from_ref(&page), &context_of(&source, T3, vec![resuming(&watermark, None)]));
    assert_eq!((first.outcome, first.error_code.as_deref()), (Outcome::Partial, Some("page_cap")));
    let progress = first.progress_at.clone().unwrap();

    let context = context_of(&source, T3, vec![resuming(&watermark, Some(progress.clone()))]);
    assert_eq!(mentions_after(&source, &context), (second - 1).to_string(), "같은 초를 다시 받는다");
    let stuck = parse_one(&source, &[page], &context);
    assert_eq!((stuck.outcome, stuck.error_code.as_deref()), (Outcome::Partial, Some("page_cap_stalled")));
    assert_eq!((stuck.watermark, stuck.progress_at), (None, None));
    assert_eq!(stuck.window_from.as_deref(), Some(progress.as_str()));
}
