//! 중 planTick, ingestResponses로 DB를 지나는 시험.
//! 판정은 kairos-source-mcp가 상태를 넘겨 시험했고, 여기서는 thread_read와 커서가 틱 사이를 잇는지 본다.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use kairos_agenda::StateBuilder;
use kairos_core::model::{
    CollectContext, CursorState, McpExpand, McpResponse, McpSource, Outcome, Source, SourceName, SourceResult,
    ThreadReadState,
};
use kairos_core::ports::{CancelToken, Clock, FixedClock};
use kairos_ledger::{CursorRepo, Ledger};
use kairos_source_mcp::{GmailSource, NotionSource};
use kairos_store::{Database, params};
use kairos_tick::{IngestPayload, IngestRow, ResponseIngestor, TickPlan, TickPlanner};
use serde_json::{Value, json};

const SENDER: &str = "notify@mail.notion.com";
const SEARCH: &str = "notion:mail";
const DIGEST_SUBJECT: &str = "합성 사용자 외 2명이 Contoso에서 업데이트함";
const DATE: &str = "2026-03-02T00:30:00.000Z";
const EDIT: &str = "이합성 이 페이지 페이지를 편집했습니다.";
const LINK: &str = "https://mg.mail.notion.com/c/synthetic";

fn fixed(now: &str) -> Arc<dyn Clock> {
    Arc::new(FixedClock(at(now)))
}

fn notion(now: &str) -> NotionSource {
    NotionSource::new(fixed(now))
}

fn context_of(source: &dyn Source, now: &str, reads: Vec<ThreadReadState>) -> CollectContext {
    let cursors: BTreeMap<String, Option<CursorState>> =
        source.queries().iter().map(|query| (query.id.clone(), None)).collect();
    let thread_reads = Some(BTreeMap::from([("mail".to_owned(), reads)]));
    CollectContext { now: at(now), cursors, thread_reads, cancel: CancelToken::new() }
}

fn search_thread(id: &str, count: i64) -> Value {
    json!({
        "id": id,
        "messageCount": count,
        "messages": [{ "id": format!("m-{id}"), "sender": SENDER, "threadId": id, "toRecipients": ["synthetic@example.com"] }],
    })
}

fn search(ids: &[&str]) -> McpResponse {
    let threads: Vec<Value> = ids.iter().map(|id| search_thread(id, 1)).collect();
    json_response(SEARCH, &json!({ "resultCountEstimate": ids.len(), "threads": threads }))
}

fn message_at(body: &str, date: &str) -> Value {
    json!({
        "id": "m",
        "sender": SENDER,
        "subject": DIGEST_SUBJECT,
        "date": date,
        "internalDate": at(date).millis().to_string(),
        "plaintextBody": body,
        "snippet": "합성 미리보기 저장금지",
        "viewUrl": "https://mail.google.com/",
    })
}

fn digest() -> String {
    fixture("notion/digest.txt")
}

fn thread_with(id: &str, messages: Vec<Value>) -> McpResponse {
    let count = messages.len();
    let body = json!({ "id": id, "messageCount": count, "messages": messages, "viewUrl": "https://mail.google.com/" });
    json_response(&format!("{SEARCH}!{id}"), &body)
}

fn thread_at(id: &str, date: &str) -> McpResponse {
    thread_with(id, vec![message_at(&digest(), date)])
}

fn thread(id: &str) -> McpResponse {
    thread_at(id, DATE)
}

fn state(id: &str) -> ThreadReadState {
    ThreadReadState {
        external_id: id.to_owned(),
        message_count: Some(1),
        failures: 0,
        first_failed_at: None,
        quarantined_at: None,
    }
}

fn parse_at(responses: &[McpResponse], now: &str, known: &[&str]) -> SourceResult {
    let source = notion(now);
    let reads = known.iter().map(|id| state(id)).collect();
    let mut results = source.parse(responses, &context_of(&source, now, reads)).unwrap();
    assert_eq!(results.len(), 1);
    results.remove(0)
}

