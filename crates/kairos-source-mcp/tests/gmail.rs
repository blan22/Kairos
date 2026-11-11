#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{
    ActorKind, CollectContext, McpSource, NOTION_SENDER, Outcome, Source, SourceName, SourceResult, TrustTier,
};
use kairos_source_mcp::GmailSource;
use kairos_source_mcp::gmail::{MAX_WINDOW_DAYS, sender_address};
use serde_json::{Value, json};

const FRIDAY_LAST_TICK: &str = "2026-11-06T10:20:00.000Z"; // 19:20 KST
const MONDAY_FIRST_TICK: &str = "2026-11-09T00:00:00.000Z"; // 09:00 KST

fn source() -> GmailSource {
    GmailSource::new(clock())
}

fn not_notion() -> String {
    format!(" -from:{NOTION_SENDER}")
}

fn context() -> CollectContext {
    context_of(&source(), T3, vec![])
}

fn body() -> Value {
    json_fixture("gmail/threads.json")
}

fn parse(value: &Value) -> SourceResult {
    parse_one(&source(), &[json_response("gmail:recent", value)], &context())
}

fn address(text: &str) -> Option<String> {
    sender_address(Some(&json!(text)))
}

fn cursor_at(watermark: &str) -> Vec<kairos_core::model::CursorState> {
    vec![cursor(SourceName::Gmail, "recent", watermark, 0, watermark)]
}

fn query_of(context: &CollectContext) -> String {
    source().plan(context)[0].args["query"].as_str().unwrap().to_owned()
}

#[test]
fn 시간당_1회이고_본문을_요청하지_않는다() {
    let source = source();
    assert_eq!(source.queries()[0].interval_seconds, 3600);
    let calls = source.plan(&context());
    assert_eq!(calls[0].tool, "search_threads");
    assert_eq!(calls[0].args["query"], format!("newer_than:2d{}", not_notion()));
    assert_eq!(calls[0].args["view"], "THREAD_VIEW_SUMMARY");
}

#[test]
fn 검색어가_notion_알림_발신자를_제외한다() {
    assert_eq!(query_of(&context()), "newer_than:2d -from:notify@mail.notion.com");
}

#[test]
fn 빈_객체는_0건이고_ok다() {
    assert_eq!((parse(&json!({})).outcome, parse(&json!({})).parsed_count), (Outcome::Ok, 0));
    assert_eq!(parse(&json!({ "resultCountEstimate": "0" })).outcome, Outcome::Ok);
}

#[test]
fn 영건과_형식_깨짐을_구분한다() {
    assert_eq!(parse(&json!({ "error": "x" })).error_code.as_deref(), Some("gmail_shape"));
    assert_eq!(parse(&json!({ "threads": "x" })).error_code.as_deref(), Some("gmail_shape"));
    assert_eq!(parse(&json!({ "threads": [{ "messages": [] }] })).error_code.as_deref(), Some("gmail_thread_id"));
}

/// 규칙 판정은 deferredTests에 있다. 규칙의 입력인 발신자와 제목까지 본다.
#[test]
fn 발신자와_제목으로_등급이_나뉜다() {
    let result = parse(&body());
    let inputs: Vec<_> =
        result.events.iter().map(|event| (event.actor_id.as_deref(), event.text_head.as_deref())).collect();
    assert_eq!(
        inputs,
        [
            (Some("no_reply@email.apple.com"), Some("Your submission: App Review completed")),
            (Some("alerts@sentry.io"), Some("New issue in widget")),
            (Some("teammate@contoso.example"), Some("사람이 보낸 메일")),
            (Some(SELF_EMAIL), Some("내가 보낸 메일")),
            (Some("no-reply@example.invalid"), Some("주문 안내")),
        ]
    );
}

/// 재수집의 멱등은 deferredTests에 있다.
#[test]
fn 스레드_id로_중복을_없애고_재수집은_멱등이다() {
    let mut repeated = body();
    let first = repeated["threads"][0].clone();
    repeated["threads"].as_array_mut().unwrap().push(first);
    let result = parse(&repeated);
    assert_eq!(result.parsed_count, 5);
    assert_eq!(result.events[0].dedupe_key, "gmail:synthetic0001", "자리는 처음 본 스레드의 것이다");
}

