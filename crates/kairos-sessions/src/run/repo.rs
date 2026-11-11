use kairos_agenda::{Playbook, ReadyKind};
use kairos_core::KairosResult;
use kairos_store::row::{enum_column, sql};
use kairos_store::rusqlite::{self, Params};
use kairos_store::{Connection, OptionalExtension, Row, params};

use super::{FailCode, Run, RunState};

const ACTIVE: &str = "('claimed', 'started', 'waiting_user', 'needs_attention')";

impl Run {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let state_index = row.as_ref().column_index("state")?;
        let playbook_index = row.as_ref().column_index("playbook")?;
        let ready_index = row.as_ref().column_index("ready_kind")?;
        Ok(Self {
            id: row.get("id")?,
            item_id: row.get("item_id")?,
            state: enum_column(&row.get::<_, String>(state_index)?, state_index)?,
            worktree_identity: row.get("worktree_identity")?,
            worktree_id: row.get("worktree_id")?,
            pane_key: row.get("pane_key")?,
            terminal_handle: row.get("terminal_handle")?,
            adopted: row.get::<_, i64>("adopted")? == 1,
            playbook: enum_column(&row.get::<_, String>(playbook_index)?, playbook_index)?,
            created_at: row.get("created_at")?,
            ended_at: row.get("ended_at")?,
            launched_at: row.get("launched_at")?,
            session_started_at: row.get("session_started_at")?,
            seen_at: row.get("seen_at")?,
            ready_kind: row
                .get::<_, Option<String>>(ready_index)?
                .map(|kind| enum_column(&kind, ready_index))
                .transpose()?,
            ready_at: row.get("ready_at")?,
            ready_seen_at: row.get("ready_seen_at")?,
            fail_code: row.get("fail_code")?,
        })
    }
}

/// 이미 떠 있는 에이전트를 run으로 잇는 값.
pub(crate) struct Adoption<'a> {
    pub worktree_identity: &'a str,
    pub worktree_id: &'a str,
    pub pane_key: &'a str,
    pub terminal_handle: Option<&'a str>,
    pub seen_at: &'a str,
}

/// runs 표. 트랜잭션 안에서도 쓰이므로 `&Connection`을 쥔다. 트랜잭션은 호출자가 연다.
#[derive(Clone, Copy, Debug)]
pub struct RunRepo<'a> {
    connection: &'a Connection,
}

