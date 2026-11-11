//! 모든 고정값은 합성 데이터다
#![allow(dead_code, clippy::unwrap_used)]

use kairos_core::Instant;
use kairos_core::model::{
    ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult,
};
use kairos_ledger::{IngestOptions, IngestReport, Ledger, ObjectRepo, ObjectRow};
use kairos_policy::{Tier, tier_of};
use kairos_store::{Database, OptionalExtension, Row, params, rusqlite};

pub const T0: &str = "2026-03-02T01:00:00.000Z"; // 월요일 10:00 KST
pub const T1: &str = "2026-03-02T01:03:00.000Z";
pub const T2: &str = "2026-03-02T01:06:00.000Z";
pub const T3: &str = "2026-03-02T01:09:00.000Z";
pub const BEFORE: &str = "2026-03-01T01:00:00.000Z";

pub fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

pub fn test_db() -> Database {
    Database::in_memory().unwrap()
}

pub fn ingest(db: &Database, result: SourceResult, now: &str) -> IngestReport {
    Ledger::new(db).ingest(result, &IngestOptions::at(at(now))).unwrap()
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

/// 그 시각의 실행 결과. 시작, 끝, watermark가 모두 그 시각이다.
pub fn result_at(when: &str) -> SourceResult {
    SourceResult { started_at: when.into(), finished_at: when.into(), watermark: Some(when.into()), ..result() }
}

/// 첫 정상 수집으로 T0를 세운다. `result()`를 바탕으로 만든 값을 받는다.
pub fn seed(db: &Database, first: SourceResult) {
    ingest(db, first, T0);
}

pub fn later() -> SourceResult {
    result_at(T1)
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

/// 일정 질의의 스냅샷 결과.
pub fn calendar(when: &str, objects: Vec<NormalizedObject>) -> SourceResult {
    SourceResult { source: SourceName::Calendar, query: "personal".into(), snapshot: true, objects, ..result_at(when) }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventRow {
    pub dedupe_key: String,
    pub event_type: String,
    pub disposition: String,
    pub rule_id: String,
    pub baseline: i64,
    pub catchup: i64,
    pub text_head: Option<String>,
    pub first_seen_at: String,
    pub run_id: i64,
    pub close_reason: Option<String>,
    pub close_evidence_event: Option<String>,
}

fn event_row(row: &Row<'_>) -> rusqlite::Result<EventRow> {
    Ok(EventRow {
        dedupe_key: row.get("dedupe_key")?,
        event_type: row.get("type")?,
        disposition: row.get("disposition")?,
        rule_id: row.get("rule_id")?,
        baseline: row.get("baseline")?,
        catchup: row.get("catchup")?,
        text_head: row.get("text_head")?,
        first_seen_at: row.get("first_seen_at")?,
        run_id: row.get("run_id")?,
        close_reason: row.get("close_reason")?,
        close_evidence_event: row.get("close_evidence_event")?,
    })
}

pub fn all_events(db: &Database) -> Vec<EventRow> {
    let mut statement = db.connection().prepare("SELECT * FROM events ORDER BY occurred_at, id").unwrap();
    statement.query_map([], event_row).unwrap().map(Result::unwrap).collect()
}

pub fn event_by_key(db: &Database, key: &str) -> EventRow {
    find_event(db, key).unwrap_or_else(|| panic!("이벤트 없음: {key}"))
}

pub fn find_event(db: &Database, key: &str) -> Option<EventRow> {
    db.connection().query_row("SELECT * FROM events WHERE dedupe_key = ?", params![key], event_row).optional().unwrap()
}

pub fn count(db: &Database, table: &str) -> i64 {
    db.connection().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

pub fn object_row(db: &Database, key: &str) -> ObjectRow {
    ObjectRepo::new(db.connection()).get(key).unwrap().unwrap_or_else(|| panic!("객체 없음: {key}"))
}

/// 그 함수는 kairos-agenda로 가지만 이 시험들이 같은 질의를 기대한다
pub fn would_show_events(db: &Database) -> Vec<(String, String, Tier)> {
    let mut statement = db
        .connection()
        .prepare(
            "SELECT dedupe_key, rule_id FROM events
             WHERE disposition = 'opened' AND baseline = 0 AND close_reason IS NULL
               AND (item_id IS NULL OR item_id IN (SELECT id FROM items WHERE state != 'closed'))
             ORDER BY occurred_at",
        )
        .unwrap();
    let mut rows: Vec<(String, String, Tier)> = statement
        .query_map([], |row| {
            let rule_id: String = row.get(1)?;
            let tier = tier_of(&rule_id);
            Ok((row.get(0)?, rule_id, tier))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    rows.sort_by_key(|row| row.2);
    rows
}
