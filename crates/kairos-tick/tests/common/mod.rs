//! 고정값은 모두 합성이다
#![allow(dead_code, clippy::unwrap_used)]

pub mod fakes;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

use kairos_agenda::{DisplayMode, EngineReport, Item, ItemActions, ItemEngine, ItemRepo, KairosControl, RefreshInput};
use kairos_core::model::{
    ActorKind, EventType, McpResponse, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult,
};
use kairos_core::ports::{Alert, Notifier};
use kairos_core::{Instant, KairosHome, LocalDate, OffDates};
use kairos_ledger::{IngestOptions, IngestReport, Ledger};
use kairos_store::{Database, params};
use serde_json::{Value, json};
use tempfile::TempDir;

/// 시험 프로필(`Profile::fixture`)의 본인 이름.
const SELF_NAME: &str = "김준서";

pub const T0: &str = "2026-03-02T01:00:00.000Z"; // 월요일 10:00 KST
pub const T1: &str = "2026-03-02T01:03:00.000Z";
pub const T2: &str = "2026-03-02T01:06:00.000Z";
pub const T3: &str = "2026-03-02T01:09:00.000Z";
pub const BEFORE: &str = "2026-03-01T01:00:00.000Z";
pub const CHANNEL: &str = "C0SYNTH1";
pub const MON: &str = "2026-03-02";
pub const TUE: &str = "2026-03-03";

pub fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

/// KST 벽시계 시각을 순간으로 바꾼다.
pub fn kst(date: &str, time: &str) -> String {
    at(&format!("{date}T{time}:00+09:00")).to_iso()
}

/// 어떤 시각에 분을 더한다.
pub fn plus_minutes(base: &str, minutes: i64) -> String {
    at(base).plus_ms(minutes * 60_000).to_iso()
}

pub fn date(text: &str) -> LocalDate {
    LocalDate::parse(text).unwrap()
}

pub fn off(dates: &[&str]) -> OffDates {
    dates.iter().map(|text| date(text)).collect()
}

pub fn none() -> OffDates {
    OffDates::new()
}

pub fn test_db() -> Database {
    Database::in_memory().unwrap()
}

pub fn temp_home() -> (TempDir, KairosHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at(dir.path());
    (dir, home)
}

pub fn result() -> SourceResult {
    SourceResult {
        source: SourceName::Github,
        query: "tick".into(),
        outcome: Outcome::Ok,
        started_at: T0.into(),
        finished_at: T0.into(),
        pages: 1,
        expected_count: None,
        parsed_count: 0,
        identity: None,
        window_from: None,
        window_to: None,
        watermark: Some(T0.into()),
        progress_at: None,
        thread_reads: Vec::new(),
        overlap_seconds: 300,
        snapshot: false,
        error_code: None,
        objects: Vec::new(),
        events: Vec::new(),
    }
}

pub fn ingest(db: &Database, result: SourceResult, now: &str) -> IngestReport {
    Ledger::new(db).ingest(result, &IngestOptions::at(at(now))).unwrap()
}

/// 첫 정상 수집을 끝내 이후 사건이 baseline이 되지 않게 한다.
pub fn prime_at(db: &Database, source: SourceName, query: &str, when: &str) {
    let first = SourceResult {
        source,
        query: query.into(),
        started_at: when.into(),
        finished_at: when.into(),
        watermark: Some(when.into()),
        ..result()
    };
    ingest(db, first, when);
}

pub fn prime(db: &Database, source: SourceName, query: &str) {
    prime_at(db, source, query, BEFORE);
}

pub fn prime_slack(db: &Database) {
    prime(db, SourceName::Slack, "tick");
}

/// `push`의 인자.
pub struct Push {
    run: SourceResult,
}

impl Push {
    pub fn from(source: SourceName, query: &str, when: &str) -> Self {
        Self {
            run: SourceResult {
                source,
                query: query.into(),
                started_at: when.into(),
                finished_at: when.into(),
                watermark: Some(when.into()),
                ..result()
            },
        }
    }

