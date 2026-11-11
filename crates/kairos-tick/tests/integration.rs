//! 파서, ingest, collect의 실제 경로를 지나고 외부 호출만 고정 응답으로 바꾼다
//! 배포일 경계 시험은 kairos-policy, install 시험 둘은 kairos-ops의 시험이 맡는다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_agenda::{KairosControl, StateBuilder};
use kairos_core::KairosHome;
use kairos_core::model::{
    CollectContext, McpResponse, NormalizedObject, Outcome, QuerySpec, Source, SourceName, SourceResult,
};
use kairos_core::ports::{Clock, FixedClock};
use kairos_core::process::ProcessOutput;
use kairos_ledger::upcoming_deploys;
use kairos_ops::MaintenanceRepo;
use kairos_policy::{DeployLinker, DeployStage};
use kairos_source_mcp::calendar::deploy_calendar;
use kairos_source_mcp::{CalendarSource, SlackSource};
use kairos_store::{Database, params};
use kairos_tick::{IngestPayload, ResponseIngestor};
use serde_json::{Value, json};

/// 시험 프로필(`Profile::fixture`)의 본인 이름.
const SELF_NAME: &str = "김준서";

const WEDNESDAY_15: &str = "2026-11-04T06:00:00.000Z";
const FIRST: &str = "2026-11-04T06:00:00.000Z";
const SECOND: &str = "2026-11-04T06:03:00.000Z";
const THIRD: &str = "2026-11-04T06:06:00.000Z";
const MILESTONE: &str = "합성 알림 고도화";

fn fixed(now: &str) -> Arc<dyn Clock> {
    Arc::new(FixedClock(at(now)))
}

fn calendar_at(now: &str) -> CalendarSource {
    CalendarSource::new(fixed(now))
}

fn calendar_body(events: Value) -> Value {
    json!({ "accessRole": "owner", "summary": SELF_EMAIL, "timeZone": "Asia/Seoul", "events": events })
}

fn no_slack_results() -> Value {
    slack_body("# Search Results for: x\n\nNo results found.")
}

fn ingest_responses(db: &Database, source: &dyn Source, now: &str, responses: Vec<McpResponse>) {
    let payload = IngestPayload { now: at(now), source: source.name(), responses };
    ResponseIngestor::new(db, fixed(now)).ingest_responses(source, &payload, &none()).unwrap();
}

fn feed(db: &Database, query: &str, events: Value, now: &str) {
    let responses = vec![json_response(&format!("calendar:{query}"), &calendar_body(events))];
    let payload = IngestPayload { now: at(now), source: SourceName::Calendar, responses };
    let summary = ResponseIngestor::new(db, fixed(now)).ingest_responses(&calendar_at(now), &payload, &none()).unwrap();
    assert_eq!(summary.results.iter().map(|row| row.outcome).collect::<Vec<_>>(), [Outcome::Ok]);
}

fn github_run(objects: Vec<NormalizedObject>, now: &str) -> SourceResult {
    SourceResult {
        source: SourceName::Github,
        query: "assigned".into(),
        parsed_count: objects.len() as i64,
        objects,
        started_at: now.into(),
        finished_at: now.into(),
        watermark: Some(now.into()),
        snapshot: true,
        ..result()
    }
}

fn deploy_event(start: Value, end: Value) -> Value {
    json!({
        "id": "syntheticdeploy",
        "status": "confirmed",
        "summary": format!("[배포] 합성 서비스 {MILESTONE}"),
        "description": "저장되면 안 되는 설명",
        "start": start,
        "end": end,
        // 배포 캘린더에서 직접 만든 일정이다. 주최자를 믿을 수 없는 [배포]는 배포로 들이지 않는다.
        "organizer": { "email": deploy_calendar(), "self": true },
    })
}

/// (유형, 규칙, baseline, 표시 값) 네 열을 시각순으로.
fn events(db: &Database) -> Vec<(String, Option<String>, i64, Option<String>)> {
    let mut statement = db
        .connection()
        .prepare("SELECT type, rule_id, baseline, close_reason FROM events ORDER BY occurred_at, id")
        .unwrap();
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
    rows.map(Result::unwrap).collect()
}

