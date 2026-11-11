//! slack, github-source, items의 DB 쪽 시험. Notion은 tests/notion_db.rs에 있다
//! 판정만 앞 크레이트가 옮겼고, 소스 파서에서 ledger까지 잇는 경로를 여기서 시험한다.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant as Monotonic;

use common::fakes::*;
use common::*;
use kairos_agenda::TriageDecision;
use kairos_core::model::{CollectContext, CursorState, McpSource, Source, SourceResult};
use kairos_core::ports::{CancelToken, Clock, FixedClock};
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest};
use kairos_core::{Instant, js};
use kairos_ledger::{OffDateBook, check_invariants};
use kairos_ops::MaintenanceRepo;
use kairos_source_github::{GithubConfig, GithubSource};
use kairos_source_mcp::{CalendarSource, SlackSource};
use kairos_store::{Database, params};
use serde_json::{Value, json};

const TWO_HOURS_LATER: &str = "2026-03-02T03:00:00.000Z";

fn fixed(now: &str) -> Arc<dyn Clock> {
    Arc::new(FixedClock(at(now)))
}

fn context_of(source: &dyn Source, now: &str) -> CollectContext {
    let cursors: BTreeMap<String, Option<CursorState>> =
        source.queries().iter().map(|query| (query.id.clone(), None)).collect();
    CollectContext { now: at(now), cursors, thread_reads: None, cancel: CancelToken::new() }
}

fn parse_as(query: &str, body: &Value, now: &str) -> SourceResult {
    let source = CalendarSource::new(fixed(now));
    let responses = [json_response(&format!("calendar:{query}"), body)];
    let mut results = source.parse(&responses, &context_of(&source, now)).unwrap();
    assert_eq!(results.len(), 1);
    results.remove(0)
}

fn meeting_body(events: Value) -> Value {
    json!({ "accessRole": "owner", "summary": SELF_EMAIL, "events": events })
}

fn new_meeting() -> Value {
    json!({
        "id": "syntheticnew",
        "status": "confirmed",
        "summary": "새 회의",
        "start": { "dateTime": "2026-03-02T12:30:00+09:00" },
        "end": { "dateTime": "2026-03-02T13:00:00+09:00" },
        "attendees": [{ "self": true, "responseStatus": "needsAction" }],
    })
}

fn far_meeting() -> Value {
    let mut far = new_meeting();
    far["start"] = json!({ "dateTime": "2026-03-06T10:00:00+09:00" });
    far["end"] = json!({ "dateTime": "2026-03-06T11:00:00+09:00" });
    far
}

