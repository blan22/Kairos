use std::str::FromStr;

use kairos_core::KairosResult;
use kairos_policy::ItemReason;
use kairos_store::row::{enum_column, sql};
use kairos_store::rusqlite::{self, Params};
use kairos_store::{Connection, OptionalExtension, Row, params};

use crate::item::{Item, ItemCloseReason, ItemState, ItemTier, ParkTrigger};

/// 아직 고르지 않은 기준선 객체. 같은 이유의 항목이 한 번이라도 있었으면 상태와 무관하게 다시 나오지 않는다.
const TRIAGE_CANDIDATES: &str = "
  SELECT object_key, source, reason FROM (
    SELECT object_key, source, first_seen_at,
      CASE
        WHEN kind = 'issue' AND assigned_to_me = 1 THEN 'assigned'
        WHEN kind = 'pull_request' AND review_requested_me = 1 THEN 'review_owed'
        WHEN kind = 'pull_request' AND authored_by_me = 1 THEN 'my_pr_attention'
      END AS reason
    FROM objects
    WHERE baseline = 1 AND missing_runs = 0 AND state = 'open'
  ) AS candidate
  WHERE reason IS NOT NULL
    AND NOT EXISTS (
      SELECT 1 FROM items WHERE items.object_key = candidate.object_key AND items.reason = candidate.reason
    )
  ORDER BY first_seen_at, object_key";

const TIER_SORT: &str = "CASE tier WHEN 'P0' THEN 0 WHEN 'P1' THEN 1 ELSE 2 END";

fn opt_enum<T: FromStr>(row: &Row<'_>, column: &str) -> rusqlite::Result<Option<T>> {
    let index = row.as_ref().column_index(column)?;
    row.get::<_, Option<String>>(index)?.map(|text| enum_column(&text, index)).transpose()
}

pub(super) fn req_enum<T: FromStr>(row: &Row<'_>, column: &str) -> rusqlite::Result<T> {
    let index = row.as_ref().column_index(column)?;
    enum_column(&row.get::<_, String>(index)?, index)
}

impl Item {
    /// `i.*`를 포함한 행이면 어디서나 읽는다. 다른 표의 열은 별칭을 써야 한다.
    pub(crate) fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            object_key: row.get("object_key")?,
            reason: req_enum(row, "reason")?,
            tier: req_enum(row, "tier")?,
            state: req_enum(row, "state")?,
            baseline: row.get::<_, i64>("baseline")? == 1,
            source: row.get("source")?,
            channel_id: row.get("channel_id")?,
            title: row.get("title")?,
            opened_by_event: row.get("opened_by_event")?,
            opened_at: row.get("opened_at")?,
            due_at: row.get("due_at")?,
            first_surfaced_at: row.get("first_surfaced_at")?,
            push_count: row.get("push_count")?,
            bundle_queued: row.get::<_, i64>("bundle_queued")? == 1,
            parked_until: row.get("parked_until")?,
            park_trigger: opt_enum(row, "park_trigger")?,
            p0_notified_at: row.get("p0_notified_at")?,
            p1_notified_at: row.get("p1_notified_at")?,
            p1_notify_failed_at: row.get("p1_notify_failed_at")?,
            p1_notify_attempts: row.get("p1_notify_attempts")?,
            p1_held_at: row.get("p1_held_at")?,
            p1_notice: opt_enum(row, "p1_notice")?,
            closed_at: row.get("closed_at")?,
            close_reason: opt_enum(row, "close_reason")?,
            close_evidence_event: row.get("close_evidence_event")?,
        })
    }
}

/// 이벤트로 새 항목을 여는 값.
pub(crate) struct EventItem<'a> {
    pub key: &'a str,
    pub reason: ItemReason,
    pub tier: ItemTier,
    pub source: &'a str,
    pub channel_id: Option<&'a str>,
    pub opened_by_event: &'a str,
    pub opened_at: &'a str,
    pub due_at: Option<&'a str>,
    pub bundle_queued: bool,
}

/// 이벤트가 살아 있는 항목에 모일 때 바뀌는 값.
pub(crate) struct ItemFold<'a> {
    pub id: i64,
    pub tier: ItemTier,
    pub event_key: &'a str,
    pub push_count: i64,
    pub bundle_queued: bool,
    pub state: ItemState,
    pub park_trigger: Option<ParkTrigger>,
    pub parked_until: Option<&'a str>,
    /// 이미 알린 P0를 다시 알릴 차례로 돌린다.
    pub rearm_p0: bool,
    /// 이미 알린 P1을 다시 알릴 차례로 돌린다.
    pub rearm_p1: bool,
}

