//! 알림이 읽고 쓰는 items 질의.

use kairos_core::KairosResult;
use kairos_policy::ItemReason;
use kairos_store::row::sql;
use kairos_store::{params, rusqlite};

use super::ItemRepo;
use super::items::req_enum;
use crate::item::P1Notice;

const LAST_P1_RULE: &str = "(SELECT e.rule_id FROM events e WHERE e.item_id = i.id AND e.rule_id LIKE 'p1.%'
   ORDER BY e.occurred_at DESC, e.id DESC LIMIT 1) AS rule_id";
const LAST_P1_AT: &str = "(SELECT e.occurred_at FROM events e WHERE e.item_id = i.id AND e.rule_id LIKE 'p1.%'
   ORDER BY e.occurred_at DESC, e.id DESC LIMIT 1) AS occurred_at";

/// 알릴 차례를 기다리는 P1 한 건.
#[derive(Clone, Debug)]
pub(crate) struct P1Waiting {
    pub id: i64,
    pub object_key: String,
    pub source: Option<String>,
    pub reason: ItemReason,
    pub channel_id: Option<String>,
    pub p1_held_at: Option<String>,
    pub p1_notify_failed_at: Option<String>,
    pub p1_notify_attempts: i64,
    /// 항목에 붙은 마지막 P1 사건의 규칙. 배포 때 다시로 깨어난 항목처럼 사건이 없으면 None이다.
    pub rule_id: Option<String>,
    /// 마지막 P1 사건이 일어난 시각. 사건이 없으면 None이다.
    pub occurred_at: Option<String>,
}

impl ItemRepo<'_> {
    /// 아직 알리지 않은 살아 있는 P0의 (id, 사유).
    pub(crate) fn unnotified_p0(&self) -> KairosResult<Vec<(i64, String)>> {
        let mut statement = self
            .connection()
            .prepare("SELECT id, reason FROM items WHERE tier = 'P0' AND state IN ('open', 'accepted') AND p0_notified_at IS NULL ORDER BY id")
            .map_err(sql)?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn mark_p0_notified(&self, id: i64, now: &str) -> KairosResult<()> {
        self.run("UPDATE items SET p0_notified_at = ? WHERE id = ?", params![now, id])?;
        Ok(())
    }

    /// 닫힌 항목, 대기, 수락은 사용자가 이미 다룬 일이다. 기준선은 새 사건이 와서 올라오기 전까지 알리지 않는다.
    pub(crate) fn waiting_p1(&self) -> KairosResult<Vec<P1Waiting>> {
        let text = format!(
            "SELECT i.id, i.object_key, i.source, i.reason, i.channel_id, i.p1_held_at, i.p1_notify_failed_at, i.p1_notify_attempts,
               {LAST_P1_RULE}, {LAST_P1_AT} FROM items i
             WHERE i.tier = 'P1' AND i.state = 'open' AND i.baseline = 0 AND i.p1_notified_at IS NULL ORDER BY i.id"
        );
        let mut statement = self.connection().prepare(&text).map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok(P1Waiting {
                    id: row.get("id")?,
                    object_key: row.get("object_key")?,
                    source: row.get("source")?,
                    reason: req_enum(row, "reason")?,
                    channel_id: row.get("channel_id")?,
                    p1_held_at: row.get("p1_held_at")?,
                    p1_notify_failed_at: row.get("p1_notify_failed_at")?,
                    p1_notify_attempts: row.get("p1_notify_attempts")?,
                    rule_id: row.get("rule_id")?,
                    occurred_at: row.get("occurred_at")?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn settle_notice(&self, id: i64, now: &str, notice: P1Notice) -> KairosResult<()> {
        self.run(
            "UPDATE items SET p1_notified_at = ?, p1_notice = ? WHERE id = ? AND p1_notified_at IS NULL",
            params![now, notice.as_str(), id],
        )?;
        Ok(())
    }

    pub(crate) fn mark_surfaced(&self, id: i64, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE items SET first_surfaced_at = coalesce(first_surfaced_at, ?), bundle_queued = 0 WHERE id = ?",
            params![now, id],
        )?;
        Ok(())
    }

    pub(crate) fn mark_held(&self, id: i64, now: &str) -> KairosResult<()> {
        self.run("UPDATE items SET p1_held_at = ? WHERE id = ?", params![now, id])?;
        Ok(())
    }

    pub(crate) fn mark_failed(&self, id: i64, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE items SET p1_notify_failed_at = ?, p1_notify_attempts = p1_notify_attempts + 1 WHERE id = ?",
            params![now, id],
        )?;
        Ok(())
    }
}
