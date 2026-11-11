use kairos_core::KairosResult;
use kairos_core::model::{CursorState, SourceName};
use kairos_store::row::sql;
use kairos_store::{Connection, OptionalExtension, params, rusqlite};

#[derive(Clone, Copy, Debug)]
pub struct CursorRepo<'a> {
    connection: &'a Connection,
}

impl<'a> CursorRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub fn get(&self, source: SourceName, query: &str) -> KairosResult<Option<CursorState>> {
        self.connection
            .query_row(
                "SELECT watermark, overlap_s, t0, first_run_at, progress_at FROM cursor WHERE source = ? AND query = ?",
                params![source.as_str(), query],
                |row| {
                    Ok(CursorState {
                        source,
                        query: query.to_owned(),
                        watermark: row.get("watermark")?,
                        overlap_seconds: row.get("overlap_s")?,
                        t0: row.get("t0")?,
                        first_run_at: row.get("first_run_at")?,
                        progress_at: row.get("progress_at")?,
                    })
                },
            )
            .optional()
            .map_err(sql)
    }

    /// t0가 선 질의들의 (소스, t0). 표시 시작 시점의 재료다.
    pub fn established_t0s(&self) -> KairosResult<Vec<(String, String)>> {
        let mut statement =
            self.connection.prepare("SELECT source, t0 FROM cursor WHERE t0 IS NOT NULL").map_err(sql)?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 이 소스의 어떤 질의든 ok를 낸 적이 있다.
    pub(crate) fn has_ok_run(&self, source: SourceName) -> KairosResult<bool> {
        self.connection
            .query_row("SELECT 1 FROM cursor WHERE source = ? AND t0 IS NOT NULL", params![source.as_str()], |_| Ok(()))
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    pub(crate) fn mark_first_run(&self, source: SourceName, query: &str, overlap: i64, at: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "INSERT INTO cursor (source, query, overlap_s, first_run_at) VALUES (?, ?, ?, ?)
                 ON CONFLICT (source, query) DO UPDATE SET first_run_at = coalesce(cursor.first_run_at, excluded.first_run_at)",
                params![source.as_str(), query, overlap, at],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// ok 실행의 기록. t0는 처음 한 번만 세우고 진행 표시를 지운다.
    pub(crate) fn settle(&self, source: SourceName, query: &str, overlap: i64, t0: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "INSERT INTO cursor (source, query, overlap_s, t0) VALUES (?, ?, ?, ?)
                 ON CONFLICT (source, query) DO UPDATE SET overlap_s = excluded.overlap_s, t0 = coalesce(cursor.t0, excluded.t0), progress_at = NULL",
                params![source.as_str(), query, overlap, t0],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub(crate) fn set_progress(&self, source: SourceName, query: &str, at: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "UPDATE cursor SET progress_at = ? WHERE source = ? AND query = ?",
                params![at, source.as_str(), query],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub(crate) fn set_watermark(&self, source: SourceName, query: &str, at: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "UPDATE cursor SET watermark = ? WHERE source = ? AND query = ?",
                params![at, source.as_str(), query],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// Notion 질의의 진행 표시를 지우고 watermark를 from까지 되돌린다. 더 이른 watermark는 그대로 둔다.
    pub(crate) fn rewind_notion(&self, from: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "UPDATE cursor SET progress_at = NULL, watermark = CASE WHEN watermark > ?1 THEN ?1 ELSE watermark END
                 WHERE source = 'notion'",
                params![from],
            )
            .map_err(sql)?;
        Ok(())
    }
}
