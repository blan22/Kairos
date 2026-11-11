use kairos_core::{KairosResult, string_enum};
use kairos_store::row::{enum_column, scalar, sql};
use kairos_store::{Connection, OptionalExtension, params, rusqlite};
use serde::Serialize;

use crate::item::{Item, ItemState};

/// 쓰기는 다음 단계의 `RunRepo`가 맡는다. 여기는 카드를 그리는 데 필요한 조회만 있다.
const ACTIVE: &str = "('claimed', 'started', 'waiting_user', 'needs_attention')";

string_enum! {
    /// run이 패널에 낸 카드의 종류. 값은 runs.ready_kind와 같다.
    pub enum ReadyKind {
        Question => "question",
        Done => "done",
        Ended => "ended",
        Attention => "attention",
        Failed => "failed",
    }
}

/// 항목에 붙은 세션.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunView {
    pub id: String,
    pub state: String,
}

/// 이슈의 활성 run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectRun {
    pub id: String,
    pub state: String,
    pub item_id: i64,
}

/// 아직 그리지 않은 세션 카드 한 건.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadyRow {
    pub item: Item,
    pub run_id: String,
    pub ready_kind: Option<ReadyKind>,
    pub fail_code: Option<String>,
    /// 고치는 세션(qa_issue)이면 커밋을 PR로 올릴 수 있다.
    pub fixes: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ReadOnlyRunRepo<'a> {
    connection: &'a Connection,
}

fn session_cards_clause(gate_open: bool) -> String {
    // 게이트가 닫히면 실패 카드만 낸다. 사용자가 시작을 눌렀는데 아무 표시가 없으면 안 된다.
    // 카드는 항목의 마지막 run 것만 낸다. 되돌린 뒤 다시 시작하면 항목은 accepted로 돌아오지만 앞 run의 카드는 낡았다.
    if gate_open {
        format!(
            "OR (r.fail_code IS NULL AND (i.state = 'accepted' OR r.state IN {ACTIVE})
                 AND NOT EXISTS (SELECT 1 FROM runs later WHERE later.item_id = r.item_id AND later.rowid > r.rowid))"
        )
    } else {
        String::new()
    }
}

impl<'a> ReadOnlyRunRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 이 이슈의 활성 run. 세션은 이슈마다 하나다. 담당과 답장 의무가 같은 이슈에 함께 있어도 그렇다.
    pub fn active_on_object(&self, object_key: &str) -> KairosResult<Option<ObjectRun>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT r.id, r.state, r.item_id FROM runs r JOIN items i ON i.id = r.item_id
                     WHERE i.object_key = ? AND r.state IN {ACTIVE}
                     ORDER BY r.created_at DESC, r.rowid DESC LIMIT 1"
                ),
                params![object_key],
                |row| Ok(ObjectRun { id: row.get(0)?, state: row.get(1)?, item_id: row.get(2)? }),
            )
            .optional()
            .map_err(sql)
    }

    /// 이 항목이 연 세션. 끝난 세션도 항목이 아직 수락 상태면 낸다. 결과를 보러 이동할 수 있어야 한다.
    pub fn of_item(&self, item_id: i64, state: ItemState) -> KairosResult<Option<RunView>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT id, state FROM runs WHERE item_id = ?
                       AND (state IN {ACTIVE} OR (state = 'ended' AND ? = 'accepted'))
                     ORDER BY created_at DESC, rowid DESC LIMIT 1"
                ),
                params![item_id, state.as_str()],
                |row| Ok(RunView { id: row.get(0)?, state: row.get(1)? }),
            )
            .optional()
            .map_err(sql)
    }

    /// 고치는 세션(qa_issue)의 run인가.
    pub fn is_fix_run(&self, run_id: &str) -> KairosResult<bool> {
        self.connection
            .query_row("SELECT 1 FROM runs WHERE id = ? AND playbook = 'qa_issue'", params![run_id], |_| Ok(()))
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    pub fn has_live_session(&self, item_id: i64) -> KairosResult<bool> {
        self.connection
            .query_row(&format!("SELECT 1 FROM runs WHERE item_id = ? AND state IN {ACTIVE}"), params![item_id], |_| {
                Ok(())
            })
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    pub fn active_count(&self) -> KairosResult<i64> {
        scalar(self.connection, &format!("SELECT count(*) FROM runs WHERE state IN {ACTIVE}"), [])
    }

    /// `ready_rows`와 같은 조건의 건수. 실패 카드만 표시 게이트를 넘는다.
    pub fn unseen_ready_count(&self, gate_open: bool) -> KairosResult<i64> {
        let sessions = session_cards_clause(gate_open);
        scalar(
            self.connection,
            &format!(
                "SELECT count(*) FROM runs r JOIN items i ON i.id = r.item_id
                 WHERE r.ready_at IS NOT NULL AND r.ready_seen_at IS NULL
                   AND ((r.fail_code IS NOT NULL AND i.state != 'closed') {sessions})"
            ),
            [],
        )
    }

    /// 항목이 먼저 닫혀도 세션이 살아 있으면 낸다.
    pub fn ready_rows(&self, gate_open: bool) -> KairosResult<Vec<ReadyRow>> {
        let sessions = session_cards_clause(gate_open);
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT r.id AS run_id, r.ready_kind, r.fail_code, r.playbook AS run_playbook, i.* FROM runs r JOIN items i ON i.id = r.item_id
                 WHERE r.ready_at IS NOT NULL AND r.ready_seen_at IS NULL
                   AND ((r.fail_code IS NOT NULL AND i.state != 'closed') {sessions})
                 ORDER BY r.ready_at, r.rowid"
            ))
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                let kind = row.get::<_, Option<String>>("ready_kind")?;
                Ok(ReadyRow {
                    item: Item::from_row(row)?,
                    run_id: row.get("run_id")?,
                    ready_kind: kind.map(|kind| enum_column(&kind, 1)).transpose()?,
                    fail_code: row.get("fail_code")?,
                    fixes: row.get::<_, Option<String>>("run_playbook")?.as_deref() == Some("qa_issue"),
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }
}
