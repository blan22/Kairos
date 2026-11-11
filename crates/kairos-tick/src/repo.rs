use kairos_core::KairosResult;
use kairos_core::model::{Outcome, SourceName};
use kairos_store::row::{enum_column, scalar, sql};
use kairos_store::{Connection, OptionalExtension, params, rusqlite};

use crate::tick::OutcomeRow;

/// 틱이 source_run과 연결을 직접 보는 곳. 쓰기는 잠자기 표시 하나뿐이다.
#[derive(Clone, Copy, Debug)]
pub struct TickRunRepo<'a> {
    connection: &'a Connection,
}

impl<'a> TickRunRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub fn last_run_id(&self) -> KairosResult<i64> {
        scalar(self.connection, "SELECT coalesce(max(id), 0) AS id FROM source_run", [])
    }

    pub fn runs_since(&self, after_id: i64) -> KairosResult<Vec<OutcomeRow>> {
        let mut statement = self
            .connection
            .prepare("SELECT source, query, outcome, error_code FROM source_run WHERE id > ? ORDER BY id")
            .map_err(sql)?;
        let rows = statement
            .query_map(params![after_id], |row| {
                Ok(OutcomeRow {
                    source: enum_column(&row.get::<_, String>(0)?, 0)?,
                    query: row.get(1)?,
                    outcome: enum_column(&row.get::<_, String>(2)?, 2)?,
                    error_code: row.get(3)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 수집 모드가 이 질의를 기록했는가.
    pub fn recorded_since(&self, source: SourceName, query: &str, after_id: i64) -> KairosResult<bool> {
        self.connection
            .query_row(
                "SELECT 1 FROM source_run WHERE source = ? AND query = ? AND id > ? ORDER BY id DESC LIMIT 1",
                params![source.as_str(), query, after_id],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    /// 이 틱의 시간 초과를 잠자기 코드로 바꾼다. outcome은 그대로다.
    pub fn mark_slept(&self, after_id: i64, code: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "UPDATE source_run SET error_code = ? WHERE id > ? AND outcome = ?",
                params![code, after_id, Outcome::Timeout.as_str()],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// 질의의 마지막 ok 실행의 신원. 바깥 None은 ok 실행이 없다는 뜻이다.
    pub fn last_ok_identity(&self, source: SourceName, query: &str) -> KairosResult<Option<Option<String>>> {
        self.connection
            .query_row(
                "SELECT identity FROM source_run WHERE source = ? AND query = ? AND outcome = 'ok' ORDER BY id DESC LIMIT 1",
                params![source.as_str(), query],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql)
    }

    /// 소스의 첫 ok 시각과 마지막으로 신원을 단언한 시각.
    pub fn identity_span(&self, source: SourceName) -> KairosResult<(Option<String>, Option<String>)> {
        self.connection
            .query_row(
                "SELECT min(finished_at) AS first, max(CASE WHEN identity IS NOT NULL THEN finished_at END) AS asserted
                 FROM source_run WHERE source = ? AND outcome = 'ok'",
                params![source.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(sql)
    }

    pub fn notion_unknown_count(&self) -> KairosResult<i64> {
        scalar(self.connection, "SELECT count(*) AS n FROM events WHERE type = 'notion_unknown'", [])
    }

    pub fn journal_mode(&self) -> KairosResult<String> {
        self.connection.query_row("PRAGMA journal_mode", [], |row| row.get(0)).map_err(sql)
    }

    /// 패닉으로 빠져나온 일이 남긴 트랜잭션을 닫는다. 열어 두면 뒤의 기록이 모두 실패한다.
    pub(crate) fn rollback_abandoned(&self) {
        if !self.connection.is_autocommit() {
            let _ = self.connection.execute_batch("ROLLBACK");
        }
    }
}
