use std::fs::OpenOptions;
use std::path::Path;
use std::time::Duration;

use kairos_core::home::FILE_MODE;
use kairos_core::platform::{CreateMode, set_mode};
use kairos_core::{KairosError, KairosHome, KairosResult};
use rusqlite::Connection;

pub const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 이전은 추가만 한다. `MIGRATIONS[i]`를 적용하면 user_version이 i+1이 된다.
/// 적용된 항목을 고치지 않고 다음 항목을 덧붙인다.
pub const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001.sql"),
    include_str!("../migrations/0002.sql"),
    include_str!("../migrations/0003.sql"),
    include_str!("../migrations/0004.sql"),
    include_str!("../migrations/0005.sql"),
    include_str!("../migrations/0006.sql"),
    include_str!("../migrations/0007.sql"),
    include_str!("../migrations/0008.sql"),
];

pub const SCHEMA_VERSION: usize = MIGRATIONS.len();

/// 열린 DB 한 벌. 권한, PRAGMA, 이전을 끝낸 상태만 존재한다.
#[derive(Debug)]
pub struct Database {
    connection: Connection,
}

impl Database {
    /// kairos.db를 열고 권한, PRAGMA, 이전을 적용한다.
    pub fn open(home: &KairosHome) -> KairosResult<Self> {
        home.ensure()?;
        let path = home.db_path();
        // SQLite는 WAL과 SHM을 DB 파일과 같은 권한으로 만든다. 그래서 DB를 먼저 0600으로 만든다.
        OpenOptions::new().append(true).create(true).create_mode(FILE_MODE).open(&path)?;
        set_mode(&path, FILE_MODE)?;

        let connection = Connection::open(&path).map_err(sql_error)?;
        connection.busy_timeout(BUSY_TIMEOUT).map_err(sql_error)?;
        // journal_mode는 결과 행을 돌려주므로 execute_batch로 버린다.
        connection
            .execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA secure_delete = ON;")
            .map_err(sql_error)?;
        let database = Self { connection };
        database.migrate()?;
        for suffix in ["-wal", "-shm"] {
            let side = side_file(&path, suffix);
            if side.exists() {
                set_mode(&side, FILE_MODE)?;
            }
        }
        Ok(database)
    }

    /// 메모리 DB. 시험 전용이다. 이전까지 끝낸 상태로 준다.
    pub fn in_memory() -> KairosResult<Self> {
        let connection = Connection::open_in_memory().map_err(sql_error)?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn user_version(&self) -> KairosResult<usize> {
        let version: i64 = self.connection.query_row("PRAGMA user_version", [], |row| row.get(0)).map_err(sql_error)?;
        Ok(version.max(0) as usize)
    }

    fn migrate(&self) -> KairosResult<()> {
        let version = self.user_version()?;
        if version > SCHEMA_VERSION {
            return Err(KairosError::new("schema_newer_than_build"));
        }
        if version == SCHEMA_VERSION {
            return Ok(());
        }
        self.transaction(|connection| {
            // 잠금을 얻은 뒤 다시 읽는다. 다른 프로세스가 먼저 올렸을 수 있다.
            let current: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0)).map_err(sql_error)?;
            for (index, migration) in MIGRATIONS.iter().enumerate().skip(current.max(0) as usize) {
                connection.execute_batch(migration).map_err(sql_error)?;
                connection.execute_batch(&format!("PRAGMA user_version = {}", index + 1)).map_err(sql_error)?;
            }
            Ok(())
        })
    }

    /// BEGIN IMMEDIATE 트랜잭션. work가 실패하면 전부 되돌린다. 중첩은 지원하지 않는다.
    pub fn transaction<T>(&self, work: impl FnOnce(&Connection) -> KairosResult<T>) -> KairosResult<T> {
        self.connection.execute_batch("BEGIN IMMEDIATE").map_err(sql_error)?;
        let outcome = work(&self.connection).and_then(|value| {
            self.connection.execute_batch("COMMIT").map_err(sql_error)?;
            Ok(value)
        });
        // COMMIT 실패도 여기로 온다. 열린 채 두면 다음 BEGIN이 실패해 실패 행조차 기록하지 못한다.
        if outcome.is_err() && !self.connection.is_autocommit() {
            self.connection.execute_batch("ROLLBACK").map_err(sql_error)?;
        }
        outcome
    }
}

fn side_file(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}

/// SQL 오류를 코드로 바꾼다. 메시지에 값이 섞일 수 있어 옮기지 않는다.
pub(crate) fn sql_error(error: rusqlite::Error) -> KairosError {
    match error {
        rusqlite::Error::SqliteFailure(failure, _) => KairosError::new(format!("sqlite_{:?}", failure.code)),
        _ => KairosError::new("sqlite_error"),
    }
}
