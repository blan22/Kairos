use kairos_core::KairosResult;
use kairos_core::model::{SourceName, ThreadReadOutcome, ThreadReadReport, ThreadReadState};
use kairos_store::row::sql;
use kairos_store::{Connection, params, rusqlite};

#[derive(Clone, Copy, Debug)]
pub struct ThreadReadRepo<'a> {
    connection: &'a Connection,
}

impl<'a> ThreadReadRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub fn list(&self, source: SourceName, query: &str) -> KairosResult<Vec<ThreadReadState>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT external_id, message_count, failures, first_failed_at, quarantined_at
                 FROM thread_read WHERE source = ? AND query = ?",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![source.as_str(), query], |row| {
                Ok(ThreadReadState {
                    external_id: row.get("external_id")?,
                    message_count: row.get("message_count")?,
                    failures: row.get("failures")?,
                    first_failed_at: row.get("first_failed_at")?,
                    quarantined_at: row.get("quarantined_at")?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn record(
        &self,
        source: SourceName,
        query: &str,
        read: &ThreadReadReport,
        now: &str,
    ) -> KairosResult<()> {
        let failed = read.outcome != ThreadReadOutcome::Read;
        let quarantined = read.outcome == ThreadReadOutcome::Quarantined;
        self.connection
            .execute(
                "INSERT INTO thread_read (source, query, external_id, message_count, failures, first_failed_at, quarantined_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (source, query, external_id) DO UPDATE SET
                   message_count = coalesce(excluded.message_count, thread_read.message_count),
                   failures = CASE WHEN ?5 = 0 THEN 0 ELSE thread_read.failures + 1 END,
                   first_failed_at = CASE WHEN ?5 = 0 THEN NULL ELSE coalesce(thread_read.first_failed_at, excluded.first_failed_at) END,
                   quarantined_at = excluded.quarantined_at,
                   updated_at = excluded.updated_at",
                params![
                    source.as_str(),
                    query,
                    read.external_id,
                    read.message_count,
                    i64::from(failed),
                    failed.then_some(now),
                    quarantined.then_some(now),
                    now,
                ],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// 형식을 몰라 저장한 메일과 격리한 메일의 스레드 id와 가장 이른 시각. 시각이 없는 대상도 있다.
    pub(crate) fn notion_reread_targets(&self) -> KairosResult<Vec<(String, Option<String>)>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT external_id AS id, min(at) AS at FROM (
                   SELECT o.external_id, e.occurred_at AS at FROM events e JOIN objects o ON o.object_key = e.object_key
                   WHERE e.source = 'notion' AND e.type = 'notion_unknown'
                   UNION ALL
                   SELECT external_id, first_failed_at AS at FROM thread_read WHERE source = 'notion' AND quarantined_at IS NOT NULL
                 ) GROUP BY external_id",
            )
            .map_err(sql)?;
        let rows = statement.query_map([], |row| Ok((row.get("id")?, row.get("at")?))).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn forget_notion(&self, external_id: &str) -> KairosResult<()> {
        self.connection
            .execute("DELETE FROM thread_read WHERE source = 'notion' AND external_id = ?", params![external_id])
            .map_err(sql)?;
        Ok(())
    }
}
