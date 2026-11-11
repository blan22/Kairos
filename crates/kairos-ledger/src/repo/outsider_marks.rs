//! 세션 게이트의 외부 글 증거. events와 달리 보존 기한과 정비가 지우지 않는다.

use kairos_core::KairosResult;
use kairos_store::row::sql;
use kairos_store::{Connection, OptionalExtension, params};

/// 객체에서 T0, T1이 아닌 사람의 글을 한 번이라도 봤다는 표시. 글이 30일을 넘거나 events에서 지워져도 남는다.
/// 세션은 이슈의 모든 글을 읽으므로 한 번 본 외부 글은 그 객체를 계속 세션 밖에 둔다(설계 8절).
/// 객체 키와 처음 본 시각만 둔다. 작성자와 본문은 남기지 않는다.
#[derive(Clone, Copy, Debug)]
pub struct OutsiderMarkRepo<'a> {
    connection: &'a Connection,
}

impl<'a> OutsiderMarkRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 처음 본 시각만 남긴다. 이미 있으면 그대로 둔다.
    pub fn mark(&self, object_key: &str, seen_at: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "INSERT OR IGNORE INTO outsider_marks (object_key, first_seen_at) VALUES (?, ?)",
                params![object_key, seen_at],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub fn contains(&self, object_key: &str) -> KairosResult<bool> {
        self.connection
            .query_row("SELECT 1 FROM outsider_marks WHERE object_key = ?", params![object_key], |_| Ok(()))
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }
}
