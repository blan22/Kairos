//! Repo가 함께 쓰는 변환 도우미.

use kairos_core::{KairosError, KairosResult};

/// rusqlite 오류를 코드로 바꾼다. Repo는 `.map_err(sql)`로 쓴다.
pub fn sql(error: rusqlite::Error) -> KairosError {
    crate::database::sql_error(error)
}

/// 표의 문자열 값을 enum으로 읽는다. CHECK가 지키므로 실패는 스키마가 어긋났다는 뜻이다.
pub fn enum_column<T: std::str::FromStr>(text: &str, column: usize) -> rusqlite::Result<T> {
    text.parse().map_err(|_| rusqlite::Error::InvalidColumnType(column, "enum".to_owned(), rusqlite::types::Type::Text))
}

/// `SELECT count(*)` 같은 단일 정수 조회.
pub fn scalar(connection: &rusqlite::Connection, sql_text: &str, params: impl rusqlite::Params) -> KairosResult<i64> {
    connection.query_row(sql_text, params, |row| row.get(0)).map_err(sql)
}