/// 저장 뒤의 DB 덤프 대신 소스가 내는 값 전체를 본다.
#[test]
fn 메일_미리보기는_저장하지_않는다() {
    assert!(!dump(&parse(&body())).contains("저장되면 안 되는"));
}

#[test]
fn date가_없으면_첫_관측_시각_있으면_그_시각이다() {
    let result = parse(&body());
    assert_eq!(result.events[0].occurred_at, T3);
    assert_eq!(result.events[3].occurred_at, "2026-03-02T00:30:00.000Z");
}

#[test]
fn 발신자_주소만_뽑는다() {
    assert_eq!(address("이름 <Mixed@Example.Invalid>").as_deref(), Some("mixed@example.invalid"));
    assert_eq!(address("plain@example.invalid").as_deref(), Some("plain@example.invalid"));
    assert_eq!(address("주소 없음"), None);
    assert_eq!(sender_address(None), None);
}

#[test]
fn 본인_주소_신원과_출처_등급() {
    let result = parse(&body());
    assert_eq!(result.events[3].actor_id.as_deref(), Some(SELF_EMAIL));
    let tiers: Vec<_> = result.events.iter().map(|event| event.trust_tier).collect();
    let expected = [TrustTier::T2, TrustTier::T3, TrustTier::T1, TrustTier::T0, TrustTier::T2];
    assert_eq!(tiers, expected.map(Some));
}

#[test]
fn 도구_실패는_0건이_아니다() {
    let result = parse_one(&source(), &[error_response("gmail:recent", "kairos:tool_missing")], &context());
    assert_eq!((result.outcome, result.watermark), (Outcome::ToolMissing, None));
}

/// plan-tick과 저장을 거치는 경로는 deferredTests에 있다. 같은 커서로 소스의 창을 본다.
#[test]
fn 조회_창이_금요일_마지막_틱부터_월요일_첫_틱까지를_덮는다() {
    let source = source();
    let monday = context_of(&source, MONDAY_FIRST_TICK, cursor_at(FRIDAY_LAST_TICK));
    assert_eq!(source.plan(&monday)[0].args["query"], format!("newer_than:4d{}", not_notion()));
    let window_ms = 4 * 24 * 3_600_000;
    let start = at(MONDAY_FIRST_TICK).minus_ms(window_ms);
    assert!(start < at(FRIDAY_LAST_TICK), "창이 마지막 ok 앞까지 간다");

    let thread = json!({ "id": "synthetic-friday", "messages": [{ "sender": "alerts@sentry.io", "subject": "금요일 밤 경보" }] });
    let read = parse_one(&source, &[json_response("gmail:recent", &json!({ "threads": [thread] }))], &monday);
    assert_eq!(read.window_from, Some(start.to_iso()));
}

#[test]
fn 짧은_공백과_첫_실행의_창은_2일이다() {
    let source = source();
    let expected = format!("newer_than:2d{}", not_notion());
    assert_eq!(query_of(&context_of(&source, T3, vec![])), expected);
    assert_eq!(query_of(&context_of(&source, T3, cursor_at(T2))), expected);
}

/// source_run 기록은 deferredTests에 있다.
#[test]
fn 상한을_넘은_공백은_window_capped로_남기고_전진한다() {
    let source = source();
    let long_ago = at(T3).minus_ms(45 * 24 * 3_600_000).to_iso();
    let context = context_of(&source, T3, cursor_at(&long_ago));
    assert_eq!(query_of(&context), format!("newer_than:{MAX_WINDOW_DAYS}d{}", not_notion()));
    let capped = parse_one(&source, &[json_response("gmail:recent", &json!({}))], &context);
    assert_eq!(capped.outcome, Outcome::Ok);
    assert_eq!(capped.error_code.as_deref(), Some("window_capped"));
    assert_eq!(capped.watermark.as_deref(), Some(T3));
}

#[test]
fn 쪽_상한에_걸려_창을_다_못_읽으면_partial이고_전진하지_않는다() {
    let mut next = body();
    next["nextPageToken"] = json!("synthetic-next");
    let result = parse(&next);
    assert_eq!(result.outcome, Outcome::Partial);
    assert_eq!(result.error_code.as_deref(), Some("page_cap"));
    assert_eq!(result.watermark, None);
    assert_eq!(result.events.len(), 5);
}

