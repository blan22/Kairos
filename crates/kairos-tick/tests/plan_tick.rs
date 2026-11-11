//! stdin 시험은 tests/mode.rs에 있다
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use common::fakes::*;
use common::*;
use kairos_core::OffDates;
use kairos_core::model::{
    McpResponse, McpSource, Outcome, ParserFailed, QuerySpec, Source, SourceMode, SourceName, SourceResult,
};
use kairos_core::ports::{Clock, FixedClock};
use kairos_source_mcp::{CalendarSource, GmailSource, NotionSource, SlackSource};
use kairos_store::Database;
use kairos_tick::{
    IngestPayload, OFF_HOURS_INTERVAL_SECONDS, PLAN_VERSION, ResponseIngestor, TickPlanner, is_ingest_payload,
    page_info_of,
};
use serde_json::{Value, json};

const WEDNESDAY_NOON: &str = "2026-03-04T03:00:00.000Z"; // 12:00 KST
const NIGHT: &str = "2026-03-04T12:00:00.000Z"; // 21:00 KST

fn clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock(at(WEDNESDAY_NOON)))
}

fn calendar_source() -> CalendarSource {
    CalendarSource::new(clock())
}

fn gmail() -> GmailSource {
    GmailSource::new(clock())
}

fn slack_source() -> SlackSource {
    SlackSource::new(clock())
}

fn later(base: &str, minutes: i64) -> String {
    plus_minutes(base, minutes)
}

fn ids(db: &Database, source: &dyn Source, now: &str, off_dates: &OffDates) -> Vec<String> {
    let due = TickPlanner::new(db.connection()).due_queries(source, at(now), off_dates, false).unwrap();
    due.into_iter().map(|query| query.id).collect()
}

fn due_now(db: &Database, source: &dyn Source, now: &str) -> Vec<String> {
    ids(db, source, now, &none())
}

fn ran_as(db: &Database, source: SourceName, query: &str, when: &str, outcome: Outcome) {
    let ok = outcome == Outcome::Ok;
    let row = SourceResult {
        source,
        query: query.to_owned(),
        outcome,
        started_at: when.to_owned(),
        finished_at: when.to_owned(),
        watermark: ok.then(|| when.to_owned()),
        ..result()
    };
    ingest(db, row, when);
}

fn ran(db: &Database, source: SourceName, query: &str, when: &str) {
    ran_as(db, source, query, when, Outcome::Ok);
}

fn plan(db: &Database, sources: &[&dyn Source], now: &str, force: bool) -> kairos_tick::TickPlan {
    TickPlanner::new(db.connection()).plan_tick(sources, at(now), &none(), force).unwrap()
}

#[test]
fn 첫_틱은_모든_질의가_차례다() {
    let db = test_db();
    let (calendar, gmail, slack, notion) = (calendar_source(), gmail(), slack_source(), NotionSource::new(clock()));
    let plan = plan(&db, &[&calendar, &gmail, &slack, &notion], WEDNESDAY_NOON, false);
    let calls: Vec<&str> = plan.calls.iter().map(|call| call.id.as_str()).collect();
    assert_eq!(
        calls,
        [
            "calendar:personal",
            "calendar:deploy",
            "calendar:holidays",
            "calendar:crew_off",
            "gmail:recent",
            "slack:mentions",
            "slack:to_me",
            "slack:from_me",
            "notion:mail",
        ]
    );
}

