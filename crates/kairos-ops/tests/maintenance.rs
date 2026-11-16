//! 보존과 정비, Notion 보존, 읽기 기록.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use common::*;
use kairos_core::model::{
    ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult,
    ThreadReadOutcome, ThreadReadReport, TrustTier,
};
use kairos_ledger::Invariants;
use kairos_ops::{Maintenance, MaintenanceRecord, MaintenanceRepo, backup_dir, maintenance_path};
use kairos_store::{Database, rusqlite};

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn event_keys(db: &Database) -> Vec<(String, String)> {
    let mut statement = db.connection().prepare("SELECT dedupe_key, disposition FROM events ORDER BY id").unwrap();
    statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().map(Result::unwrap).collect()
}

fn opened(rows: &[(String, String)]) -> usize {
    rows.iter().filter(|(_, disposition)| disposition == "opened").count()
}

fn gmail(query_result: SourceResult) -> SourceResult {
    SourceResult { source: SourceName::Gmail, query: "recent".into(), ..query_result }
}

fn mail_object(key: &str, text: &str, actor: Option<&str>) -> NormalizedObject {
    let external = key.trim_start_matches("gmail:");
    NormalizedObject {
        text_head: Some(text.into()),
        actor_id: actor.map(str::to_owned),
        ..NormalizedObject::new(key, SourceName::Gmail, ObjectKind::MailThread, external)
    }
}

#[test]
fn 보존_정리는_30일_지난_닫힌_이벤트만_지우고_열린_항목과_종료_증거는_남긴다() {
    let test = TestHome::new();
    let db = test.open_db();
    let repo = MaintenanceRepo::new(db.connection());
    ingest(&db, SourceResult { objects: vec![issue(1)], ..result_at(BEFORE) }, BEFORE);
    let noise = NormalizedEvent {
        actor_kind: ActorKind::Bot,
        actor_id: Some("synthetic-bot[bot]".into()),
        ..comment("old-noise", "2026-01-01T01:00:00.000Z")
    };
    // 보존 기간 안에 저장된 뒤 시간이 지난 경우다. 이미 지난 사건은 ingest가 받지 않는다.
    let old = SourceResult {
        events: vec![noise, comment("old-open", "2026-01-02T01:00:00.000Z")],
        watermark: None,
        ..result()
    };
    ingest(&db, old, "2026-01-02T02:00:00.000Z");
    ingest(&db, SourceResult { events: vec![comment("new", T1)], ..result_at(T1) }, T1);
    let before = event_keys(&db);
    let deleted = repo.prune_events(at(T3)).unwrap();
    let after = event_keys(&db);
    assert_eq!((before.len() - after.len()) as i64, deleted);
    assert_eq!(opened(&after), opened(&before));
    assert!(!after.iter().any(|(key, _)| key == "gh:comment:old-noise"));
}

#[test]
fn 정비_30일_동안_보이지_않고_이벤트도_없는_객체는_제목과_함께_지운다() {
    let test = TestHome::new();
    let db = test.open_db();
    let repo = MaintenanceRepo::new(db.connection());
    let mail = mail_object("gmail:old", "인증 코드 482913", Some("sender@example.invalid"));
    let event = NormalizedEvent {
        object_key: Some("gmail:old".into()),
        ..NormalizedEvent::new("gmail:old", SourceName::Gmail, EventType::MailThread, T0, ActorKind::Bot)
    };
    ingest(&db, gmail(SourceResult { objects: vec![mail], events: vec![event], ..result() }), T0);
    ingest(&db, SourceResult { objects: vec![issue(1)], ..result() }, T0);
    let later = days_after(T0, 31);
    let later_iso = later.to_iso();
    // 스냅샷으로 계속 보이는 객체는 지우지 않는다.
    ingest(&db, SourceResult { objects: vec![issue(1)], ..result_at(&later_iso) }, &later_iso);

    assert_eq!(repo.prune_objects(later).unwrap(), 0, "이벤트가 남아 있으면 지우지 않는다");
    assert_eq!(repo.prune_events(later).unwrap(), 1);
    assert_eq!(repo.prune_objects(later).unwrap(), 1);
    let key: String = db.connection().query_row("SELECT object_key FROM objects", [], |row| row.get(0)).unwrap();
    assert_eq!(key, "gh:acme/widget#1");
    assert_eq!(count(&db, "objects"), 1);
}