fn parse(responses: &[McpResponse]) -> SourceResult {
    parse_at(responses, T3, &[])
}

/// `ingestResponses`의 첫 행.
fn feed(db: &Database, now: &str, responses: Vec<McpResponse>) -> IngestRow {
    let payload = IngestPayload { now: at(now), source: SourceName::Notion, responses };
    let summary = ResponseIngestor::new(db, fixed(now)).ingest_responses(&notion(now), &payload, &none()).unwrap();
    summary.results.into_iter().next().unwrap()
}

fn forced_plan(db: &Database, now: &str) -> TickPlan {
    TickPlanner::new(db.connection()).plan_tick(&[&notion(now)], at(now), &none(), true).unwrap()
}

fn expand_at(db: &Database, now: &str) -> McpExpand {
    forced_plan(db, now).calls[0].expand.clone().unwrap()
}

fn skip_of(expand: &McpExpand) -> Vec<(String, i64)> {
    expand.skip.iter().map(|(key, count)| (key.to_owned(), count)).collect()
}

fn outcome_of(row: &IngestRow) -> (Outcome, Option<&str>) {
    (row.outcome, row.error_code.as_deref())
}

fn event_row(db: &Database, key: &str) -> (String, Option<String>, i64) {
    db.connection()
        .query_row("SELECT type, rule_id, baseline FROM events WHERE dedupe_key = ?", params![key], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
}

fn all_events(db: &Database) -> Vec<(Option<String>, i64)> {
    let mut statement =
        db.connection().prepare("SELECT rule_id, baseline FROM events ORDER BY occurred_at, id").unwrap();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

fn would_show_rules(db: &Database) -> Vec<String> {
    StateBuilder::new(db.connection()).would_show_events().unwrap().into_iter().map(|row| row.event.rule_id).collect()
}

fn cursor(db: &Database, source: SourceName, query: &str) -> Option<CursorState> {
    CursorRepo::new(db.connection()).get(source, query).unwrap()
}

#[test]
fn 첫_수집은_baseline이고_이후의_멘션은_표시_후보다() {
    let db = test_db();
    ingest(&db, parse(&[search(&["t1"]), thread("t1")]), T3);
    assert!(all_events(&db).iter().all(|row| row.1 == 1));
    assert!(would_show_rules(&db).is_empty());

    let later = "2026-03-02T02:00:00.000Z";
    ingest(&db, parse_at(&[search(&["t1", "t2"]), thread_at("t2", later)], later, &["t1"]), later);
    assert_eq!(
        would_show_rules(&db),
        [
            "p1.notion_mention",
            "p1.notion_mention",
            "p1.notion_mention",
            "p2.notion_comment",
            "p2.notion_comment",
            "p2.notion_unknown",
        ]
    );
}

#[test]
fn 규칙_멘션과_이름은_p1_댓글은_p2_편집은_noise_본인은_noise_self_형식_밖은_p2로_연다() {
    let db = test_db();
    ingest(&db, parse(&[search(&["t1"]), thread("t1")]), T3);
    let mut statement = db.connection().prepare("SELECT rule_id, disposition FROM events ORDER BY dedupe_key").unwrap();
    let rows: Vec<(String, String)> =
        statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect();
    let expected = [
        ("p2.notion_comment", "opened"),
        ("p1.notion_mention", "opened"),
        ("p1.notion_mention", "opened"),
        ("p2.notion_comment", "opened"),
        ("noise.self", "noise"),
        ("noise.notion_edit", "noise"),
        ("p1.notion_mention", "opened"),
        ("p2.notion_unknown", "opened"),
    ];
    assert_eq!(rows.iter().map(|(rule, way)| (rule.as_str(), way.as_str())).collect::<Vec<_>>(), expected);
}

#[test]
fn 첫_시도가_partial이어도_그_뒤에_온_멘션은_표시_후보다() {
    let db = test_db();
    feed(&db, T3, vec![search(&["t1", "stuck"]), thread("t1")]);
    let later = "2026-03-02T05:00:00.000Z";
    feed(&db, later, vec![search(&["t1", "stuck", "new1"]), thread_at("new1", later)]);
    feed(&db, later, vec![search(&["t1", "stuck", "new1"]), thread("stuck")]);
    assert_eq!(cursor(&db, SourceName::Notion, "mail").unwrap().t0.as_deref(), Some(T3));
    assert_eq!(event_row(&db, "notion-mail:new1:2").2, 0);
    assert_eq!(event_row(&db, "notion-mail:t1:2").2, 1);
    let shown = StateBuilder::new(db.connection()).would_show_events().unwrap();
    assert!(shown.iter().any(|row| row.event.dedupe_key == "notion-mail:new1:2"));
}

#[test]
fn 이미_저장한_스레드는_plan_tick이_db에서_찾아_건너뛴다_소스는_db를_열지_않는다() {
    let db = test_db();
    ingest(&db, parse(&[search(&["t1"]), thread("t1")]), T3);
    assert_eq!(skip_of(&expand_at(&db, "2026-03-04T03:00:00.000Z")), [("t1".to_owned(), 1)]);
    let source = notion(T3);
    let context = TickPlanner::new(db.connection()).context_for(&source, &source, at(T3)).unwrap();
    assert_eq!(context.thread_reads.unwrap()["mail"], [state("t1")]);
}

#[test]
fn 일부_읽기가_실패하면_읽은_것은_저장하고_partial이며_watermark가_멈춘다_다음_틱에_그_스레드만_읽는다() {
    let db = test_db();
    let seed = parse(&[search(&[])]);
    ingest(&db, SourceResult { started_at: BEFORE.into(), finished_at: BEFORE.into(), ..seed }, BEFORE);
    let before = cursor(&db, SourceName::Notion, "mail").unwrap().watermark;
    let failed = error_response(&format!("{SEARCH}!t2"), "synthetic failure");
    let first = feed(&db, T3, vec![search(&["t1", "t2"]), thread("t1"), failed]);
    assert_eq!(outcome_of(&first), (Outcome::Partial, Some("mcp_tool_error")));
    assert_eq!(cursor(&db, SourceName::Notion, "mail").unwrap().watermark, before);
    assert_eq!(count(&db, "objects"), 1);

    let expand = expand_at(&db, T3);
    assert_eq!(skip_of(&expand), [("t1".to_owned(), 1)]);
    assert_eq!(expand.retry_ids, ["t2"]);
    let second = feed(&db, T3, vec![search(&["t1", "t2"]), thread("t2")]);
    assert_eq!(outcome_of(&second), (Outcome::Ok, None));
    assert_eq!(count(&db, "objects"), 2);
}

#[test]
fn 두_통짜리_스레드는_저장한_메시지_수까지_다시_읽지_않아_ok가_막히지_않는다() {
    let db = test_db();
    let ids: Vec<String> = (0..13).map(|index| format!("t{index}")).collect();
    let two =
        |id: &String| json!({ "id": id, "messageCount": 2, "messages": [{ "sender": SENDER }, { "sender": SENDER }] });
    let listing = json_response(SEARCH, &json!({ "threads": ids.iter().map(two).collect::<Vec<_>>() }));
    let body = vec![message_at(&digest(), DATE), message_at(&digest(), DATE)];
    let tick = |read: &[String]| {
        let mut responses = vec![listing.clone()];
        responses.extend(read.iter().map(|id| thread_with(id, body.clone())));
        feed(&db, T3, responses).outcome
    };
    assert_eq!(tick(&ids[..12]), Outcome::Partial);
    assert_eq!(tick(&ids[12..]), Outcome::Ok);
    assert_eq!(expand_at(&db, T3).skip.get("t0"), Some(&2));
    assert_eq!(tick(&[]), Outcome::Ok);
}

#[test]
fn 본문이_비면_읽은_것으로_치지_않고_다음_틱에_다시_읽어_저장한다() {
    let db = test_db();
    for body in ["", "   \n "] {
        let parsed = parse(&[search(&["t1"]), thread_with("t1", vec![message_at(body, DATE)])]);
        assert_eq!(
            (parsed.outcome, parsed.error_code.as_deref(), parsed.objects.len()),
            (Outcome::Partial, Some("notion_body"), 0)
        );
    }
    let mut bodiless = message_at("", DATE);
    bodiless.as_object_mut().unwrap().remove("plaintextBody");
    let parsed = parse(&[search(&["t1"]), thread_with("t1", vec![bodiless])]);
    assert_eq!((parsed.outcome, parsed.error_code.as_deref()), (Outcome::Partial, Some("notion_body")));

    let mixed = thread_with("t1", vec![message_at("", DATE), message_at(&digest(), DATE)]);
    feed(&db, T3, vec![search(&["t1"]), mixed]);
    assert_eq!(count(&db, "objects"), 0);
    let expand = expand_at(&db, T3);
    assert!(expand.skip.is_empty());
    assert_eq!(expand.retry_ids, ["t1"]);
    feed(&db, T3, vec![search(&["t1"]), thread("t1")]);
    assert_eq!(count(&db, "objects"), 1);
}

#[test]
fn 같은_스레드가_한_시간_넘게_실패하면_격리하고_고지한_뒤_전진한다_메시지가_붙으면_다시_읽는다() {
    let db = test_db();
    let after = |minutes: i64| plus_minutes(T3, minutes);
    let fail = || error_response(&format!("{SEARCH}!bad"), "synthetic failure");
    for minutes in [0, 3, 6, 9] {
        assert_eq!(feed(&db, &after(minutes), vec![search(&["bad"]), fail()]).outcome, Outcome::Partial);
    }
    let early = feed(&db, &after(12), vec![search(&["bad"]), fail()]);
    assert_eq!(early.outcome, Outcome::Partial, "횟수만 차고 한 시간이 지나지 않았다");
    let quarantined = feed(&db, &after(70), vec![search(&["bad"]), fail()]);
    assert_eq!(outcome_of(&quarantined), (Outcome::Ok, Some("notion_quarantined")));
    assert_eq!(skip_of(&expand_at(&db, &after(73))), [("bad".to_owned(), 1)]);
    assert_eq!(feed(&db, &after(73), vec![search(&["bad"])]).outcome, Outcome::Ok);
    let quiet = feed(&db, &after(74), vec![search(&["bad"])]);
    assert_eq!(quiet.error_code.as_deref(), Some("notion_quarantined"));
    let grown = json_response(SEARCH, &json!({ "threads": [search_thread("bad", 2)] }));
    assert_eq!(feed(&db, &after(76), vec![grown]).outcome, Outcome::Partial);
}

fn head(title: &str) -> String {
    format!("[| | {title} |]({LINK})")
}

fn row(cell: &str) -> String {
    format!("| | {cell}[]({LINK}) |")
}

#[test]
fn 파서를_고친_뒤_다시_읽으면_unknown_블록만_새로_판정되고_다른_블록은_그대로다() {
    let db = test_db();
    ingest(&db, parse(&[search(&["t0"])]), BEFORE);
    let later = "2026-03-02T02:00:00.000Z";
    let lines = |action: &str| [head("합성 제목 1"), row(EDIT), head("합성 제목 2"), row(action)].join("\n");
    let mail = message_at(&lines("김합성 이 페이지를 공유했습니다."), later);
    feed(&db, later, vec![search(&["t1"]), thread_with("t1", vec![mail])]);
    assert_eq!(event_row(&db, "notion-mail:t1:1").0, "notion_unknown");
    assert_eq!(Ledger::new(&db).reread_notion(at(later)).unwrap(), 1);
    assert!(expand_at(&db, later).skip.is_empty());

    let fixed_mail = message_at(&lines("김합성 이 페이지페이지에서 나를 멘션했습니다."), later);
    feed(&db, later, vec![search(&["t1"]), thread_with("t1", vec![fixed_mail])]);
    assert_eq!(event_row(&db, "notion-mail:t1:0").0, "notion_edit");
    let row = event_row(&db, "notion-mail:t1:1");
    assert_eq!((row.0.as_str(), row.1.as_deref()), ("notion_mention", Some("p1.notion_mention")));
    assert_eq!(count(&db, "events"), 2);
}

#[test]
fn 첫_ok가_늦어지면_창이_첫_시도까지_넓어져_읽지_못한_메일이_창에서_빠지지_않는다() {
    let db = test_db();
    feed(&db, T3, vec![search(&["t1"])]);
    assert_eq!(cursor(&db, SourceName::Notion, "mail").unwrap().first_run_at.as_deref(), Some(T3));
    let later = at(T3).plus_ms(3 * 86_400_000).to_iso();
    let plan = forced_plan(&db, &later);
    let queries: Vec<&str> = plan.calls.iter().map(|call| call.args["query"].as_str().unwrap()).collect();
    assert_eq!(queries, [format!("from:{SENDER} newer_than:2d"), format!("from:{SENDER} newer_than:4d older_than:2d")]);
}

#[test]
fn 조회_창이_금요일_마지막_틱부터_월요일_첫_틱까지를_덮는다() {
    const FRIDAY_LAST_TICK: &str = "2026-11-06T10:20:00.000Z"; // 19:20 KST
    const MONDAY_FIRST_TICK: &str = "2026-11-09T00:00:00.000Z"; // 09:00 KST
    let db = test_db();
    let gmail = |now: &str| GmailSource::new(fixed(now));
    let context = |now: &str, cursor: Option<CursorState>| CollectContext {
        now: at(now),
        cursors: BTreeMap::from([("recent".to_owned(), cursor)]),
        thread_reads: None,
        cancel: CancelToken::new(),
    };
    let empty = [json_response("gmail:recent", &json!({}))];
    let first = gmail(FRIDAY_LAST_TICK).parse(&empty, &context(FRIDAY_LAST_TICK, None)).unwrap();
    ingest(&db, first.into_iter().next().unwrap(), FRIDAY_LAST_TICK);

    let source = gmail(MONDAY_FIRST_TICK);
    let plan = TickPlanner::new(db.connection()).plan_tick(&[&source], at(MONDAY_FIRST_TICK), &none(), false).unwrap();
    assert_eq!(plan.calls[0].args["query"], "newer_than:4d -from:notify@mail.notion.com");
    let window_ms = 4 * 24 * 3_600_000;
    assert!(at(MONDAY_FIRST_TICK).minus_ms(window_ms) < at(FRIDAY_LAST_TICK), "창이 마지막 ok 앞까지 간다");

    let saved = cursor(&db, SourceName::Gmail, "recent");
    let thread = json!({ "id": "synthetic-friday", "messages": [{ "sender": "alerts@sentry.io", "subject": "금요일 밤 경보" }] });
    let responses = [json_response("gmail:recent", &json!({ "threads": [thread] }))];
    let read = source.parse(&responses, &context(MONDAY_FIRST_TICK, saved)).unwrap().remove(0);
    assert_eq!(read.window_from, Some(at(MONDAY_FIRST_TICK).minus_ms(window_ms).to_iso()));
    ingest(&db, read, MONDAY_FIRST_TICK);
    assert_eq!(event_row(&db, "gmail:synthetic-friday").1.as_deref(), Some("p1.sentry_mail"));
}