impl<'a> RunRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    fn one(&self, query: &str, params: impl Params) -> KairosResult<Option<Run>> {
        self.connection.query_row(query, params, Run::from_row).optional().map_err(sql)
    }

    fn run(&self, query: &str, params: impl Params) -> KairosResult<usize> {
        self.connection.execute(query, params).map_err(sql)
    }

    pub fn get(&self, id: &str) -> KairosResult<Option<Run>> {
        self.one("SELECT * FROM runs WHERE id = ?", params![id])
    }

    /// 실패한 것을 뺀 가장 최근 run. 활성 run은 유일 인덱스로 많아야 하나다.
    pub fn latest(&self, item_id: i64) -> KairosResult<Option<Run>> {
        self.one(
            "SELECT * FROM runs WHERE item_id = ? AND state != 'failed' ORDER BY created_at DESC, rowid DESC LIMIT 1",
            params![item_id],
        )
    }

    pub fn active(&self) -> KairosResult<Vec<Run>> {
        let mut statement = self
            .connection
            .prepare(&format!("SELECT * FROM runs WHERE state IN {ACTIVE} ORDER BY created_at, rowid"))
            .map_err(sql)?;
        let rows = statement.query_map([], Run::from_row).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn insert_claimed(&self, id: &str, item_id: i64, playbook: Playbook, now: &str) -> KairosResult<()> {
        self.run(
            "INSERT INTO runs (id, item_id, state, playbook, created_at) VALUES (?, ?, 'claimed', ?, ?)",
            params![id, item_id, playbook.as_str(), now],
        )?;
        Ok(())
    }

    pub(crate) fn adopt(&self, id: &str, adoption: &Adoption<'_>) -> KairosResult<()> {
        self.run(
            "UPDATE runs SET state = 'started', adopted = 1, worktree_identity = ?, worktree_id = ?, pane_key = ?,
               terminal_handle = ?, seen_at = ? WHERE id = ?",
            params![
                adoption.worktree_identity,
                adoption.worktree_id,
                adoption.pane_key,
                adoption.terminal_handle,
                adoption.seen_at,
                id
            ],
        )?;
        Ok(())
    }

    pub(crate) fn locate(&self, id: &str, identity: &str, worktree_id: &str, launched_at: &str) -> KairosResult<()> {
        self.run(
            "UPDATE runs SET worktree_identity = ?, worktree_id = ?, launched_at = ? WHERE id = ?",
            params![identity, worktree_id, launched_at, id],
        )?;
        Ok(())
    }

    pub(crate) fn attach_terminal(&self, id: &str, pane_key: Option<&str>, handle: Option<&str>) -> KairosResult<()> {
        self.run("UPDATE runs SET pane_key = ?, terminal_handle = ? WHERE id = ?", params![pane_key, handle, id])?;
        Ok(())
    }

    pub(crate) fn flag_attention(&self, id: &str) -> KairosResult<()> {
        self.run("UPDATE runs SET state = 'needs_attention' WHERE id = ?", params![id])?;
        Ok(())
    }

    pub(crate) fn clear_ready(&self, id: &str) -> KairosResult<()> {
        self.run("UPDATE runs SET ready_kind = NULL, ready_at = NULL, ready_seen_at = NULL WHERE id = ?", params![id])?;
        Ok(())
    }

    pub(crate) fn observe(&self, id: &str, state: RunState, now: &str) -> KairosResult<()> {
        self.run(
            &format!("UPDATE runs SET state = ?, seen_at = coalesce(seen_at, ?) WHERE id = ? AND state IN {ACTIVE}"),
            params![state.as_str(), now, id],
        )?;
        Ok(())
    }

    pub(crate) fn mark_session_started(&self, id: &str, now: &str) -> KairosResult<()> {
        self.run("UPDATE runs SET session_started_at = ? WHERE id = ?", params![now, id])?;
        Ok(())
    }

    /// 처음 한 번만 남긴다. 확인 필요 카드는 진짜 준비됨을 막지 않는다.
    pub(crate) fn mark_ready(&self, id: &str, kind: ReadyKind, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE runs SET ready_kind = ?, ready_at = ?, ready_seen_at = NULL
             WHERE id = ? AND (ready_at IS NULL OR (ready_kind = 'attention' AND ? != 'attention'))",
            params![kind.as_str(), now, id, kind.as_str()],
        )?;
        Ok(())
    }

    /// 바꾼 행 수를 돌려준다. 이미 내려간 run은 건드리지 않는다.
    pub(crate) fn mark_failed(&self, id: &str, now: &str) -> KairosResult<usize> {
        self.run(
            &format!("UPDATE runs SET state = 'failed', ended_at = ? WHERE id = ? AND state IN {ACTIVE}"),
            params![now, id],
        )
    }

    pub(crate) fn mark_item_failed(&self, item_id: i64, now: &str) -> KairosResult<()> {
        self.run(
            &format!("UPDATE runs SET state = 'failed', ended_at = ? WHERE item_id = ? AND state IN {ACTIVE}"),
            params![now, item_id],
        )?;
        Ok(())
    }

    pub(crate) fn record_failure(&self, id: &str, code: FailCode, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE runs SET fail_code = ?, ready_at = ?, ready_kind = NULL, ready_seen_at = NULL WHERE id = ?",
            params![code.as_str(), now, id],
        )?;
        Ok(())
    }

    /// 바꾼 행 수를 돌려준다.
    pub(crate) fn mark_ended(&self, id: &str, now: &str) -> KairosResult<usize> {
        self.run(
            &format!("UPDATE runs SET state = 'ended', ended_at = ? WHERE id = ? AND state IN {ACTIVE}"),
            params![now, id],
        )
    }
}
