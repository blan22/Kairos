//! 불변식과 커서 시험은 kairos-ledger에 있다
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use kairos_core::KairosHome;
use kairos_core::model::Outcome;
use kairos_store::{Database, SCHEMA_VERSION, params};

fn mode(path: &std::path::Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn temp_home() -> (tempfile::TempDir, KairosHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = KairosHome::at(dir.path().join("home"));
    (dir, home)
}

fn count(db: &Database, table: &str) -> i64 {
    db.connection().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

#[test]
fn 디렉터리_0700_db와_wal과_shm_0600() {
    let (_dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    db.connection().execute_batch("INSERT INTO cursor (source, query) VALUES ('slack', 'probe')").unwrap();
    assert_eq!(mode(home.root()), 0o700);
    let path = home.db_path();
    assert_eq!(mode(&path), 0o600);
    for suffix in ["-wal", "-shm"] {
        let mut side = path.clone().into_os_string();
        side.push(suffix);
        assert_eq!(mode(std::path::Path::new(&side)), 0o600, "{suffix}");
    }
}

#[test]
fn pragma_적용() {
    let (_dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    let text =
        |name: &str| -> String { db.connection().query_row(&format!("PRAGMA {name}"), [], |row| row.get(0)).unwrap() };
    let number =
        |name: &str| -> i64 { db.connection().query_row(&format!("PRAGMA {name}"), [], |row| row.get(0)).unwrap() };
    assert_eq!(text("journal_mode"), "wal");
    assert_eq!(number("secure_delete"), 1);
    assert_eq!(number("busy_timeout"), 5000);
    assert_eq!(number("synchronous"), 1);
    assert_eq!(number("user_version"), SCHEMA_VERSION as i64);
}

#[test]
fn 테이블은_단계_1의_다섯_개_단계_2의_세_개_틱_시계_외부_글_표다() {
    let db = Database::in_memory().unwrap();
    let mut statement =
        db.connection().prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").unwrap();
    let tables: Vec<String> = statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(
        tables,
        [
            "cursor",
            "events",
            "items",
            "objects",
            "outsider_marks",
            "runs",
            "source_run",
            "suggestions",
            "thread_read",
            "tick_clock"
        ]
    );
}

#[test]
fn 다시_열어도_데이터가_유지된다() {
    let (_dir, home) = temp_home();
    Database::open(&home)
        .unwrap()
        .connection()
        .execute_batch("INSERT INTO cursor (source, query) VALUES ('slack', 'probe')")
        .unwrap();
    assert_eq!(count(&Database::open(&home).unwrap(), "cursor"), 1);
}

#[test]
fn 더_새로운_스키마는_거부한다() {
    let (_dir, home) = temp_home();
    drop(Database::open(&home).unwrap());
    let raw = rusqlite::Connection::open(home.db_path()).unwrap();
    raw.execute_batch(&format!("PRAGMA user_version = {}", SCHEMA_VERSION + 1)).unwrap();
    drop(raw);
    assert_eq!(Database::open(&home).unwrap_err().code(), "schema_newer_than_build");
}

#[test]
fn 모든_outcome을_저장할_수_있고_그_밖의_값은_거부한다() {
    let db = Database::in_memory().unwrap();
    let sql =
        "INSERT INTO source_run (source, query, started_at, finished_at, outcome) VALUES ('slack', 'q', 'a', 'b', ?1)";
    for outcome in Outcome::ALL {
        db.connection().execute(sql, params![outcome.as_str()]).unwrap();
    }
    assert!(db.connection().execute(sql, params!["fine"]).unwrap_err().to_string().contains("CHECK"));
}

#[test]
fn 트랜잭션은_실패하면_되돌린다() {
    let db = Database::in_memory().unwrap();
    let result: kairos_core::KairosResult<()> = db.transaction(|connection| {
        connection.execute_batch("INSERT INTO cursor (source, query) VALUES ('slack', 'probe')").unwrap();
        Err(kairos_core::KairosError::new("synthetic"))
    });
    assert!(result.is_err());
    assert_eq!(count(&db, "cursor"), 0);
    assert!(db.connection().is_autocommit());
}

/// 원본: `SCHEMA_VERSION`과 이전 파일의 대응. 이전 원문을 바꾸면 운영 DB와 어긋난다.
#[test]
fn 이전은_여덟_개다() {
    assert_eq!(SCHEMA_VERSION, 8);
}