#[test]
fn 정비가_외부_글_이벤트와_객체를_지워도_세션_게이트의_표시는_남는다() {
    let test = TestHome::new();
    let db = test.open_db();
    let repo = MaintenanceRepo::new(db.connection());
    // 내 것이 아닌 이슈라 댓글이 항목을 열지 않는다.
    let watched = NormalizedObject { assigned_to_me: Some(false), ..issue(1) };
    ingest(&db, SourceResult { objects: vec![watched], ..result() }, T0);
    let outsider = NormalizedEvent { trust_tier: Some(TrustTier::T3), ..comment("t3", T1) };
    ingest(&db, SourceResult { events: vec![outsider], ..result_at(T1) }, T1);
    let has_outsider = || kairos_ledger::EventRepo::new(db.connection()).has_outsider_on_object("gh:acme/widget#1");
    assert!(has_outsider().unwrap());

    let later = days_after(T1, 31);
    assert_eq!(repo.prune_events(later).unwrap(), 1, "항목을 열지 않은 외부 글은 30일 뒤 지워진다");
    repo.prune_objects(later).unwrap();
    assert_eq!((count(&db, "events"), count(&db, "objects")), (0, 0));
    assert!(has_outsider().unwrap(), "지운 행이 외부 글 없음이 되지 않는다");
    assert_eq!(count(&db, "outsider_marks"), 1);
}

type EventState = (String, Option<String>, Option<String>);