    pub fn events(mut self, events: Vec<NormalizedEvent>) -> Self {
        self.run.events = events;
        self
    }

    pub fn objects(mut self, objects: Vec<NormalizedObject>) -> Self {
        self.run.objects = objects;
        self
    }

    pub fn snapshot(mut self) -> Self {
        self.run.snapshot = true;
        self
    }

    pub fn partial(mut self) -> Self {
        self.run.outcome = Outcome::Partial;
        self.run.watermark = None;
        self
    }

    pub fn run(self, db: &Database) -> IngestReport {
        let now = self.run.started_at.clone();
        ingest(db, self.run, &now)
    }
}

pub fn slack(when: &str) -> Push {
    Push::from(SourceName::Slack, "tick", when)
}

pub fn github(when: &str) -> Push {
    Push::from(SourceName::Github, "tick", when)
}

pub fn calendar(query: &str, when: &str) -> Push {
    Push::from(SourceName::Calendar, query, when)
}

pub fn mention_in(ts: &str, occurred_at: &str, channel: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("U0SYNTH2".into()),
        thread_key: Some(format!("slack:{channel}:{ts}")),
        mentions_me: Some(true),
        mention_count: Some(1),
        channel_id: Some(channel.into()),
        permalink: Some(format!("https://synthetic.example/archives/{channel}/p{ts}")),
        text_head: Some(format!("합성 멘션 {ts}")),
        ..NormalizedEvent::new(
            format!("slack:{channel}:{ts}"),
            SourceName::Slack,
            EventType::SlackMention,
            occurred_at,
            ActorKind::Human,
        )
    }
}

pub fn mention(ts: &str, occurred_at: &str) -> NormalizedEvent {
    mention_in(ts, occurred_at, CHANNEL)
}

pub fn my_message(ts: &str, occurred_at: &str, thread_key: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("U0SYNTHME".into()),
        thread_key: Some(thread_key.into()),
        channel_id: Some(CHANNEL.into()),
        ..NormalizedEvent::new(
            format!("slack:{CHANNEL}:{ts}"),
            SourceName::Slack,
            EventType::SlackMessage,
            occurred_at,
            ActorKind::SelfActor,
        )
    }
}

pub fn issue(number: i64) -> NormalizedObject {
    NormalizedObject {
        text_head: Some(format!("합성 이슈 {number}")),
        state: Some("open".into()),
        assigned_to_me: Some(true),
        authored_by_me: Some(false),
        ..NormalizedObject::new(
            format!("gh:acme/widget#{number}"),
            SourceName::Github,
            ObjectKind::Issue,
            number.to_string(),
        )
    }
}

pub fn pull_request(number: i64) -> NormalizedObject {
    NormalizedObject {
        kind: ObjectKind::PullRequest,
        assigned_to_me: Some(false),
        authored_by_me: Some(true),
        ..issue(number)
    }
}

pub fn comment(id: &str, occurred_at: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("synthetic-teammate".into()),
        object_key: Some("gh:acme/widget#1".into()),
        ..NormalizedEvent::new(
            format!("gh:comment:{id}"),
            SourceName::Github,
            EventType::GhComment,
            occurred_at,
            ActorKind::Human,
        )
    }
}

pub fn meeting(id: &str) -> NormalizedObject {
    NormalizedObject {
        text_head: Some(format!("합성 회의 {id}")),
        all_day: Some(false),
        start_at: Some("2026-03-03T10:00:00+09:00".into()),
        end_at: Some("2026-03-03T11:00:00+09:00".into()),
        state: Some("confirmed".into()),
        my_response: Some("accepted".into()),
        busy: Some(true),
        ..NormalizedObject::new(format!("gcal:{id}"), SourceName::Calendar, ObjectKind::Meeting, id)
    }
}

