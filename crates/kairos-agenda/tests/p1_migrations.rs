//! 6, v7 이전 시험.
#![allow(clippy::unwrap_used)]

mod common;

use std::process::{Command, Stdio};

use common::*;
use kairos_core::KairosHome;
use kairos_core::model::SourceName;
use kairos_store::{Connection, Database, MIGRATIONS, SCHEMA_VERSION};

const CHILD_HOME: &str = "KAIROS_AGENDA_TEST_CHILD_HOME";

fn user_version(connection: &Connection) -> i64 {
    connection.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap()
}

fn raw_db(home: &KairosHome, version: usize) -> Connection {
    home.ensure().unwrap();
    let raw = Connection::open(home.db_path()).unwrap();
    // 운영 DB는 이미 WAL이다. 원본 고정값은 DELETE 모드인데, 그러면 자식들이 동시에 WAL로 바꾸다 약 4%로
    // DatabaseBusy가 난다. 이 시험이 보려는 것은 이전이 한 번만 되는가이므로 그 경합을 뺀다.
    raw.execute_batch("PRAGMA journal_mode = WAL").unwrap();
    for migration in &MIGRATIONS[..version] {
        raw.execute_batch(migration).unwrap();
    }
    raw.execute_batch(&format!("PRAGMA user_version = {version}")).unwrap();
    raw
}

/// v5까지 적용하고 그때의 행을 넣은 합성 DB.
fn v5_home(home: &KairosHome) {
    let raw = raw_db(home, 5);
    raw.execute_batch(&format!(
        "INSERT INTO events (dedupe_key, source, thread_key, type, actor_kind, occurred_at, first_seen_at, run_id, disposition, rule_id)
           VALUES ('e0', 'calendar', 'th', 'cal_created', 'human', 'a', 'a', 1, 'opened', 'p0.meeting_imminent');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at) VALUES (1, 'k1', 'reply_owed', 'P1', 'open', 'slack', 'a');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at, parked_until, park_trigger) VALUES (2, 'k2', 'reply_owed', 'P1', 'parked', 'slack', 'a', '{BEFORE}', 'next_bundle');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at) VALUES (3, 'k3', 'reply_owed', 'P1', 'accepted', 'slack', 'a');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at) VALUES (4, 'k4', 'review_owed', 'P2', 'open', 'github', 'a');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at, opened_by_event, p0_notified_at) VALUES (5, 'k5', 'meeting_new', 'P0', 'open', 'calendar', 'a', 'e0', 'p0시각');
         INSERT INTO suggestions (item_id, card_id, kind, tier, shown_at, surface) VALUES (1, 'c', 'item', 'P1', 'a', 'notification');"
    ))
    .unwrap();
}

/// v6까지 적용하고 그때의 행을 넣은 합성 DB.
fn v6_home(home: &KairosHome) {
    let raw = raw_db(home, 6);
    raw.execute_batch(&format!(
        "INSERT INTO items (id, object_key, reason, tier, state, source, opened_at, p1_notified_at) VALUES (1, 'k1', 'reply_owed', 'P1', 'open', 'slack', 'a', '알린 시각');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at) VALUES (2, 'k2', 'reply_owed', 'P1', 'open', 'slack', 'a');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at, p1_notify_failed_at) VALUES (3, 'k3', 'reply_owed', 'P1', 'open', 'slack', 'a', '{BEFORE}');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at) VALUES (4, 'k4', 'review_owed', 'P2', 'open', 'github', 'a');
         INSERT INTO items (id, object_key, reason, tier, state, source, opened_at, baseline) VALUES (5, 'k5', 'review_owed', 'P1', 'open', 'github', 'a', 1);
         INSERT INTO suggestions (item_id, card_id, kind, tier, shown_at, surface) VALUES (1, 'c', 'item', 'P1', 'a', 'osascript');"
    ))
    .unwrap();
}