fn open_event(db: &Database) -> EventState {
    db.connection()
        .query_row(
            "SELECT disposition, close_reason, text_head FROM events WHERE dedupe_key = 'gmail:open'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
}

#[test]
fn 정비_닫히지_않은_사건은_남기되_90일이_지나면_제목과_발신자를_지운다() {
    let test = TestHome::new();
    let db = test.open_db();
    let repo = MaintenanceRepo::new(db.connection());
    let mail = mail_object("gmail:open", "합성 경보 제목", Some("alerts@sentry.io"));
    let event = NormalizedEvent {
        actor_id: Some("alerts@sentry.io".into()),
        object_key: Some("gmail:open".into()),
        text_head: Some("합성 경보 제목".into()),
        ..NormalizedEvent::new("gmail:open", SourceName::Gmail, EventType::MailThread, T1, ActorKind::Human)
    };
    ingest(&db, gmail(result()), T0);
    ingest(&db, gmail(SourceResult { objects: vec![mail], events: vec![event], ..result_at(T1) }), T1);
    assert_eq!(open_event(&db).0, "opened");

    assert_eq!(repo.prune_events(days_after(T1, 31)).unwrap(), 0);
    assert_eq!(repo.prune_objects(days_after(T1, 31)).unwrap(), 0);
    repo.scrub_stale_open(days_after(T1, 89)).unwrap();
    assert_eq!(open_event(&db).2.as_deref(), Some("합성 경보 제목"));
    Maintenance::new(db.connection(), &test.home).run_daily(days_after(T1, 91), false).unwrap();
    assert_eq!(open_event(&db), ("opened".to_owned(), None, None));
    let object: (Option<String>, Option<String>) = db
        .connection()
        .query_row("SELECT text_head, actor_id FROM objects WHERE object_key = 'gmail:open'", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(object, (None, None));
}

#[test]
fn 정비_실행_기록은_30일이_지나면_지우되_질의별_마지막_ok는_남긴다() {
    let test = TestHome::new();
    let db = test.open_db();
    let timeout = SourceResult {
        query: "a".into(),
        outcome: Outcome::Timeout,
        error_code: Some("x".into()),
        watermark: None,
        ..result_at(T1)
    };
    ingest(&db, SourceResult { query: "a".into(), ..result() }, T0);
    ingest(&db, timeout, T1);
    ingest(&db, SourceResult { query: "b".into(), ..result() }, T0);
    ingest(&db, SourceResult { query: "b".into(), ..result_at(T1) }, T1);
    assert_eq!(MaintenanceRepo::new(db.connection()).prune_runs(days_after(T1, 31)).unwrap(), 2);
    let mut statement =
        db.connection().prepare("SELECT query, outcome, finished_at FROM source_run ORDER BY query").unwrap();
    let left: Vec<(String, String, String)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        left,
        [("a".to_owned(), "ok".to_owned(), T0.to_owned()), ("b".to_owned(), "ok".to_owned(), T1.to_owned())]
    );
}

#[test]
fn 일일_정비가_객체와_실행_기록까지_정리하고_건수를_남긴다() {
    let test = TestHome::new();
    let db = test.open_db();
    let mail = mail_object("gmail:old", "인증 코드 482913", None);
    ingest(&db, gmail(SourceResult { objects: vec![mail], ..result() }), T0);
    let timeout = SourceResult { outcome: Outcome::Timeout, error_code: Some("x".into()), watermark: None, ..result() };
    ingest(&db, gmail(timeout), T0);
    let later = days_after(T0, 31);
    let later_iso = later.to_iso();
    ingest(&db, gmail(result_at(&later_iso)), &later_iso);
    let record = Maintenance::new(db.connection(), &test.home).run_daily(later, false).unwrap().unwrap();
    assert_eq!((record.objects_deleted, record.runs_deleted), (1, 2));
    assert_eq!(count(&db, "objects"), 0);
}

#[test]
fn 일일_정비_백업은_0600_7개까지_하루_한_번_불변식_기록() {
    let test = TestHome::new();
    let db = test.open_db();
    ingest(&db, SourceResult { objects: vec![issue(1)], ..result() }, T0);
    let maintenance = Maintenance::new(db.connection(), &test.home);
    let first = maintenance.run_daily(at(T1), false).unwrap().unwrap();
    let backup = first.backup_file.clone().unwrap();
    assert_eq!(maintenance.run_daily(at(T2), false).unwrap(), None);
    assert_eq!(mode(&backup_dir(&test.home)), 0o700);
    assert_eq!(mode(Path::new(&backup)), 0o600);
    assert_eq!(first.invariants.closures_without_evidence, 0);
    for day in 3..=12 {
        maintenance.run_daily(at(&format!("2026-03-{day:02}T01:00:00.000Z")), false).unwrap();
    }
    let mut files: Vec<String> = fs::read_dir(backup_dir(&test.home))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(files.len(), 7);
    assert_eq!(files.last().map(String::as_str), Some("kairos-2026-03-12.db"));
    drop(db);
    let restored = Command::new("/usr/bin/sqlite3")
        .arg(backup_dir(&test.home).join(&files[0]))
        .arg("SELECT count(*) FROM objects")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&restored.stdout).trim(), "1");
}

fn notion_ok() -> SourceResult {
    SourceResult { source: SourceName::Notion, query: "mail".into(), overlap_seconds: 0, ..result() }
}

