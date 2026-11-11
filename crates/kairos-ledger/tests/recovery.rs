//! 원장 수준의 복구와 후속 읽기 시험.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{
    ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult,
    ThreadReadOutcome, ThreadReadReport, ThreadReadState,
};
use kairos_ledger::{CursorRepo, Ledger, MANUAL_SKIP, ThreadReadRepo};
use kairos_store::Database;

const UNTIL: &str = "2026-03-02T02:00:00.000Z";
const NOW: &str = "2026-03-02T03:00:00.000Z";

fn cursor(db: &Database, source: SourceName, query: &str) -> kairos_core::model::CursorState {
    CursorRepo::new(db.connection()).get(source, query).unwrap().unwrap()
}

#[test]
fn 커서를_건너뛰면_고른_시각부터_다시_읽고_건너뛴_구간을_남긴다() {
    let db = test_db();
    ingest(&db, SourceResult { source: SourceName::Slack, query: "mentions".into(), ..result_at(T3) }, T3);
    let ledger = Ledger::new(&db);
    assert!(ledger.skip_cursor(SourceName::Slack, "mentions", at(UNTIL), at(NOW)).unwrap());

    let moved = cursor(&db, SourceName::Slack, "mentions");
    assert_eq!(moved.window_start().unwrap().to_iso(), UNTIL);
    let row: (String, String, String, String, String) = db
        .connection()
        .query_row(
            "SELECT outcome, error_code, window_from, window_to, started_at FROM source_run ORDER BY id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(row, ("partial".into(), MANUAL_SKIP.into(), T3.into(), UNTIL.into(), NOW.into()));

    assert!(!ledger.skip_cursor(SourceName::Slack, "mentions", at(T0), at(NOW)).unwrap(), "뒤로는 옮기지 않는다");
    assert!(
        !ledger.skip_cursor(SourceName::Slack, "to_me", at(UNTIL), at(NOW)).unwrap(),
        "커서가 없으면 건너뛸 구간이 없다"
    );
}

fn notion_result(when: &str, objects: Vec<NormalizedObject>, events: Vec<NormalizedEvent>) -> SourceResult {
    SourceResult { source: SourceName::Notion, query: "mail".into(), objects, events, ..result_at(when) }
}

fn notion_mail(thread: &str) -> NormalizedObject {
    NormalizedObject::new(format!("notion-mail:{thread}"), SourceName::Notion, ObjectKind::NotionMail, thread)
}

fn notion_block(thread: &str, block: i64, event_type: EventType, when: &str) -> NormalizedEvent {
    NormalizedEvent {
        object_key: Some(format!("notion-mail:{thread}")),
        ..NormalizedEvent::new(
            format!("notion-mail:{thread}:{block}"),
            SourceName::Notion,
            event_type,
            when,
            ActorKind::Human,
        )
    }
}

#[test]
fn 파서를_고친_뒤_다시_읽으면_unknown_블록만_새로_판정되고_다른_블록은_그대로다() {
    let db = test_db();
    ingest(&db, notion_result(BEFORE, vec![], vec![]), BEFORE);
    let later = UNTIL;
    let first = vec![
        notion_block("t1", 0, EventType::NotionEdit, later),
        notion_block("t1", 1, EventType::NotionUnknown, later),
    ];
    ingest(&db, notion_result(later, vec![notion_mail("t1")], first), later);
    assert_eq!(event_by_key(&db, "notion-mail:t1:1").event_type, "notion_unknown");
    assert_eq!(Ledger::new(&db).reread_notion(at(later)).unwrap(), 1);

    let fixed = vec![
        notion_block("t1", 0, EventType::NotionEdit, later),
        notion_block("t1", 1, EventType::NotionMention, later),
    ];
    ingest(&db, notion_result(later, vec![notion_mail("t1")], fixed), later);
    assert_eq!(event_by_key(&db, "notion-mail:t1:0").event_type, "notion_edit");
    let reread = event_by_key(&db, "notion-mail:t1:1");
    assert_eq!(reread.event_type, "notion_mention");
    assert_eq!(reread.rule_id, "p1.notion_mention");
    assert_eq!(count(&db, "events"), 2);
}

#[test]
fn 다시_읽기는_가장_이른_시각보다_한_시간_앞으로_물리고_읽기_기록을_지운다() {
    let db = test_db();
    let quarantined =
        ThreadReadReport { external_id: "t2".into(), outcome: ThreadReadOutcome::Quarantined, message_count: None };
    let unknown = notion_block("t1", 0, EventType::NotionUnknown, T1);
    let run =
        SourceResult { thread_reads: vec![quarantined], ..notion_result(T3, vec![notion_mail("t1")], vec![unknown]) };
    ingest(&db, run, T3);

    assert_eq!(Ledger::new(&db).reread_notion(at(NOW)).unwrap(), 2);
    assert_eq!(cursor(&db, SourceName::Notion, "mail").watermark.as_deref(), Some("2026-03-02T00:03:00.000Z"));
    assert!(ThreadReadRepo::new(db.connection()).list(SourceName::Notion, "mail").unwrap().is_empty());
}

#[test]
fn 다시_읽기는_메일_검색_창_29일_앞으로는_물리지_않는다() {
    let db = test_db();
    let old = "2026-02-01T00:00:00.000Z";
    let recent = "2026-03-05T00:00:00.000Z";
    let unknown = notion_block("t1", 0, EventType::NotionUnknown, old);
    ingest(&db, notion_result(old, vec![notion_mail("t1")], vec![unknown]), old);
    ingest(&db, notion_result(recent, vec![], vec![]), recent);
    assert_eq!(Ledger::new(&db).reread_notion(at(recent)).unwrap(), 1);
    assert_eq!(cursor(&db, SourceName::Notion, "mail").watermark.as_deref(), Some("2026-02-04T00:00:00.000Z"));
}

#[test]
fn 다시_읽을_것이_없으면_커서를_건드리지_않는다() {
    let db = test_db();
    ingest(&db, notion_result(T1, vec![], vec![]), T1);
    assert_eq!(Ledger::new(&db).reread_notion(at(NOW)).unwrap(), 0);
    assert_eq!(cursor(&db, SourceName::Notion, "mail").watermark.as_deref(), Some(T1));
}

fn read(id: &str, outcome: ThreadReadOutcome, message_count: Option<i64>) -> ThreadReadReport {
    ThreadReadReport { external_id: id.into(), outcome, message_count }
}

#[test]
fn 후속_읽기_기록은_실패를_세고_성공이_지운다() {
    let db = test_db();
    let run = |when: &str, reads: Vec<ThreadReadReport>| {
        ingest(&db, SourceResult { thread_reads: reads, ..notion_result(when, vec![], vec![]) }, when);
    };
    let list = || ThreadReadRepo::new(db.connection()).list(SourceName::Notion, "mail").unwrap();

    run(T0, vec![read("t1", ThreadReadOutcome::Read, Some(2))]);
    let state =
        |message_count, failures, first_failed_at: Option<&str>, quarantined_at: Option<&str>| ThreadReadState {
            external_id: "t1".into(),
            message_count,
            failures,
            first_failed_at: first_failed_at.map(Into::into),
            quarantined_at: quarantined_at.map(Into::into),
        };
    assert_eq!(list(), [state(Some(2), 0, None, None)]);

    run(T1, vec![read("t1", ThreadReadOutcome::Failed, None)]);
    assert_eq!(list(), [state(Some(2), 1, Some(T1), None)]);

    run(T2, vec![read("t1", ThreadReadOutcome::Quarantined, None)]);
    assert_eq!(list(), [state(Some(2), 2, Some(T1), Some(T2))]);

    run(T3, vec![read("t1", ThreadReadOutcome::Read, Some(3))]);
    assert_eq!(list(), [state(Some(3), 0, None, None)]);
}

#[test]
fn partial의_진행_표시는_앞으로만_가고_ok가_지운다() {
    let db = test_db();
    let partial = |progress: &str| SourceResult {
        outcome: Outcome::Partial,
        progress_at: Some(progress.into()),
        ..notion_result(T1, vec![], vec![])
    };
    let progress = |db: &Database| cursor(db, SourceName::Notion, "mail").progress_at;
    ingest(&db, partial("2026-03-02T10:06:00+09:00"), T1);
    assert_eq!(progress(&db).as_deref(), Some(T2));
    ingest(&db, partial(T1), T1);
    assert_eq!(progress(&db).as_deref(), Some(T2), "뒤로는 가지 않는다");
    ingest(&db, partial(T3), T1);
    assert_eq!(progress(&db).as_deref(), Some(T3));
    ingest(&db, notion_result(T3, vec![], vec![]), T3);
    assert_eq!(progress(&db), None);
}

#[test]
fn 해석할_수_없는_진행_표시는_저장을_실패시킨다() {
    let db = test_db();
    let broken = SourceResult {
        outcome: Outcome::Partial,
        progress_at: Some("언젠가".into()),
        ..notion_result(T1, vec![], vec![comment_on_notion()])
    };
    let report = ingest(&db, broken, T1);
    assert_eq!(report.error_code.as_deref(), Some("ingest_failed"));
    assert_eq!(count(&db, "events"), 0);
}

fn comment_on_notion() -> NormalizedEvent {
    notion_block("t1", 0, EventType::NotionComment, T1)
}

#[test]
fn 보존_기간이_지난_사건은_되살리지_않고_중복으로_센다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let old = "2026-01-30T00:59:59.000Z";
    let report = ingest(&db, SourceResult { events: vec![comment("old", old)], ..later() }, T1);
    assert_eq!(report.events_inserted, 0);
    assert_eq!(report.events_duplicate, 1);
    assert_eq!(find_event(&db, "gh:comment:old"), None);
}