/// 시험 바이너리를 자식 프로세스로 여럿 띄워 같은 DB를 동시에 연다.
fn open_from_children(home: &KairosHome) {
    let binary = std::env::current_exe().unwrap();
    let children: Vec<_> = (0..6)
        .map(|_| {
            Command::new(&binary)
                .args(["--exact", "자식_프로세스가_db를_연다", "--ignored"])
                .env(CHILD_HOME, home.root())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
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
}

#[test]
#[ignore = "여러 프로세스 시험의 자식이다"]
fn 자식_프로세스가_db를_연다() {
    if let Ok(path) = std::env::var(CHILD_HOME) {
        drop(Database::open(&KairosHome::at(path)).unwrap());
    }
}

#[test]
fn v6_이전_기존_행을_보존하고_이미_있던_p1은_울리지_않는다() {
    let (_dir, home) = temp_home();
    v5_home(&home);
    let db = Database::open(&home).unwrap();
    assert_eq!(SCHEMA_VERSION, 8);
    assert_eq!(user_version(db.connection()), 8);
    assert_eq!((count(&db, "items"), count(&db, "suggestions"), count(&db, "events")), (5, 1, 1));
    for id in [1, 2, 3] {
        let notified = item(&db, id).p1_notified_at.unwrap();
        assert!(kairos_core::Instant::parse(&notified).is_some_and(|at| at.to_iso() == notified), "{id}");
    }
    assert_eq!(item(&db, 4).p1_notified_at, None, "P2는 나중에 P1로 오르면 새 P1이다");
    assert_eq!((item(&db, 5).p0_notified_at.as_deref(), item(&db, 5).p1_notified_at), (Some("p0시각"), None));
    assert_eq!(item(&db, 1).p1_notify_failed_at, None);
    let recorder = Recorder::new();
    let when = kst(MON, "10:03");
    assert_eq!(tick(&db, &when, &recorder), report(0, 0, 0, 0));
    assert!(recorder.messages().is_empty());
    assert_eq!(kairos_agenda::ItemRepo::new(db.connection()).counts(at(&when)).unwrap().p1_held, 0);
}

#[test]
fn v6_이전_여러_프로세스가_동시에_열어도_한_번만_적용된다() {
    let (_dir, home) = temp_home();
    v5_home(&home);
    open_from_children(&home);
    let db = Database::open(&home).unwrap();
    assert_eq!(user_version(db.connection()), 8);
    assert_eq!(count(&db, "items"), 5);
    assert!(item(&db, 1).p1_notified_at.is_some());
}

#[test]
fn v7_이전_기존_행을_보존하고_v6의_보류와_실패를_잇는다() {
    let (_dir, home) = temp_home();
    v6_home(&home);
    let db = Database::open(&home).unwrap();
    assert_eq!(user_version(db.connection()), 8);
    assert_eq!((count(&db, "items"), count(&db, "suggestions"), count(&db, "tick_clock")), (5, 1, 0));
    assert_eq!(item(&db, 1).p1_notified_at.as_deref(), Some("알린 시각"));
    assert_eq!(item(&db, 1).p1_held_at, None);
    assert!(item(&db, 2).p1_held_at.is_some(), "v6에서 기다리던 건은 보류로 잇는다");
    assert_eq!((item(&db, 3).p1_held_at, item(&db, 3).p1_notify_attempts), (None, 1));
    assert_eq!((item(&db, 4).p1_held_at, item(&db, 5).p1_held_at), (None, None));
}

#[test]
fn v7_이전_v6에서_기다리던_건은_닫힌_틱을_지나도_끝날_때_알린다() {
    let (_dir, home) = temp_home();
    v6_home(&home);
    let db = Database::open(&home).unwrap();
    for source in [SourceName::Slack, SourceName::Github] {
        prime(&db, source, "tick");
    }
    let recorder = Recorder::new();
    let paused = Over::control(control_on().with_paused_until(Some("indefinite")));
    assert_eq!(tick_over(&db, &kst(MON, "11:00"), &recorder, paused).dropped, 0);
    tick(&db, &kst(MON, "11:03"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 답장 필요 2건"]);
}

#[test]
fn v7_이전_여러_프로세스가_동시에_열어도_한_번만_적용된다() {
    let (_dir, home) = temp_home();
    v6_home(&home);
    open_from_children(&home);
    let db = Database::open(&home).unwrap();
    assert_eq!(user_version(db.connection()), 8);
    assert_eq!(item(&db, 3).p1_notify_attempts, 1);
}