#[test]
fn 소스별_주기_3분_30분_60분_하루() {
    let db = test_db();
    let (calendar, gmail) = (calendar_source(), gmail());
    for query in calendar.queries() {
        ran(&db, SourceName::Calendar, &query.id, WEDNESDAY_NOON);
    }
    ran(&db, SourceName::Gmail, "recent", WEDNESDAY_NOON);
    let empty: [&str; 0] = [];
    assert_eq!(due_now(&db, &calendar, &later(WEDNESDAY_NOON, 2)), empty);
    assert_eq!(due_now(&db, &calendar, &later(WEDNESDAY_NOON, 3)), ["personal"]);
    assert_eq!(due_now(&db, &calendar, &later(WEDNESDAY_NOON, 30)), ["personal", "deploy"]);
    assert_eq!(due_now(&db, &gmail, &later(WEDNESDAY_NOON, 57)), empty);
    assert_eq!(due_now(&db, &gmail, &later(WEDNESDAY_NOON, 60)), ["recent"]);
    assert!(due_now(&db, &calendar, &later(WEDNESDAY_NOON, 24 * 60)).contains(&"holidays".to_owned()));
}

#[test]
fn launchd_틱이_앞_실행_끝보다_몇_초_이르게_와도_차례를_놓치지_않는다() {
    let db = test_db();
    ran(&db, SourceName::Slack, "mentions", WEDNESDAY_NOON);
    let almost = at(WEDNESDAY_NOON).plus_ms(175_000).to_iso();
    assert!(due_now(&db, &slack_source(), &almost).contains(&"mentions".to_owned()));
}

#[test]
fn 근무_시간의_실패는_주기를_기다리지_않고_다음_틱에_다시_시도한다() {
    let db = test_db();
    ran(&db, SourceName::Gmail, "recent", WEDNESDAY_NOON);
    ran_as(&db, SourceName::Gmail, "recent", &later(WEDNESDAY_NOON, 60), Outcome::Auth);
    assert_eq!(due_now(&db, &gmail(), &later(WEDNESDAY_NOON, 63)), ["recent"]);
}

#[test]
fn 구시_이십시_밖에는_호출을_줄인다() {
    let db = test_db();
    assert!(!TickPlanner::is_collecting_hours(at(NIGHT), &none()));
    ran(&db, SourceName::Slack, "mentions", NIGHT);
    let slack = slack_source();
    let has = |now: &str| due_now(&db, &slack, now).contains(&"mentions".to_owned());
    assert!(!has(&later(NIGHT, 3)));
    assert!(!has(&later(NIGHT, 30)));
    assert!(has(&later(NIGHT, OFF_HOURS_INTERVAL_SECONDS / 60)));
}

#[test]
fn 근무_시간_밖에서는_실패해도_틱마다_재시도하지_않는다() {
    let db = test_db();
    ran_as(&db, SourceName::Slack, "mentions", NIGHT, Outcome::Auth);
    assert!(!due_now(&db, &slack_source(), &later(NIGHT, 3)).contains(&"mentions".to_owned()));
}

#[test]
fn 주말과_공휴일_지정한_쉬는_날에는_호출을_줄인다() {
    let db = test_db();
    let saturday = at("2026-03-07T03:00:00.000Z");
    assert!(!TickPlanner::is_collecting_hours(saturday, &none()));
    assert!(TickPlanner::is_collecting_hours(at(WEDNESDAY_NOON), &none()));
    let off_dates = off(&["2026-03-04"]);
    assert!(!TickPlanner::is_collecting_hours(at(WEDNESDAY_NOON), &off_dates));
    ran(&db, SourceName::Slack, "mentions", WEDNESDAY_NOON);
    let due = ids(&db, &slack_source(), &later(WEDNESDAY_NOON, 3), &off_dates);
    assert!(!due.contains(&"mentions".to_owned()));
}

#[test]
fn 하루_주기는_근무_시간_밖에서도_그대로다() {
    let db = test_db();
    ran(&db, SourceName::Calendar, "holidays", NIGHT);
    assert!(!due_now(&db, &calendar_source(), &later(NIGHT, 120)).contains(&"holidays".to_owned()));
}

#[test]
fn 계약_시험은_주기와_무관하게_모든_질의를_낸다() {
    let db = test_db();
    ran(&db, SourceName::Gmail, "recent", WEDNESDAY_NOON);
    let plan = plan(&db, &[&gmail()], &later(WEDNESDAY_NOON, 3), true);
    assert_eq!(plan.calls.iter().map(|call| call.id.as_str()).collect::<Vec<_>>(), ["gmail:recent"]);
}