fn sent(sender: &str) -> kairos_core::model::NormalizedEvent {
    let thread = json!({ "id": "x", "messages": [{ "sender": sender, "subject": "합성" }] });
    parse(&json!({ "threads": [thread] })).events.remove(0)
}

#[test]
fn 표시_이름의_꺾쇠로_신원을_위조하지_못한다() {
    let me = SELF_EMAIL;
    assert_eq!(address(&format!("\"<{me}>\" <evil@attacker.test>")).as_deref(), Some("evil@attacker.test"));
    assert_eq!(address("\"<alerts@sentry.io>\" <evil@attacker.test>").as_deref(), Some("evil@attacker.test"));
    assert_eq!(address(&format!("<evil@attacker.test> \"<{me}>\"")), None, "끝이 주소가 아니면 주소가 없다");
    assert_eq!(address(&format!("x <a@b.test> {me}")), None, "주소처럼 보이는 꼬리를 믿지 않는다");

    let spoofed = sent(&format!("\"<{me}>\" <evil@attacker.test>"));
    assert_eq!(spoofed.actor_kind, ActorKind::Human);
    assert_eq!(spoofed.trust_tier, Some(TrustTier::T3));
    assert_eq!(spoofed.actor_id.as_deref(), Some("evil@attacker.test"));
    assert_eq!(sent("\"<boss@contoso.example>\" <evil@attacker.test>").trust_tier, Some(TrustTier::T3));
    let doubled = sent(&format!("\"<x@y.test>\" <{me}>"));
    assert_eq!(
        (doubled.actor_kind, doubled.trust_tier),
        (ActorKind::Human, Some(TrustTier::T3)),
        "꺾쇠가 둘이면 믿지 않는다"
    );
    assert_eq!(sent("\"<x@y.test>\" <boss@contoso.example>").trust_tier, Some(TrustTier::T3));
}

/// 규칙 판정은 deferredTests에 있다. 규칙이 보는 주소까지 본다.
#[test]
fn 표시_이름으로_sentry_메일_규칙을_얻지_못한다() {
    let event = sent("\"<alerts@sentry.io>\" <evil@attacker.test>");
    assert_eq!(event.actor_id.as_deref(), Some("evil@attacker.test"));
}

#[test]
fn 그룹_캘린더_주소는_메일_신뢰_목록이_아니다() {
    assert_eq!(sent("anything@group.calendar.google.com").trust_tier, Some(TrustTier::T3));
}

// 아래는 원본에 없는 시험이다.

#[test]
fn null_스레드는_나머지를_쓰되_전진하지_않아_다음_틱에_다시_읽는다() {
    let thread = json!({ "id": "t1", "messages": [{ "sender": "a@b.test", "subject": "합성" }] });
    let result = parse(&json!({ "threads": [null, thread] }));
    assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Partial, Some("record_null")));
    assert_eq!((result.objects.len(), result.watermark.as_deref()), (1, None));
    let nameless = parse(&json!({ "threads": [{ "id": "" }, null] }));
    assert_eq!((nameless.outcome, nameless.error_code.as_deref()), (Outcome::ParseError, Some("gmail_thread_id")));
}

#[test]
fn 되읽을_수_없는_date는_지금_시각으로_둔다() {
    let message = json!({ "sender": "a@b.test", "date": "Fri, 31 Dec 9999 23:59:59 -0100" });
    let result = parse(&json!({ "threads": [{ "id": "t1", "messages": [message] }] }));
    assert_eq!(result.objects[0].ts.as_deref(), Some(T3));
}

#[test]
fn 같은_id가_다시_오면_뒤의_값이_앞의_자리에_들어간다() {
    let thread =
        |subject: &str, id: &str| json!({ "id": id, "messages": [{ "sender": "a@b.test", "subject": subject }] });
    let result = parse(&json!({ "threads": [thread("앞", "x"), thread("중간", "y"), thread("뒤", "x")] }));
    let heads: Vec<_> = result.events.iter().map(|event| event.text_head.as_deref()).collect();
    assert_eq!(heads, [Some("뒤"), Some("중간")]);
}