/// 그 시간대의 회의.
pub fn meeting_at(id: &str, start: &str, end: &str) -> NormalizedObject {
    NormalizedObject { start_at: Some(start.into()), end_at: Some(end.into()), ..meeting(id) }
}

pub fn deploy(id: &str) -> NormalizedObject {
    NormalizedObject {
        text_head: Some("[배포] 합성 기능 고도화".into()),
        all_day: Some(true),
        start_date: Some("2026-03-04".into()),
        end_date: Some("2026-03-05".into()),
        state: Some("confirmed".into()),
        ..NormalizedObject::new(format!("gcal:{id}"), SourceName::Calendar, ObjectKind::Deploy, id)
    }
}

pub fn count(db: &Database, table: &str) -> i64 {
    db.connection().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

pub fn item(db: &Database, id: i64) -> Item {
    ItemRepo::new(db.connection()).get(id).unwrap().unwrap_or_else(|| panic!("항목 없음: {id}"))
}

pub fn items(db: &Database) -> Vec<Item> {
    let mut statement = db.connection().prepare("SELECT id FROM items ORDER BY id").unwrap();
    let ids: Vec<i64> = statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    ids.into_iter().map(|id| item(db, id)).collect()
}

/// 이벤트의 열 하나.
pub fn event_text(db: &Database, key: &str, column: &str) -> Option<String> {
    db.connection()
        .query_row(&format!("SELECT {column} FROM events WHERE dedupe_key = ?"), params![key], |row| row.get(0))
        .unwrap()
}

pub fn event_flag(db: &Database, key: &str, column: &str) -> i64 {
    db.connection()
        .query_row(&format!("SELECT {column} FROM events WHERE dedupe_key = ?"), params![key], |row| row.get(0))
        .unwrap()
}

pub fn refresh_with(db: &Database, now: &str, off_dates: &OffDates, ok_sources: &[&str]) -> EngineReport {
    let ok_sources: BTreeSet<String> = ok_sources.iter().map(|source| (*source).to_owned()).collect();
    let input = RefreshInput { now: at(now), off_dates, ok_sources: &ok_sources, cards_held: false };
    ItemEngine::new(db).refresh(&input).unwrap()
}

pub fn refresh(db: &Database, now: &str) -> EngineReport {
    refresh_with(db, now, &none(), &[])
}

pub fn actions(db: &Database) -> ItemActions<'_> {
    ItemActions::new(db)
}

/// 표시를 켠 제어 값.
pub fn control_on() -> KairosControl {
    KairosControl::default().with_display_mode(DisplayMode::On)
}

/// 알림 기록기.
/// 본문은 `messages`, 제목과 항목까지는 `alerts`에 쌓는다.
pub struct Recorder {
    pub messages: Mutex<Vec<String>>,
    pub alerts: Mutex<Vec<Alert>>,
    delivered: bool,
}

impl Recorder {
    pub fn new() -> Self {
        Self { messages: Mutex::new(Vec::new()), alerts: Mutex::new(Vec::new()), delivered: true }
    }

    pub fn failing() -> Self {
        Self { messages: Mutex::new(Vec::new()), alerts: Mutex::new(Vec::new()), delivered: false }
    }

    pub fn messages(&self) -> Vec<String> {
        self.messages.lock().unwrap().clone()
    }

    pub fn alerts(&self) -> Vec<Alert> {
        self.alerts.lock().unwrap().clone()
    }
}

impl Notifier for Recorder {
    fn notify(&self, alert: &Alert) -> bool {
        self.messages.lock().unwrap().push(alert.body.clone());
        self.alerts.lock().unwrap().push(alert.clone());
        self.delivered
    }
}

pub const ME: &str = "U0SYNTH0001";
pub const SELF_EMAIL: &str = "kairos-dev@contoso.example";
pub const END_OF_RESULTS: &str = "End of results - No more pages available.\\n";

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

pub fn slack_body(results: &str) -> Value {
    json!({ "results": results, "pagination_info": END_OF_RESULTS })
}