#[test]
fn 미연결_소스는_호출을_만들지_않는다() {
    let db = test_db();
    let unconnected = unconnected(SourceName::Notion, NotionSource::new(clock()).queries().to_vec());
    assert!(plan(&db, &[unconnected.as_ref()], WEDNESDAY_NOON, false).calls.is_empty());
}

#[test]
fn plan_tick_출력에는_호출_인자만_있다() {
    let db = test_db();
    let value = serde_json::to_value(plan(&db, &[&gmail()], WEDNESDAY_NOON, false)).unwrap();
    let keys: Vec<&str> = value.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["version", "now", "calls"]);
    assert_eq!(value["version"], PLAN_VERSION);
    assert_eq!(value["now"], WEDNESDAY_NOON);
}

#[test]
fn ingest_입력을_검사한다() {
    assert!(is_ingest_payload(&json!({ "now": WEDNESDAY_NOON, "source": "gmail", "responses": [] })));
    for bad in [
        Value::Null,
        json!({}),
        json!({ "now": "x", "source": "gmail", "responses": [] }),
        json!({ "now": WEDNESDAY_NOON, "source": "nope", "responses": [] }),
        json!({ "now": WEDNESDAY_NOON, "source": "gmail", "responses": [{}] }),
        json!({ "now": WEDNESDAY_NOON, "source": "gmail", "responses": [null] }),
        json!({ "now": 1, "source": "gmail", "responses": [] }),
        json!({ "now": WEDNESDAY_NOON, "source": "gmail", "responses": {} }),
    ] {
        assert!(!is_ingest_payload(&bad), "{bad}");
    }
}

fn payload(source: SourceName, responses: Vec<McpResponse>) -> IngestPayload {
    IngestPayload { now: at(WEDNESDAY_NOON), source, responses }
}

fn ingestor(db: &Database) -> ResponseIngestor<'_> {
    ResponseIngestor::new(db, clock())
}

#[test]
fn 응답을_파싱해_저장하고_건수와_코드만_돌려준다() {
    let db = test_db();
    let responses = vec![json_response("gmail:recent", &json!({}))];
    let summary = ingestor(&db).ingest_responses(&gmail(), &payload(SourceName::Gmail, responses), &none()).unwrap();
    assert_eq!(
        serde_json::to_string(&summary).unwrap(),
        r#"{"source":"gmail","results":[{"query":"recent","outcome":"ok","errorCode":null,"objects":0,"events":0,"closed":0}]}"#
    );
}