/// 기준선 객체 하나.
pub(crate) struct TriageCandidate {
    pub object_key: String,
    pub source: String,
    pub reason: ItemReason,
}

#[derive(Clone, Copy, Debug)]
pub struct ItemRepo<'a> {
    connection: &'a Connection,
}

impl<'a> ItemRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub(super) const fn connection(&self) -> &'a Connection {
        self.connection
    }

    fn all(&self, text: &str, values: impl Params) -> KairosResult<Vec<Item>> {
        let mut statement = self.connection.prepare(text).map_err(sql)?;
        let rows = statement.query_map(values, Item::from_row).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(super) fn run(&self, text: &str, values: impl Params) -> KairosResult<usize> {
        self.connection.execute(text, values).map_err(sql)
    }

    pub fn get(&self, id: i64) -> KairosResult<Option<Item>> {
        self.connection
            .query_row("SELECT * FROM items WHERE id = ?", params![id], Item::from_row)
            .optional()
            .map_err(sql)
    }

    pub fn live(&self, key: &str, reason: ItemReason) -> KairosResult<Option<Item>> {
        self.connection
            .query_row(
                "SELECT * FROM items WHERE object_key = ? AND reason = ? AND state != 'closed'",
                params![key, reason.as_str()],
                Item::from_row,
            )
            .optional()
            .map_err(sql)
    }

    /// 한 건의 실패가 틱 전체의 항목 갱신을 되돌리지 않게 저장점 안에서 돌린다. 실패하면 되돌리고 거짓을 준다.
    pub(crate) fn isolated(&self, work: impl FnOnce() -> KairosResult<()>) -> KairosResult<bool> {
        self.connection.execute_batch("SAVEPOINT assign_event").map_err(sql)?;
        let succeeded = work().is_ok();
        if !succeeded {
            self.connection.execute_batch("ROLLBACK TO assign_event").map_err(sql)?;
        }
        self.connection.execute_batch("RELEASE assign_event").map_err(sql)?;
        Ok(succeeded)
    }

    /// 사용자가 이미 다룬 P1은 보류가 끝나도 울리지 않는다. P2는 나중에 P1로 오르면 새 소식이라 표시하지 않는다.
    pub fn settle_p1_notice(&self, id: i64, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE items SET p1_notified_at = ?, p1_notice = 'handled' WHERE id = ? AND tier = 'P1' AND p1_notified_at IS NULL",
            params![now, id],
        )?;
        Ok(())
    }

    pub(crate) fn close(
        &self,
        id: i64,
        reason: ItemCloseReason,
        evidence: Option<&str>,
        now: &str,
    ) -> KairosResult<()> {
        self.run(
            "UPDATE items SET state = 'closed', closed_at = ?, close_reason = ?, close_evidence_event = ?, parked_until = NULL, park_trigger = NULL
             WHERE id = ? AND state != 'closed'",
            params![now, reason.as_str(), evidence, id],
        )?;
        Ok(())
    }

    /// 대기 시각이 지난 항목을 연다. 배포 때 다시는 트리거 때 P1이다(설계 5절).
    pub(crate) fn wake(&self, now: &str) -> KairosResult<usize> {
        let deploy = self.run(
            "UPDATE items SET state = 'open', tier = 'P1', baseline = 0, parked_until = NULL, park_trigger = NULL, push_count = 0, bundle_queued = 0
             WHERE state = 'parked' AND park_trigger = 'deploy' AND parked_until <= ?",
            params![now],
        )?;
        let bundle = self.run(
            "UPDATE items SET state = 'open', parked_until = NULL, park_trigger = NULL, bundle_queued = 1
             WHERE state = 'parked' AND park_trigger = 'next_bundle' AND parked_until <= ?",
            params![now],
        )?;
        Ok(deploy + bundle)
    }

    pub(crate) fn count_push(&self, id: i64) -> KairosResult<()> {
        self.run("UPDATE items SET push_count = push_count + 1 WHERE id = ?", params![id])?;
        Ok(())
    }

    /// 다음 묶음에 싣는다.
    pub fn queue_for_bundle(&self, id: i64) -> KairosResult<()> {
        self.run("UPDATE items SET bundle_queued = 1 WHERE id = ?", params![id])?;
        Ok(())
    }

    /// 기준선 항목은 사람의 새 사건이나 배포 연결이 생기면 올라온다(설계 4.3).
    pub(crate) fn lift_baseline(&self, key: &str) -> KairosResult<usize> {
        self.run(
            "UPDATE items SET baseline = 0, state = 'open', park_trigger = NULL, parked_until = NULL, bundle_queued = 1
             WHERE object_key = ? AND baseline = 1 AND state IN ('open', 'parked') AND (park_trigger IS NULL OR park_trigger = 'triage')",
            params![key],
        )
    }

    pub(crate) fn fold(&self, fold: &ItemFold<'_>) -> KairosResult<()> {
        self.run(
            "UPDATE items SET tier = ?, baseline = 0, opened_by_event = coalesce(opened_by_event, ?), push_count = ?, bundle_queued = ?,
               state = ?, park_trigger = ?, parked_until = ?,
               p0_notified_at = CASE WHEN ? THEN NULL ELSE p0_notified_at END WHERE id = ?",
            params![
                fold.tier.as_str(),
                fold.event_key,
                fold.push_count,
                fold.bundle_queued,
                fold.state.as_str(),
                fold.park_trigger.map(ParkTrigger::as_str),
                fold.parked_until,
                fold.rearm_p0,
                fold.id
            ],
        )?;
        if fold.rearm_p1 {
            self.run(
                "UPDATE items SET p1_notified_at = NULL, p1_notice = NULL, p1_held_at = NULL, p1_notify_failed_at = NULL,
                   p1_notify_attempts = 0 WHERE id = ?",
                params![fold.id],
            )?;
        }
        Ok(())
    }

    pub(crate) fn open_from_event(&self, item: &EventItem<'_>) -> KairosResult<i64> {
        self.run(
            "INSERT INTO items (object_key, reason, tier, source, channel_id, opened_by_event, opened_at, due_at, bundle_queued)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                item.key,
                item.reason.as_str(),
                item.tier.as_str(),
                item.source,
                item.channel_id,
                item.opened_by_event,
                item.opened_at,
                item.due_at,
                item.bundle_queued
            ],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    /// 이 객체의 철회로 닫힌 회의 항목 중 가장 새 것.
    pub(crate) fn withdrawn_meeting(&self, key: &str) -> KairosResult<Option<i64>> {
        self.connection
            .query_row(
                "SELECT max(id) FROM items WHERE object_key = ? AND reason = 'meeting_new' AND close_reason = 'withdrawn'",
                params![key],
                |row| row.get(0),
            )
            .map_err(sql)
    }

    pub(crate) fn live_all(&self) -> KairosResult<Vec<Item>> {
        self.all("SELECT * FROM items WHERE state != 'closed'", [])
    }

    pub(crate) fn park_for_bundle(&self, id: i64, until: &str) -> KairosResult<()> {
        self.run(
            "UPDATE items SET state = 'parked', park_trigger = 'next_bundle', parked_until = ?, push_count = push_count + 1 WHERE id = ?",
            params![until, id],
        )?;
        Ok(())
    }

    pub(crate) fn park_for_deploy(&self, id: i64, until: &str) -> KairosResult<()> {
        self.run(
            "UPDATE items SET state = 'parked', park_trigger = 'deploy', parked_until = ? WHERE id = ?",
            params![until, id],
        )?;
        Ok(())
    }

    pub(crate) fn park_kept(&self, id: i64) -> KairosResult<()> {
        self.run(
            "UPDATE items SET state = 'parked', park_trigger = 'kept', parked_until = NULL WHERE id = ?",
            params![id],
        )?;
        Ok(())
    }

    pub(crate) fn insert_work_owed(&self, from: &Item, now: &str) -> KairosResult<()> {
        self.run(
            "INSERT INTO items (object_key, reason, tier, source, channel_id, opened_by_event, opened_at, first_surfaced_at)
             VALUES (?, 'work_owed', 'P2', ?, ?, ?, ?, ?)",
            params![from.object_key, from.source, from.channel_id, from.opened_by_event, now, now],
        )?;
        Ok(())
    }

    pub(crate) fn insert_manual(&self, object_key: &str, title: &str, now: &str) -> KairosResult<i64> {
        self.run(
            "INSERT INTO items (object_key, reason, tier, title, opened_at, first_surfaced_at) VALUES (?, 'manual', 'P2', ?, ?, ?)",
            params![object_key, title, now, now],
        )?;
        Ok(self.connection.last_insert_rowid())
    }

    pub(crate) fn accept(&self, id: i64) -> KairosResult<()> {
        self.run("UPDATE items SET state = 'accepted' WHERE id = ?", params![id])?;
        Ok(())
    }

    pub(crate) fn reopen(&self, id: i64) -> KairosResult<()> {
        self.run("UPDATE items SET state = 'open' WHERE id = ?", params![id])?;
        Ok(())
    }

    pub(crate) fn keep_triaged(&self, id: i64) -> KairosResult<()> {
        self.run("UPDATE items SET state = 'open', park_trigger = NULL WHERE id = ?", params![id])?;
        Ok(())
    }

    pub(crate) fn pending_triage(&self) -> KairosResult<Vec<Item>> {
        self.all("SELECT * FROM items WHERE state = 'parked' AND park_trigger = 'triage' ORDER BY id", [])
    }

    pub(crate) fn triage_candidates(&self) -> KairosResult<Vec<TriageCandidate>> {
        let mut statement = self.connection.prepare(TRIAGE_CANDIDATES).map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok(TriageCandidate {
                    object_key: row.get("object_key")?,
                    source: row.get("source")?,
                    reason: req_enum(row, "reason")?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn insert_triage(&self, candidate: &TriageCandidate, now: &str) -> KairosResult<()> {
        self.run(
            "INSERT INTO items (object_key, reason, tier, state, baseline, source, opened_at, park_trigger)
             VALUES (?, ?, 'P2', 'parked', 1, ?, ?, 'triage')",
            params![candidate.object_key, candidate.reason.as_str(), candidate.source, now],
        )?;
        Ok(())
    }

    /// 이 이슈의 열린 항목. 먼저 열린 것이 앞이다.
    pub(crate) fn open_on_object(&self, object_key: &str) -> KairosResult<Vec<Item>> {
        self.all("SELECT * FROM items WHERE object_key = ? AND state = 'open' ORDER BY id", params![object_key])
    }

    fn open_where(&self, condition: &str) -> KairosResult<Vec<Item>> {
        self.all(&format!("SELECT * FROM items WHERE state = 'open' AND {condition} ORDER BY opened_at, id"), [])
    }

    pub(crate) fn open_p0(&self) -> KairosResult<Vec<Item>> {
        self.open_where("tier = 'P0'")
    }

    pub(crate) fn open_p1(&self) -> KairosResult<Vec<Item>> {
        self.open_where("baseline = 0 AND tier = 'P1'")
    }

    pub(crate) fn bundle_p2(&self) -> KairosResult<Vec<Item>> {
        self.open_where("baseline = 0 AND tier = 'P2' AND (first_surfaced_at IS NULL OR bundle_queued = 1)")
    }

    /// overflow면 오늘 P1 표시를 다 써서 아직 표면에 안 오른 건도 묶음에 싣는다.
    pub(crate) fn bundle_p1(&self, overflow: bool) -> KairosResult<Vec<Item>> {
        let queued = if overflow { "(bundle_queued = 1 OR first_surfaced_at IS NULL)" } else { "bundle_queued = 1" };
        self.open_where(&format!("baseline = 0 AND tier = 'P1' AND {queued}"))
    }

    /// 패널 목록. 닫힌 항목이어도 세션이 살아 있으면 남긴다. 패널에서 그 세션으로 갈 길은 이 줄뿐이다.
    pub(crate) fn listed(&self) -> KairosResult<Vec<Item>> {
        self.all(
            &format!(
                "SELECT * FROM items
                 WHERE (state != 'closed' OR id IN (SELECT item_id FROM runs WHERE state IN ('claimed', 'started', 'waiting_user', 'needs_attention')))
                   AND coalesce(park_trigger, '') != 'triage'
                 ORDER BY baseline, {TIER_SORT}, opened_at, id"
            ),
            [],
        )
    }

    pub(crate) fn count_focus_held(&self, from: &str, to: &str) -> KairosResult<i64> {
        self.connection
            .query_row(
                "SELECT count(*) FROM items WHERE state = 'open' AND baseline = 0 AND tier = 'P1' AND opened_at >= ? AND opened_at <= ?",
                params![from, to],
                |row| row.get(0),
            )
            .map_err(sql)
    }
}
