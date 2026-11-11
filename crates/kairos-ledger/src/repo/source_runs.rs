use kairos_core::KairosResult;
use kairos_core::model::{Outcome, SourceName};
use kairos_store::row::{enum_column, sql};
use kairos_store::{Connection, OptionalExtension, params, params_from_iter, rusqlite};

/// source_run에 넣을 한 행. 정제와 시각 검증은 끝난 값이다.
pub(crate) struct NewRun<'a> {
    pub source: SourceName,
    pub query: &'a str,
    pub started_at: &'a str,
    pub finished_at: &'a str,
    pub outcome: Outcome,
    pub pages: i64,
    pub expected_count: Option<i64>,
    pub parsed_count: i64,
    pub identity: Option<&'a str>,
    pub window_from: Option<&'a str>,
    pub window_to: Option<&'a str>,
    pub error_code: Option<&'a str>,
}

/// 질의의 마지막 실행 요약.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunSummary {
    pub outcome: Outcome,
    pub finished_at: String,
    pub error_code: Option<String>,
}

/// 질의와 코드별로 묶은 덮지 못한 구간.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageGapRow {
    pub source: SourceName,
    pub query: String,
    pub error_code: String,
    pub window_from: Option<String>,
    pub window_to: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct SourceRunRepo<'a> {
    connection: &'a Connection,
}

impl<'a> SourceRunRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 넣은 행의 id를 준다. 이 값이 run_id다.
    pub(crate) fn insert(&self, run: &NewRun<'_>) -> KairosResult<i64> {
        self.connection
            .execute(
                "INSERT INTO source_run (
                   source, query, started_at, finished_at, outcome, pages, expected_count, parsed_count,
                   identity, window_from, window_to, error_code
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    run.source.as_str(),
                    run.query,
                    run.started_at,
                    run.finished_at,
                    run.outcome.as_str(),
                    run.pages,
                    run.expected_count,
                    run.parsed_count,
                    run.identity,
                    run.window_from,
                    run.window_to,
                    run.error_code,
                ],
            )
            .map_err(sql)?;
        Ok(self.connection.last_insert_rowid())
    }

    /// 질의의 마지막 실행. `ok_only`면 마지막 ok 실행이다.
    pub fn latest(&self, source: SourceName, query: &str, ok_only: bool) -> KairosResult<Option<RunSummary>> {
        let filter = if ok_only { "AND outcome = 'ok'" } else { "" };
        let text = format!(
            "SELECT outcome, finished_at, error_code FROM source_run WHERE source = ? AND query = ? {filter}
             ORDER BY id DESC LIMIT 1"
        );
        self.connection
            .query_row(&text, params![source.as_str(), query], |row| {
                Ok(RunSummary {
                    outcome: enum_column(&row.get::<_, String>(0)?, 0)?,
                    finished_at: row.get(1)?,
                    error_code: row.get(2)?,
                })
            })
            .optional()
            .map_err(sql)
    }

    /// `since` 이후에 끝난 실행 중 `codes`로 끝난 구간. 겹쳐 읽기로 같은 건이 여러 틱에 찍히므로 질의와 코드별로 묶는다.
    pub fn coverage_gaps(&self, codes: &[&str], since: &str) -> KairosResult<Vec<CoverageGapRow>> {
        let marks = vec!["?"; codes.len()].join(", ");
        let text = format!(
            "SELECT source, query, error_code AS errorCode, min(window_from) AS windowFrom, max(window_to) AS windowTo
             FROM source_run WHERE error_code IN ({marks}) AND finished_at >= ?
             GROUP BY source, query, error_code ORDER BY source, query"
        );
        let mut statement = self.connection.prepare(&text).map_err(sql)?;
        let values = codes.iter().copied().chain(std::iter::once(since));
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok(CoverageGapRow {
                    source: enum_column(&row.get::<_, String>(0)?, 0)?,
                    query: row.get(1)?,
                    error_code: row.get(2)?,
                    window_from: row.get(3)?,
                    window_to: row.get(4)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }
}