#[test]
fn 한_소스의_실패가_source_run에_그대로_남는다() {
    let db = test_db();
    let responses = vec![error_response("gmail:recent", "kairos:tool_missing")];
    let summary = ingestor(&db).ingest_responses(&gmail(), &payload(SourceName::Gmail, responses), &none()).unwrap();
    assert_eq!(summary.results[0].outcome, Outcome::ToolMissing);
    let row: (String, String) = db
        .connection()
        .query_row("SELECT outcome, error_code FROM source_run", [], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    assert_eq!(row, ("tool_missing".to_owned(), "mcp_not_connected".to_owned()));
}

struct Broken {
    inner: GmailSource,
    panics: bool,
}

impl McpSource for Broken {
    fn plan(&self, _context: &kairos_core::model::CollectContext) -> Vec<kairos_core::model::McpCall> {
        Vec::new()
    }

    fn parse(
        &self,
        _responses: &[McpResponse],
        _context: &kairos_core::model::CollectContext,
    ) -> Result<Vec<SourceResult>, ParserFailed> {
        assert!(!self.panics, "외부 문자열");
        Err(ParserFailed)
    }
}

impl Source for Broken {
    fn name(&self) -> SourceName {
        SourceName::Gmail
    }

    fn queries(&self) -> &[QuerySpec] {
        self.inner.queries()
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::Mcp(self)
    }
}

#[test]
fn 파서가_던져도_기록_없이_사라지지_않는다() {
    for panics in [false, true] {
        let db = test_db();
        let broken = Broken { inner: gmail(), panics };
        let responses = vec![json_response("gmail:recent", &json!({}))];
        let summary = ingestor(&db).ingest_responses(&broken, &payload(SourceName::Gmail, responses), &none()).unwrap();
        assert_eq!(summary.results[0].outcome, Outcome::ParseError);
        assert_eq!(summary.results[0].error_code.as_deref(), Some("parser_threw"));
    }
}

#[test]
fn 쪽_넘김에_쓰는_두_값만_고른다() {
    let long = "x".repeat(4097);
    let body = json!({ "nextPageToken": "n", "pagination_info": long, "subject": "저장금지" }).to_string();
    assert_eq!(Value::Object(page_info_of(Some(&body))), json!({ "nextPageToken": "n" }));
    assert_eq!(Value::Object(page_info_of(None)), json!({}));
    assert_eq!(Value::Object(page_info_of(Some("not json"))), json!({}));
    assert_eq!(Value::Object(page_info_of(Some("null"))), json!({}));
    let both = r#"{"pagination_info":"p","nextPageToken":"n"}"#;
    assert_eq!(
        serde_json::to_string(&page_info_of(Some(both))).unwrap(),
        r#"{"nextPageToken":"n","pagination_info":"p"}"#
    );
}

fn fast_slow() -> Arc<dyn Source> {
    mcp_fake(SourceName::Gmail, vec![QuerySpec::new("fast", 180, 0), QuerySpec::new("slow", 3600, 0)])
}

#[test]
fn collect는_이번_틱에_차례가_아닌_질의를_미기록_실패로_세지_않는다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let db = Database::open(&home).unwrap();
    ran(&db, SourceName::Gmail, "fast", WEDNESDAY_NOON);
    ran(&db, SourceName::Gmail, "slow", WEDNESDAY_NOON);
    drop(db);

    let contract = Arc::new(Mutex::new(Vec::new()));
    let seen = contract.clone();
    let inner = home.clone();
    let now = later(WEDNESDAY_NOON, 3);
    let when = now.clone();
    let mode = runner(move |request| {
        seen.lock().unwrap().push(request.env.as_ref().unwrap()["KAIROS_CONTRACT_TEST"].clone());
        ran(&Database::open(&inner).unwrap(), SourceName::Gmail, "fast", &when);
        dropped(json!({}))
    });
    let report = tick(&home, &now, vec![fast_slow()]).claude(&plugin, mode).forced();
    assert!(report.outcomes.is_empty());
    assert_eq!(*contract.lock().unwrap(), ["0"]);
}

#[test]
fn collect는_차례인_질의가_없으면_claude를_띄우지_않는다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let db = Database::open(&home).unwrap();
    ran(&db, SourceName::Gmail, "recent", WEDNESDAY_NOON);
    drop(db);
    let launched = Arc::new(Mutex::new(0));
    let count = launched.clone();
    let mode = runner(move |_| {
        *count.lock().unwrap() += 1;
        kairos_core::process::ProcessOutput::exited(0, "")
    });
    let source: Arc<dyn Source> = Arc::new(gmail());
    tick(&home, &later(WEDNESDAY_NOON, 3), vec![source]).claude(&plugin, mode).forced();
    assert_eq!(*launched.lock().unwrap(), 0);
}

#[test]
fn 해석할_수_없는_마지막_시각은_차례가_아니다() {
    let db = test_db();
    ran(&db, SourceName::Slack, "mentions", WEDNESDAY_NOON);
    db.connection().execute_batch("UPDATE source_run SET finished_at = '합성'").unwrap();
    assert!(!due_now(&db, &slack_source(), &later(WEDNESDAY_NOON, 90)).contains(&"mentions".to_owned()));
}