#[test]
fn 비워진_dedupe_key라도_보존_기간이_지났으면_실패하지_않는다() {
    let db = test_db();
    seed(&db, result());
    let blank = |when: &str| NormalizedEvent { dedupe_key: "\u{7}".into(), ..comment("x", when) };
    let old = ingest(&db, SourceResult { events: vec![blank("2026-01-30T00:59:59.000Z")], ..later() }, T1);
    assert_eq!(old.outcome, Outcome::Ok);
    assert_eq!(old.events_duplicate, 1);
    let fresh = ingest(&db, SourceResult { events: vec![blank(T1)], ..later() }, T1);
    assert_eq!(fresh.error_code.as_deref(), Some("ingest_failed"));
}

#[test]
fn 이벤트의_스레드는_지정값_객체_중복_키_순으로_정한다() {
    let db = test_db();
    let threaded = NormalizedEvent { thread_key: Some("thread-x".into()), ..comment("a", T0) };
    let orphan = NormalizedEvent { object_key: None, ..comment("b", T0) };
    ingest(&db, SourceResult { events: vec![threaded, comment("c", T0), orphan], ..result() }, T0);
    let thread = |key: &str| -> String {
        db.connection()
            .query_row("SELECT thread_key FROM events WHERE dedupe_key = ?", [key], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(thread("gh:comment:a"), "thread-x");
    assert_eq!(thread("gh:comment:c"), "gh:acme/widget#1");
    assert_eq!(thread("gh:comment:b"), "gh:comment:b");
}

#[test]
fn 일정_이동의_키는_없는_시작을_null로_찍는다() {
    let db = test_db();
    let unscheduled = NormalizedObject { start_at: None, end_at: None, ..meeting("m1") };
    ingest(&db, calendar(T0, vec![unscheduled]), T0);
    ingest(&db, calendar(T1, vec![meeting("m1")]), T1);
    let keys: Vec<String> = all_events(&db).into_iter().map(|event| event.dedupe_key).collect();
    assert_eq!(keys, ["gcal:m1#created", "gcal:m1#moved:null>2026-03-03T01:00:00.000Z"]);
}