/// 이벤트의 (유형, 규칙, baseline).
fn events(db: &Database) -> Vec<(String, Option<String>, i64)> {
    let mut statement =
        db.connection().prepare("SELECT type, rule_id, baseline FROM events ORDER BY occurred_at, id").unwrap();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

fn off_size(db: &Database) -> usize {
    OffDateBook::new(db.connection()).load_off_dates(&none()).unwrap().iter().count()
}

fn dates(off_dates: &kairos_core::OffDates) -> Vec<String> {
    off_dates.iter().map(|date| date.to_string()).collect()
}

#[test]
fn 지워진_연차는_ok_스냅샷_2회_부재_뒤_쉬는_날에서_빠진다() {
    let db = test_db();
    ingest(&db, parse_as("crew_off", &json_fixture("calendar/crew.json"), T0), T0);
    let empty = json!({ "accessRole": "reader", "summary": "합성 크루 캘린더" });
    ingest(&db, parse_as("crew_off", &empty, T1), T1);
    assert_eq!(off_size(&db), 2, "한 번 빠진 것으로는 지우지 않는다");
    ingest(&db, parse_as("crew_off", &empty, T2), T2);
    assert_eq!(off_size(&db), 0);
}

#[test]
fn 새_회의_60분_안이면_응답_상태와_무관하게_p0다() {
    let db = test_db();
    ingest(&db, parse_as("personal", &meeting_body(json!([])), T0), T0);
    let body = meeting_body(json!([new_meeting()]));
    ingest(&db, parse_as("personal", &body, TWO_HOURS_LATER), TWO_HOURS_LATER);
    assert_eq!(events(&db)[0], ("cal_created".to_owned(), Some("p0.meeting_imminent".to_owned()), 0));
}

#[test]
fn 첫_수집의_회의는_baseline이고_p0가_표시되지_않는다() {
    let db = test_db();
    let body = meeting_body(json!([new_meeting()]));
    ingest(&db, parse_as("personal", &body, TWO_HOURS_LATER), TWO_HOURS_LATER);
    assert_eq!(events(&db)[0].2, 1);
}

#[test]
fn 시간_변경은_같은_객체의_이동으로_잡힌다() {
    let db = test_db();
    ingest(&db, parse_as("personal", &meeting_body(json!([far_meeting()])), T0), T0);
    let body = meeting_body(json!([new_meeting()]));
    ingest(&db, parse_as("personal", &body, TWO_HOURS_LATER), TWO_HOURS_LATER);
    let rows = events(&db);
    let moved = rows.iter().find(|row| row.0 == "cal_moved").unwrap();
    assert_eq!(moved.1.as_deref(), Some("p0.meeting_imminent"));
}

#[test]
fn 삭제는_ok_스냅샷_2회_연속_부재로_취소_증거가_된다() {
    let db = test_db();
    ingest(&db, parse_as("personal", &meeting_body(json!([far_meeting()])), T0), T0);
    ingest(&db, parse_as("personal", &meeting_body(json!([])), T1), T1);
    assert!(!events(&db).iter().any(|row| row.0 == "object_withdrawn"));
    ingest(&db, parse_as("personal", &meeting_body(json!([])), T2), T2);
    let rows = events(&db);
    let withdrawn = rows.iter().find(|row| row.0 == "object_withdrawn").unwrap();
    assert_eq!(withdrawn.1.as_deref(), Some("p2.meeting_change"));
}

#[test]
fn 이미_저장된_긴_기간도_날짜를_끝까지_펼치지_않는다() {
    let db = test_db();
    ingest(&db, parse_as("crew_off", &json_fixture("calendar/crew.json"), T0), T0);
    db.connection().execute_batch("UPDATE objects SET end_date = '9999-12-31'").unwrap();
    let started = Monotonic::now();
    assert_eq!(off_size(&db), 31);
    assert!(started.elapsed().as_millis() < 200);
}

#[test]
fn 공유_캘린더의_연차는_수집과_감시를_끄지_않고_사용자에게_드러난다() {
    let db = test_db();
    ingest(&db, parse_as("crew_off", &json_fixture("calendar/crew.json"), T0), T0);
    ingest(&db, parse_as("holidays", &json_fixture("calendar/holidays.json"), T0), T0);
    let book = OffDateBook::new(db.connection());
    assert_eq!(dates(&book.load_off_dates(&none()).unwrap()), ["2026-03-03", "2026-03-09", "2026-03-10"]);
    assert_eq!(dates(&book.load_watch_off_dates(&off(&["2026-03-20"])).unwrap()), ["2026-03-03", "2026-03-20"]);
    let shared = |now: &str| -> Vec<String> {
        book.shared_off_dates(at(now)).unwrap().iter().map(ToString::to_string).collect()
    };
    assert_eq!(shared(T0), ["2026-03-09", "2026-03-10"]);
    assert_eq!(shared("2026-03-10T01:00:00.000Z"), ["2026-03-10"]);
}

fn slack_parse(id: &str, markdown: &str, now: &str) -> SourceResult {
    let source = SlackSource::new(fixed(now));
    let responses = [json_response(&format!("slack:{id}"), &slack_body(markdown))];
    let mut results = source.parse(&responses, &context_of(&source, now)).unwrap();
    assert_eq!(results.len(), 1);
    results.remove(0)
}

fn event_row(db: &Database, key: &str) -> (Option<String>, Option<String>, i64) {
    db.connection()
        .query_row("SELECT disposition, rule_id, baseline FROM events WHERE dedupe_key = ?", params![key], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap()
}

#[test]
fn t0_이후의_멘션은_등급을_받고_내_스레드_답장으로_닫힌다() {
    let db = test_db();
    let started = "2026-03-02T00:00:00.000Z";
    let empty = format!("# Search Results for: <@{ME}>\n\nNo results found.\n");
    ingest(&db, slack_parse("mentions", &empty, started), started);

    ingest(&db, slack_parse("mentions", &fixture("slack/mentions-page.md"), T3), T3);
    let key = "slack:C0SYNTH01:1772413200.000100";
    assert_eq!(event_row(&db, key), (Some("opened".into()), Some("p1.direct_mention".into()), 0));
    assert_eq!(event_row(&db, "slack:C0SYNTH02:1772413260.000200").1.as_deref(), Some("p2.multi_mention"));
    assert_eq!(event_row(&db, "slack:C0SYNTH01:1772413320.000300").1.as_deref(), Some("noise.involves"));

    let empty_from = format!("# Search Results for: from:<@{ME}>\n\nNo results found.\n");
    ingest(&db, slack_parse("from_me", &empty_from, started), started);
    ingest(&db, slack_parse("from_me", &fixture("slack/from-me-page.md"), T3), T3);
    assert_eq!(event_text(&db, key, "close_reason").as_deref(), Some("done_evidence"));
    let evidence = event_text(&db, key, "close_evidence_event");
    assert_eq!(evidence.as_deref(), Some("slack:C0SYNTH01:1772413500.000600"));
}

/// 시험용 gh. 고정 노드를 질의별로 돌려준다.
fn fake_gh(request: &ProcessRequest) -> ProcessOutput {
    let table: [(&str, &[&str]); 5] = [
        ("review-requested", &["PR_synth_b"]),
        ("assignee:", &["I_synth_c"]),
        ("author:", &["PR_synth_a", "PR_synth_b"]),
        ("mentions:", &["I_synth_c", "PR_synth_a"]),
        ("is:closed", &["PR_synth_d", "I_synth_e"]),
    ];
    let arg = |index: usize| request.args.get(index).map(String::as_str);
    match (arg(0), arg(1), arg(3)) {
        (Some("auth"), ..) => ProcessOutput::exited(0, "synthetic-token\n"),
        (_, Some("rate_limit"), _) => {
            ProcessOutput::exited(0, json!({ "resources": { "search": { "remaining": 30 } } }).to_string())
        }
        (.., Some("search/issues")) => {
            let at = request.args.iter().position(|value| value == "-f").unwrap();
            let q = &request.args[at + 1];
            let ids = table.iter().find(|(needle, _)| q.contains(needle)).map_or(&[][..], |(_, ids)| *ids);
            let items: Vec<Value> = ids.iter().map(|id| json!({ "node_id": id })).collect();
            let body = json!({ "total_count": ids.len(), "incomplete_results": false, "items": items });
            ProcessOutput::exited(0, body.to_string())
        }
        (_, Some("graphql"), _) => {
            let nodes = js::parse_json(&fixture("github/nodes.json").replace("__LOGIN__", "kairos-dev")).unwrap();
            let body: Value = serde_json::from_str(request.stdin.as_deref().unwrap()).unwrap();
            let found: Vec<Value> = body["variables"]["ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    nodes.as_object().unwrap().values().find(|node| node["id"] == *id).cloned().unwrap_or(Value::Null)
                })
                .collect();
            ProcessOutput::exited(
                0,
                json!({ "data": { "viewer": { "login": "kairos-dev" }, "nodes": found } }).to_string(),
            )
        }
        _ => ProcessOutput::ended(ProcessEnd::Exited(1)),
    }
}

#[test]
fn ingest_통합_첫_수집은_baseline이라_표시_후보가_없고_같은_응답_재수집은_이벤트를_늘리지_않는다() {
    let db = test_db();
    let config = || GithubConfig::from_vars(|_| None);
    let collect = |now: &str| -> Vec<SourceResult> {
        let source = GithubSource::new(runner(fake_gh), fixed(now), config());
        let context =
            CollectContext { now: at(now), cursors: BTreeMap::new(), thread_reads: None, cancel: CancelToken::new() };
        kairos_core::model::DirectSource::collect(&source, &context)
    };
    let count_where = |condition: &str| -> i64 {
        db.connection().query_row(&format!("SELECT count(*) FROM {condition}"), [], |row| row.get(0)).unwrap()
    };
    for row in collect(T0) {
        ingest(&db, row, T0);
    }
    assert_eq!(count(&db, "source_run"), 5);
    assert_eq!(count_where("events WHERE disposition = 'opened' AND baseline = 0"), 0);
    assert!(count_where("events WHERE baseline = 1") > 0);
    assert_eq!(count_where("objects WHERE kind = 'pull_request'"), 3);

    // 같은 응답을 다시 읽어도 이벤트가 늘지 않는다.
    let before = count(&db, "events");
    let later: String = Instant::from_millis(at(T0).millis() + 180_000).to_iso();
    for row in collect(&later) {
        ingest(&db, row, &later);
    }
    assert_eq!(count(&db, "events"), before);
    assert_eq!(count_where("objects WHERE milestone_title = '합성 마일스톤 알파'"), 2);
}

fn after_days(when: &str, days: i64) -> Instant {
    at(when).plus_ms(days * 86_400_000)
}

#[test]
fn 정비_열린_항목의_객체와_종료_근거는_지우지_않고_오래_닫힌_항목은_지운다() {
    let db = test_db();
    prime_slack(&db);
    let key = format!("slack:{CHANNEL}:1");
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    slack(T1).events(vec![my_message("2", T1, &key)]).run(&db);
    refresh(&db, T1);
    github(T0).objects(vec![issue(1)]).run(&db);
    let batch = actions(&db).triage_batch(at(T1)).unwrap();
    actions(&db).triage_decide(&[batch.rows[0].id], TriageDecision::Keep, at(T1)).unwrap();

    let repo = MaintenanceRepo::new(db.connection());
    let month = after_days(T1, 31);
    assert_eq!(repo.prune_objects(month).unwrap(), 0, "열린 항목의 객체는 남긴다");
    assert_eq!(repo.prune_items(after_days(T1, 29)).unwrap(), 0);
    repo.prune_events(month).unwrap();
    assert_eq!(
        check_invariants(db.connection()).unwrap().item_closures_without_evidence,
        0,
        "살아 있는 종료 기록의 근거는 남는다"
    );
    assert_eq!(repo.prune_items(month).unwrap(), 1);
    let states: Vec<_> = items(&db).iter().map(|row| row.state.as_str()).collect();
    assert_eq!(states, ["open"]);
}

#[test]
fn 정비_사용자가_끝낸_항목의_오래된_이벤트는_열린_이벤트로_남지_않는다() {
    let db = test_db();
    prime_slack(&db);
    slack(T0).events(vec![mention("1", T0)]).run(&db);
    refresh(&db, T0);
    let repo = MaintenanceRepo::new(db.connection());
    let month = after_days(T0, 31);
    assert_eq!(repo.prune_events(month).unwrap(), 0, "살아 있는 항목의 이벤트는 지우지 않는다");
    actions(&db).done(&[1], at(T0), false).unwrap();
    assert!(repo.prune_events(month).unwrap() >= 1);
    let key = format!("slack:{CHANNEL}:1");
    let left: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM events WHERE dedupe_key = ?", params![key], |row| row.get(0))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn 버린_기준선_건은_30일이_지나도_triage로_돌아오지_않는다() {
    let db = test_db();
    github(T0).objects(vec![issue(1)]).run(&db);
    let batch = actions(&db).triage_batch(at(T1)).unwrap();
    actions(&db).triage_decide(&[batch.rows[0].id], TriageDecision::Drop, at(T1)).unwrap();
    assert_eq!(MaintenanceRepo::new(db.connection()).prune_items(after_days(T1, 31)).unwrap(), 0);
    assert!(actions(&db).triage_batch(at(T1)).unwrap().rows.is_empty());
}