fn would_show(db: &Database) -> Vec<kairos_agenda::WouldShowRow> {
    StateBuilder::new(db.connection()).would_show_events().unwrap()
}

#[test]
fn 십일월_오일_종일_배포는_시각_변환_없이_11_5이고_11_4_근무일에_d_1로_연결된다() {
    let db = test_db();
    let event = deploy_event(json!({ "date": "2026-11-05T00:00:00Z" }), json!({ "date": "2026-11-06T00:00:00Z" }));
    feed(&db, "deploy", json!([event]), WEDNESDAY_15);
    let objects = vec![
        NormalizedObject { milestone_title: Some(MILESTONE.into()), ..issue(1) },
        issue(2),
        NormalizedObject { text_head: Some(format!("{MILESTONE} 합성 PR")), ..pull_request(3) },
    ];
    ingest(&db, github_run(objects, WEDNESDAY_15), WEDNESDAY_15);

    let stored: (i64, String, String, Option<String>, Option<String>) = db
        .connection()
        .query_row(
            "SELECT all_day, start_date, end_date, start_at, end_at FROM objects WHERE kind = 'deploy'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(stored, (1, "2026-11-05".to_owned(), "2026-11-06".to_owned(), None, None));

    let deploys = upcoming_deploys(db.connection(), at(WEDNESDAY_15)).unwrap();
    assert_eq!(deploys[0].date.to_string(), "2026-11-05");
    assert_eq!(deploys[0].link.milestones, [MILESTONE.to_lowercase()]);
    let mut keys = deploys[0].link.object_keys.clone();
    keys.sort();
    assert_eq!(keys, ["gh:acme/widget#1", "gh:acme/widget#3"]);

    let rows = events(&db);
    let risk = rows.iter().find(|row| row.0 == "deploy_risk_d1").unwrap();
    assert_eq!((risk.1.as_deref(), risk.2), (Some("p1.deploy_d1"), 0));
}

#[test]
fn 십삼분_전에_생긴_회의_재생_다음_틱에_p0_1건이고_재수집은_늘리지_않고_끝나면_닫힌다() {
    let db = test_db();
    // 측정값: 시작 12분 50초 전 생성. 생성 직전 틱과 3분 8초 뒤 틱 사이에 들어온다.
    let before = "2026-11-04T05:59:52.000Z";
    let next = "2026-11-04T06:03:08.000Z";
    let meeting = json!({
        "id": "syntheticlate",
        "status": "confirmed",
        "summary": "합성 긴급 회의",
        "description": "저장되면 안 되는 설명",
        "created": "2026-11-04T06:00:00.000Z",
        "start": { "dateTime": "2026-11-04T15:12:50+09:00" },
        "end": { "dateTime": "2026-11-04T15:42:50+09:00" },
        "organizer": { "email": "organizer@example.com" },
        "attendees": [
            { "email": SELF_EMAIL, "self": true, "responseStatus": "needsAction" },
            { "email": "attendee@example.com", "responseStatus": "accepted" },
        ],
    });
    feed(&db, "personal", json!([]), before);
    feed(&db, "personal", json!([meeting.clone()]), next);

    let shown = would_show(&db);
    assert_eq!(shown[0].tier.as_str(), "P0");
    assert_eq!(
        (shown[0].event.rule_id.as_str(), shown[0].event.event_type.as_str()),
        ("p0.meeting_imminent", "cal_created")
    );
    assert!(!shown[0].event.catchup);
    let calendar = calendar_at(next);
    let state =
        StateBuilder::new(db.connection()).build(&[&calendar], at(next), &KairosControl::default(), None).unwrap();
    assert_eq!(state.counts.would_show.p0, 1);
    let margin = at(meeting["start"]["dateTime"].as_str().unwrap()).ms_since(at(next));
    assert_eq!(margin, (9 * 60 + 42) * 1000);

    feed(&db, "personal", json!([meeting.clone()]), "2026-11-04T06:06:08.000Z");
    assert_eq!(events(&db).len(), 1);

    let mut statement = db.connection().prepare("SELECT * FROM objects").unwrap();
    let width = statement.column_count();
    let stored: Vec<String> = statement
        .query_map([], |row| {
            Ok((0..width).map(|index| format!("{:?}", row.get_ref(index).unwrap())).collect::<Vec<_>>().join(","))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let stored = stored.join("\n");
    assert!(!stored.contains("attendee@example.com"));
    assert!(!stored.contains("저장되면 안 되는"));

    feed(&db, "personal", json!([meeting]), "2026-11-04T06:43:08.000Z");
    assert_eq!(events(&db)[0].3.as_deref(), Some("expired"));
    assert!(would_show(&db).is_empty());
}

/// 일정의 날짜 해석은 kairos-policy의 시험이 맡는다. 여기서는 순간이 KST 날짜를 거쳐 단계로 가는 길을 본다.
#[test]
fn 배포일_경계_kst_자정을_기준으로_d_1과_당일이_갈린다() {
    let deploy = date("2026-11-05");
    let stage_at = |instant: &str| DeployLinker::stage(deploy, at(instant).local_date(), &none());
    assert_eq!(stage_at("2026-11-04T14:59:00Z"), Some(DeployStage::D1)); // 11/4 23:59 KST
    assert_eq!(stage_at("2026-11-04T15:00:00Z"), Some(DeployStage::D0)); // 11/5 00:00 KST
    assert_eq!(stage_at("2026-11-03T14:59:00Z"), None); // 11/3 23:59 KST
}

fn cursor_of(home: &KairosHome, source: &str, query: &str) -> Option<String> {
    let db = Database::open(home).unwrap();
    db.connection()
        .query_row("SELECT watermark FROM cursor WHERE source = ? AND query = ?", params![source, query], |row| {
            row.get(0)
        })
        .ok()
        .flatten()
}

/// 수집 모드의 대역. 소스별 응답을 kairos ingest와 같은 몸통으로 넘기고 drop 결과를 돌려준다.
fn mode_returning(
    home: &KairosHome,
    now: &str,
    slack_response: fn(&str) -> McpResponse,
) -> Arc<dyn kairos_core::process::ProcessRunner> {
    let (home, now) = (home.clone(), now.to_owned());
    runner(move |_| {
        let db = Database::open(&home).unwrap();
        let personal = json_response("calendar:personal", &calendar_body(json!([])));
        ingest_responses(&db, &calendar_at(&now), &now, vec![personal]);
        let slack = SlackSource::new(fixed(&now));
        let responses = slack.queries().iter().map(|query| slack_response(&query.id)).collect();
        ingest_responses(&db, &slack, &now, responses);
        dropped(json!({}))
    })
}

fn sources_at(
    now: &str,
    run: impl Fn(&CollectContext) -> Vec<SourceResult> + Send + Sync + 'static,
) -> Vec<Arc<dyn Source>> {
    let github = direct_with(SourceName::Github, vec![QuerySpec::new("assigned", 180, 300)], run);
    vec![Arc::new(calendar_at(now)), Arc::new(SlackSource::new(fixed(now))), github]
}

fn slack_ok(query: &str) -> McpResponse {
    json_response(&format!("slack:{query}"), &no_slack_results())
}

fn slack_auth(query: &str) -> McpResponse {
    error_response(&format!("slack:{query}"), "invalid_auth: 합성 오류")
}

fn github_ok(now: &'static str) -> impl Fn(&CollectContext) -> Vec<SourceResult> + Send + Sync + 'static {
    move |_| vec![github_run(vec![issue(1)], now)]
}

#[test]
fn 실패_주입_인증이_막힌_소스의_커서는_그대로고_나머지는_전진하며_틱은_멈추지_않는다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    tick(&home, FIRST, sources_at(FIRST, github_ok(FIRST)))
        .claude(&plugin, mode_returning(&home, FIRST, slack_ok))
        .forced();
    let slack_cursor = cursor_of(&home, "slack", "mentions");
    assert_eq!(slack_cursor.as_deref(), Some(FIRST));
    assert_eq!(cursor_of(&home, "github", "assigned").as_deref(), Some(FIRST));

    // Slack만 인증 실패
    let blocked = tick(&home, SECOND, sources_at(SECOND, github_ok(SECOND)))
        .claude(&plugin, mode_returning(&home, SECOND, slack_auth))
        .forced();
    assert!(!blocked.halted);
    let slack_rows: Vec<_> = blocked
        .mode_outcomes
        .iter()
        .filter(|row| row.source == SourceName::Slack)
        .map(|row| (row.outcome, row.error_code.as_deref()))
        .collect();
    assert_eq!(slack_rows, [(Outcome::Auth, Some("mcp_auth")); 3]);
    let github_outcomes = |rows: &[kairos_tick::OutcomeRow]| -> Vec<Outcome> {
        rows.iter().filter(|row| row.source == SourceName::Github).map(|row| row.outcome).collect()
    };
    assert_eq!(github_outcomes(&blocked.outcomes), [Outcome::Ok]);
    assert_eq!(cursor_of(&home, "slack", "mentions"), slack_cursor);
    assert_eq!(cursor_of(&home, "calendar", "personal").as_deref(), Some(SECOND));
    assert_eq!(cursor_of(&home, "github", "assigned").as_deref(), Some(SECOND));

    // GitHub만 인증 실패, Slack은 복구
    let github_auth = |_: &CollectContext| {
        vec![SourceResult {
            outcome: Outcome::Auth,
            error_code: Some("gh_auth".into()),
            ..github_run(Vec::new(), THIRD)
        }]
    };
    let recovered = tick(&home, THIRD, sources_at(THIRD, github_auth))
        .claude(&plugin, mode_returning(&home, THIRD, slack_ok))
        .forced();
    assert_eq!(github_outcomes(&recovered.outcomes), [Outcome::Auth]);
    assert_eq!(cursor_of(&home, "github", "assigned").as_deref(), Some(SECOND));
    assert_eq!(cursor_of(&home, "slack", "mentions").as_deref(), Some(THIRD));
}

#[test]
fn 모드_실행이_실패해도_이미_기록한_질의와_차례가_아닌_질의에는_실패_행을_쓰지_않는다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let db = Database::open(&home).unwrap();
    let calendar = calendar_at(FIRST);
    for query in calendar.queries() {
        feed(&db, &query.id, json!([]), FIRST);
    }
    let slack = SlackSource::new(fixed(FIRST));
    let responses = slack.queries().iter().map(|query| slack_ok(&query.id)).collect();
    ingest_responses(&db, &slack, FIRST, responses);
    drop(db);

    // 네트워크 장애 재현: Calendar personal만 기록한 뒤 훅이 실패 drop으로 끝난다.
    let inner = home.clone();
    let failing = runner(move |_| {
        let db = Database::open(&inner).unwrap();
        let responses = vec![error_response("calendar:personal", "kairos:tool_missing")];
        ingest_responses(&db, &calendar_at(SECOND), SECOND, responses);
        let stdout = claude_stdout(json!({ "result": "Prompt dropped by a hook: collector-failed" }));
        ProcessOutput::exited(0, stdout)
    });
    let sources: Vec<Arc<dyn Source>> = vec![Arc::new(calendar_at(SECOND)), Arc::new(SlackSource::new(fixed(SECOND)))];
    let report = tick(&home, SECOND, sources).claude(&plugin, failing).forced();
    assert!(!report.halted);
    let row = |row: &kairos_tick::OutcomeRow| (row.source, row.query.clone(), row.outcome);
    assert_eq!(
        report.mode_outcomes.iter().map(row).collect::<Vec<_>>(),
        [(SourceName::Calendar, "personal".to_owned(), Outcome::ToolMissing)]
    );
    let failed: Vec<_> =
        report.outcomes.iter().map(|row| (row.query.as_str(), row.outcome, row.error_code.as_deref())).collect();
    let broken = (Outcome::ContractBreak, Some("drop_contract_broken"));
    assert_eq!(
        failed,
        [("mentions", broken.0, broken.1), ("to_me", broken.0, broken.1), ("from_me", broken.0, broken.1)]
    );
}

#[test]
fn 정비가_지운_이벤트는_재수집으로_되살아나지_않고_답한_댓글이_다시_열리지_않는다() {
    let db = test_db();
    let start = "2026-11-04T06:00:00.000Z";
    let later = "2026-11-04T08:00:00.000Z";
    let month = "2026-12-05T08:00:00.000Z";
    let answered = kairos_core::model::NormalizedEvent {
        actor_kind: kairos_core::model::ActorKind::SelfActor,
        actor_id: Some("kairos-dev".into()),
        ..comment("answered", "2026-11-04T07:30:00.000Z")
    };
    let thread = vec![comment("asked", "2026-11-04T07:00:00.000Z"), answered];
    ingest(&db, github_run(vec![issue(1)], start), start);
    ingest(&db, SourceResult { events: thread.clone(), ..github_run(vec![issue(1)], later) }, later);
    assert_eq!(event_text(&db, "gh:comment:asked", "close_reason").as_deref(), Some("done_evidence"));

    assert_eq!(MaintenanceRepo::new(db.connection()).prune_events(at(month)).unwrap(), 2);
    // GitHub는 객체의 최근 댓글을 틱마다 다시 읽는다. 같은 사건이 그대로 돌아온다.
    let report = ingest(&db, SourceResult { events: thread, ..github_run(vec![issue(1)], month) }, month);
    assert_eq!(report.events_inserted, 0);
    assert_eq!(events(&db).len(), 0);
    assert!(would_show(&db).is_empty());
}

#[test]
fn 공휴일이면_watch_tsv가_active_0이다_plan_tick이_호출을_줄여도_감시자가_지연으로_오해하지_않는다() {
    let (_dir, home) = temp_home();
    let holiday = json!({
        "id": "syntheticholiday",
        "status": "confirmed",
        "summary": "합성 공휴일",
        "description": "공휴일\n합성",
        "start": { "date": "2026-11-04" },
        "end": { "date": "2026-11-05" },
    });
    feed(&Database::open(&home).unwrap(), "holidays", json!([holiday]), WEDNESDAY_15);
    tick(&home, WEDNESDAY_15, Vec::new()).run();
    let text = fs::read_to_string(home.file("watch.tsv")).unwrap();
    assert_eq!(text.split('\n').next(), Some("active\t0"));
}

#[test]
fn 공유_캘린더의_연차는_watch_tsv를_끄지_않고_덮지_못한_구간은_감시자_경고가_된다() {
    let (_dir, home) = temp_home();
    let leave = json!({
        "id": "syntheticleave",
        "status": "confirmed",
        "summary": format!("{SELF_NAME} 연차"),
        "start": { "date": "2026-11-04" },
        "end": { "date": "2026-11-05" },
    });
    let db = Database::open(&home).unwrap();
    feed(&db, "crew_off", json!([leave]), WEDNESDAY_15);
    let quarantined = SourceResult {
        source: SourceName::Slack,
        query: "mentions".into(),
        error_code: Some("record_quarantined".into()),
        started_at: WEDNESDAY_15.into(),
        finished_at: WEDNESDAY_15.into(),
        watermark: Some(WEDNESDAY_15.into()),
        ..result()
    };
    ingest(&db, quarantined, WEDNESDAY_15);
    drop(db);
    tick(&home, WEDNESDAY_15, Vec::new()).run();
    let text = fs::read_to_string(home.file("watch.tsv")).unwrap();
    assert_eq!(text.split('\n').take(2).collect::<Vec<_>>(), ["active\t1", "alert\tcoverage gap"]);
}
