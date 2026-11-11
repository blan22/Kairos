#![allow(clippy::unwrap_used)]

use std::process::Command;

use kairos_core::KairosHome;
use kairos_store::rusqlite::Connection;
use kairos_store::{Database, MIGRATIONS, SCHEMA_VERSION, params};

const LIVE_ITEM: &str =
    "INSERT INTO items (object_key, reason, tier, state, opened_at) VALUES ('k', 'assigned', 'P2', 'open', 'a')";
const CHILD_HOME: &str = "KAIROS_TEST_MIGRATION_HOME";

fn temp_home() -> (tempfile::TempDir, KairosHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at(dir.path().join("home"));
    (dir, home)
}

fn test_db() -> Database {
    Database::in_memory().unwrap()
}

fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

fn user_version(db: &Connection) -> i64 {
    db.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap()
}

fn fails_with(db: &Connection, text: &str, expected: &str) {
    let error = db.execute_batch(text).unwrap_err().to_string();
    assert!(error.contains(expected), "{text}: {error}");
}

/// v2까지만 적용한 합성 DB를 만든다. 운영 DB는 건드리지 않는다.
fn legacy_db(home: &KairosHome) {
    drop(Database::open(home).unwrap());
    let raw = Connection::open(home.db_path()).unwrap();
    raw.execute_batch(
        "DROP TABLE outsider_marks; DROP TABLE tick_clock; DROP TABLE runs; DROP TABLE suggestions; DROP TABLE items; DROP TABLE thread_read;
         DROP TABLE events; DROP TABLE objects; DROP TABLE cursor; DROP TABLE source_run;
         PRAGMA user_version = 0",
    )
    .unwrap();
    for migration in &MIGRATIONS[..2] {
        raw.execute_batch(migration).unwrap();
    }
    raw.execute_batch(
        "PRAGMA user_version = 2;
         INSERT INTO source_run (source, query, started_at, finished_at, outcome) VALUES ('github', 'tick', 'a', 'b', 'ok');
         INSERT INTO cursor (source, query, t0) VALUES ('github', 'tick', '2026-03-01T01:00:00.000Z');
         INSERT INTO objects (object_key, source, query, kind, external_id, snapshot_hash, first_seen_at, last_seen_run, baseline)
           VALUES ('gh:acme/widget#1', 'github', 'tick', 'issue', '1', 'h', 'a', 1, 1);
         INSERT INTO events (dedupe_key, source, thread_key, type, actor_kind, occurred_at, first_seen_at, run_id, disposition, rule_id)
           VALUES ('gh:comment:1', 'github', 'th', 'gh_comment', 'human', 'a', 'a', 1, 'opened', 'p2.owned_comment');
         INSERT INTO thread_read (source, query, external_id, updated_at) VALUES ('notion', 'mail', 't1', 'a');",
    )
    .unwrap();
}