fn notion_rows(db: &Database) -> Vec<(String, Option<String>, Option<String>)> {
    let mut statement =
        db.connection().prepare("SELECT dedupe_key, text_head, actor_id FROM events ORDER BY dedupe_key").unwrap();
    statement
        .query_map([], |row: &rusqlite::Row<'_>| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn 보존_닫히지_않은_notion_사건은_남기되_90일_뒤_제목과_행위자를_지운다() {
    let test = TestHome::new();
    let db = test.open_db();
    let key = "notion-mail:t1";
    let event = |suffix: u8, event_type: EventType, mentions: Option<bool>| NormalizedEvent {
        actor_id: Some("합성 행위자".into()),
        object_key: Some(key.into()),
        text_head: Some("합성 제목".into()),
        trust_tier: Some(TrustTier::T2),
        permalink: Some("https://mail.google.com/".into()),
        mentions_me: mentions,
        ..NormalizedEvent::new(format!("{key}:{suffix}"), SourceName::Notion, event_type, T0, ActorKind::Human)
    };
    ingest(&db, SourceResult { started_at: BEFORE.into(), finished_at: BEFORE.into(), ..notion_ok() }, BEFORE);
    let object = NormalizedObject {
        trust_tier: Some(TrustTier::T2),
        ..NormalizedObject::new(key, SourceName::Notion, ObjectKind::NotionMail, "t1")
    };
    let run = SourceResult {
        objects: vec![object],
        events: vec![event(0, EventType::NotionMention, Some(true)), event(1, EventType::NotionEdit, None)],
        ..notion_ok()
    };
    ingest(&db, run, T0);
    Maintenance::new(db.connection(), &test.home).run_daily(days_after(T0, 31), false).unwrap();
    let kept = (format!("{key}:0"), Some("합성 제목".to_owned()), Some("합성 행위자".to_owned()));
    assert_eq!(notion_rows(&db), [kept], "편집 noise만 30일에 지워진다");
    MaintenanceRepo::new(db.connection()).scrub_stale_open(days_after(T0, 91)).unwrap();
    assert_eq!(notion_rows(&db), [(format!("{key}:0"), None, None)]);
}

#[test]
fn 오래_건드리지_않은_읽기_기록은_지운다() {
    let test = TestHome::new();
    let db = test.open_db();
    let read = ThreadReadReport { external_id: "t1".into(), outcome: ThreadReadOutcome::Read, message_count: Some(1) };
    ingest(&db, SourceResult { thread_reads: vec![read], ..notion_ok() }, T0);
    let repo = MaintenanceRepo::new(db.connection());
    assert_eq!(repo.prune_thread_reads(at(T1)).unwrap(), 0);
    assert_eq!(repo.prune_thread_reads(days_after(T0, 32)).unwrap(), 1);
}

// 아래는 원본에 없는 시험이다. 파일 형식을 고정한다.

#[test]
fn 정비_기록의_키와_순서는_maintenance_json_그대로다() {
    let test = TestHome::new();
    let db = test.open_db();
    let record = Maintenance::new(db.connection(), &test.home).run_daily(at(T1), false).unwrap().unwrap();
    let backup = record.backup_file.clone().unwrap();
    let expected = format!(
        "{{\n  \"date\": \"2026-03-02\",\n  \"invariants\": {{\n    \"eventsWithoutDisposition\": 0,\n    \"closuresWithoutEvidence\": 0,\n    \"unclassified\": 0,\n    \"itemClosuresWithoutEvidence\": 0\n  }},\n  \"eventsDeleted\": 0,\n  \"objectsDeleted\": 0,\n  \"runsDeleted\": 0,\n  \"backupFile\": \"{backup}\"\n}}\n"
    );
    assert_eq!(fs::read_to_string(maintenance_path(&test.home)).unwrap(), expected);
    assert_eq!(mode(&maintenance_path(&test.home)), 0o600);
    assert_eq!(MaintenanceRecord::read(&test.home), Some(record));
}

#[test]
fn 옛_기록에_없는_건수는_0으로_읽고_깨진_기록은_없는_것으로_읽는다() {
    let test = TestHome::new();
    assert_eq!(MaintenanceRecord::read(&test.home), None);
    let path = maintenance_path(&test.home);
    fs::write(&path, r#"{"date":"2026-03-02","eventsDeleted":3}"#).unwrap();
    let record = MaintenanceRecord::read(&test.home).unwrap();
    assert_eq!(record.events_deleted, 3);
    assert_eq!((record.objects_deleted, record.runs_deleted, record.backup_file), (0, 0, None));
    assert_eq!(record.invariants, Invariants::default());
    fs::write(&path, "{깨진").unwrap();
    assert_eq!(MaintenanceRecord::read(&test.home), None);
}

#[test]
fn 같은_날이어도_force면_다시_돌고_백업은_새로_만들지_않는다() {
    let test = TestHome::new();
    let db = test.open_db();
    let maintenance = Maintenance::new(db.connection(), &test.home);
    assert!(maintenance.run_daily(at(T1), false).unwrap().unwrap().backup_file.is_some());
    let again = maintenance.run_daily(at(T2), true).unwrap().unwrap();
    assert_eq!(again.backup_file, None);
}

#[test]
fn 경로에_따옴표가_있어도_백업한다() {
    let test = TestHome::new();
    let home = kairos_core::KairosHome::at(test.join("it's"));
    let db = Database::open(&home).unwrap();
    let backup = Maintenance::new(db.connection(), &home).backup(at(T1)).unwrap().unwrap();
    assert!(backup.exists());
}