#[test]
fn v2_db는_기존_행을_그대로_두고_최신_버전으로_올라간다() {
    let (_dir, home) = temp_home();
    legacy_db(&home);
    let db = Database::open(&home).unwrap();
    assert_eq!(SCHEMA_VERSION, 8);
    assert_eq!(user_version(db.connection()), 8);
    for table in ["source_run", "cursor", "objects", "events", "thread_read"] {
        assert_eq!(count(db.connection(), table), 1, "{table}");
    }
    for table in ["items", "suggestions", "runs"] {
        assert_eq!(count(db.connection(), table), 0, "{table}");
    }
    let event: (String, Option<i64>) = db
        .connection()
        .query_row("SELECT rule_id, item_id FROM events", [], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap();
    assert_eq!(event, ("p2.owned_comment".to_owned(), None));
}

/// 아래 시험의 자식 프로세스 본체. 환경 변수가 없으면 아무것도 하지 않는다.
#[test]
#[ignore = "여러_프로세스가_동시에_v2_db를_열어도... 시험이 자식 프로세스로 부른다"]
fn 자식_프로세스가_db를_연다() {
    if let Ok(path) = std::env::var(CHILD_HOME) {
        drop(Database::open(&KairosHome::at(path)).unwrap());
    }
}

#[test]
fn 여러_프로세스가_동시에_v2_db를_열어도_이전은_한_번만_되고_아무도_실패하지_않는다() {
    let (_dir, home) = temp_home();
    legacy_db(&home);
    let binary = std::env::current_exe().unwrap();
    let children: Vec<_> = (0..6)
        .map(|_| {
            Command::new(&binary)
                .args(["--exact", "자식_프로세스가_db를_연다", "--ignored"])
                .env(CHILD_HOME, home.root())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{stdout}");
        assert!(stdout.contains("1 passed"), "자식이 시험을 돌리지 않았다: {stdout}");
    }
    assert_eq!(user_version(&Connection::open(home.db_path()).unwrap()), 8, "자식 프로세스가 올렸다");
    let db = Database::open(&home).unwrap();
    assert_eq!(user_version(db.connection()), 8);
    assert_eq!(count(db.connection(), "events"), 1);
}

#[test]
fn v3_db의_run은_그대로_두고_수락_열만_더한다() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    let raw = Connection::open(home.db_path()).unwrap();
    for migration in &MIGRATIONS[..3] {
        raw.execute_batch(migration).unwrap();
    }
    raw.execute_batch(
        "PRAGMA user_version = 3;
         INSERT INTO items (id, object_key, reason, tier, state, opened_at) VALUES (1, 'k', 'assigned', 'P2', 'accepted', 'a');
         INSERT INTO runs (id, item_id, state, playbook, created_at) VALUES ('r', 1, 'claimed', 'qa_issue', 'a');",
    )
    .unwrap();
    drop(raw);
    let db = Database::open(&home).unwrap();
    let row: (String, i64, Option<String>, Option<String>) = db
        .connection()
        .query_row("SELECT state, adopted, launched_at, ready_at FROM runs", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    assert_eq!(row, ("claimed".to_owned(), 0, None, None));
    assert!(db.connection().execute_batch("UPDATE runs SET ready_kind = 'nope'").is_err());
}

#[test]
fn 열린_항목은_object_key와_reason당_하나이고_닫히면_새_행이_된다() {
    let db = test_db();
    db.connection().execute_batch(LIVE_ITEM).unwrap();
    fails_with(db.connection(), LIVE_ITEM, "UNIQUE");
    db.connection().execute_batch(
        "INSERT INTO items (object_key, reason, tier, state, opened_at) VALUES ('k', 'reply_owed', 'P2', 'open', 'a')",
    )
    .unwrap();
    db.connection()
        .execute_batch(
            "UPDATE items SET state = 'closed', closed_at = 'b', close_reason = 'user_done' WHERE reason = 'assigned'",
        )
        .unwrap();
    db.connection().execute_batch(LIVE_ITEM).unwrap();
    assert_eq!(count(db.connection(), "items"), 3);
}

#[test]
fn 한_항목의_활성_세션은_하나다() {
    let db = test_db();
    db.connection().execute_batch(LIVE_ITEM).unwrap();
    let run = |id: &str, state: &str, ended: Option<&str>| {
        db.connection().execute(
            "INSERT INTO runs (id, item_id, state, playbook, created_at, ended_at) VALUES (?, 1, ?, 'qa_issue', 'a', ?)",
            params![id, state, ended],
        )
    };
    run("00000000-0000-4000-8000-000000000001", "claimed", None).unwrap();
    for state in ["claimed", "started", "waiting_user", "needs_attention"] {
        let error = run("00000000-0000-4000-8000-000000000002", state, None).unwrap_err().to_string();
        assert!(error.contains("UNIQUE"), "{state}: {error}");
    }
    db.connection().execute_batch("UPDATE runs SET state = 'ended', ended_at = 'b'").unwrap();
    run("00000000-0000-4000-8000-000000000003", "claimed", None).unwrap();
    assert_eq!(count(db.connection(), "runs"), 2);
}

#[test]
fn check_근거_없는_종료_상태_불일치_잘못된_값을_막는다() {
    let db = test_db();
    let insert = |columns: &str, values: &str| {
        fails_with(
            db.connection(),
            &format!(
                "INSERT INTO items (object_key, reason, tier, opened_at, {columns}) VALUES ('c', 'assigned', 'P2', 'a', {values})"
            ),
            "CHECK",
        );
    };
    insert("state, closed_at, close_reason", "'closed', 'b', 'done_evidence'");
    insert("state, closed_at, close_reason", "'closed', 'b', 'withdrawn'");
    insert("state, closed_at, close_reason, close_evidence_event", "'closed', 'b', 'absent', 'e'");
    insert("state, close_reason", "'closed', 'user_done'");
    insert("state, closed_at", "'closed', 'b'");
    insert("state, closed_at, close_reason", "'open', 'b', 'user_done'");
    insert("state", "'parked'");
    insert("state, park_trigger", "'open', 'next_bundle'");
    insert("state", "'later'");
    insert("baseline", "2");
    insert("push_count", "-1");
    insert("title", "'외부 문자열'"); // 제목은 manual만 저장한다
    fails_with(
        db.connection(),
        "INSERT INTO items (object_key, reason, tier, opened_at) VALUES ('c', 'nope', 'P2', 'a')",
        "CHECK",
    );
    fails_with(
        db.connection(),
        "INSERT INTO items (object_key, reason, tier, opened_at) VALUES ('c', 'assigned', 'P3', 'a')",
        "CHECK",
    );
}

#[test]
fn check_p0는_규칙이_만든_회의나_배포_항목만_될_수_있다() {
    let db = test_db();
    let p0 = |reason: &str, event: &str| {
        format!(
            "INSERT INTO items (object_key, reason, tier, opened_at, opened_by_event) VALUES ('p', '{reason}', 'P0', 'a', {event})"
        )
    };
    fails_with(db.connection(), &p0("meeting_new", "NULL"), "CHECK"); // 근거 이벤트 없는 P0
    fails_with(db.connection(), &p0("manual", "'e'"), "CHECK");
    fails_with(db.connection(), &p0("reply_owed", "'e'"), "CHECK");
    db.connection().execute_batch(&p0("meeting_new", "'e'")).unwrap();
    assert_eq!(count(db.connection(), "items"), 1);
}

#[test]
fn suggestions_항목당_미응답_노출은_하나이고_응답과_시각은_함께_있다() {
    let db = test_db();
    db.connection().execute_batch(LIVE_ITEM).unwrap();
    let shown = |extra: &str| {
        format!(
            "INSERT INTO suggestions (item_id, card_id, kind, tier, shown_at, surface, seen, response, response_at)
             VALUES (1, 'c', 'item', 'P1', 'a', 'panel', 1, {extra})"
        )
    };
    db.connection().execute_batch(&shown("NULL, NULL")).unwrap();
    fails_with(db.connection(), &shown("NULL, NULL"), "UNIQUE");
    fails_with(db.connection(), &shown("'later', NULL"), "CHECK");
    fails_with(db.connection(), &shown("'sure', 'b'"), "CHECK");
    db.connection().execute_batch(&shown("'later', 'b'")).unwrap();
    assert_eq!(count(db.connection(), "suggestions"), 2);
}

#[test]
fn 없는_항목을_가리키는_제안과_세션은_외래_키가_막는다() {
    let db = test_db();
    fails_with(
        db.connection(),
        "INSERT INTO suggestions (item_id, card_id, kind, tier, shown_at, surface) VALUES (99, 'c', 'item', 'P1', 'a', 'panel')",
        "FOREIGN KEY",
    );
    fails_with(
        db.connection(),
        "INSERT INTO runs (id, item_id, state, playbook, created_at) VALUES ('r', 99, 'claimed', 'qa_issue', 'a')",
        "FOREIGN KEY",
    );
}
