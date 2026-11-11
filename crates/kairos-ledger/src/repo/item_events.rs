//! 항목 엔진과 카드가 쓰는 events 질의. 표의 소유자가 ledger라 여기에 둔다.

use kairos_core::KairosResult;
use kairos_core::model::EventType;
use kairos_policy::{CloseReason, Disposition, EvidenceEvent};
use kairos_store::row::{enum_column, scalar, sql};
use kairos_store::{OptionalExtension, params, rusqlite};

use super::EventRepo;

/// 항목에 아직 붙지 않은 이벤트. 처분은 opened나 folded다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemEvent {
    pub id: i64,
    pub dedupe_key: String,
    pub source: String,
    pub object_key: Option<String>,
    pub thread_key: String,
    pub event_type: String,
    pub rule_id: String,
    pub occurred_at: String,
    pub channel_id: Option<String>,
    pub catchup: bool,
    pub disposition: Disposition,
}

/// 항목에 붙은 opened 이벤트의 종료 기록.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkedClosure {
    pub close_reason: Option<CloseReason>,
    pub close_evidence_event: Option<String>,
}

/// 카드의 제목과 링크를 고르는 재료.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventHead {
    pub rule_id: String,
    pub text_head: Option<String>,
    pub permalink: Option<String>,
}

/// 표시가 켜져 있었다면 나갔을 이벤트.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WouldShowEvent {
    pub dedupe_key: String,
    pub source: String,
    pub event_type: String,
    pub rule_id: String,
    pub occurred_at: String,
    pub catchup: bool,
    pub permalink: Option<String>,
    pub text_head: Option<String>,
}

impl EventRepo<'_> {
    pub fn count(&self) -> KairosResult<i64> {
        scalar(self.connection, "SELECT count(*) FROM events", [])
    }

    /// 항목에 붙일 이벤트를 시각순으로 준다.
    pub fn awaiting_item(&self) -> KairosResult<Vec<ItemEvent>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, dedupe_key, source, object_key, thread_key, type, rule_id, occurred_at, channel_id, catchup, disposition FROM events
                 WHERE disposition IN ('opened', 'folded') AND baseline = 0 AND close_reason IS NULL AND item_id IS NULL
                 ORDER BY occurred_at, id",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok(ItemEvent {
                    id: row.get("id")?,
                    dedupe_key: row.get("dedupe_key")?,
                    source: row.get("source")?,
                    object_key: row.get("object_key")?,
                    thread_key: row.get("thread_key")?,
                    event_type: row.get("type")?,
                    rule_id: row.get("rule_id")?,
                    occurred_at: row.get("occurred_at")?,
                    channel_id: row.get("channel_id")?,
                    catchup: row.get::<_, i64>("catchup")? == 1,
                    disposition: enum_column(&row.get::<_, String>("disposition")?, 10)?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub fn link_item(&self, event_id: i64, item_id: i64) -> KairosResult<()> {
        self.connection
            .execute("UPDATE events SET item_id = ? WHERE id = ?", params![item_id, event_id])
            .map_err(sql)?;
        Ok(())
    }

    /// 상태 변경 이력은 events 행으로 남긴다(설계 4.1). 제목 같은 외부 문자열은 넣지 않는다.
    pub fn record_item_change(&self, item_id: i64, change: &str, unique: &str, now: &str) -> KairosResult<()> {
        self.connection
            .execute(
                "INSERT INTO events (dedupe_key, source, thread_key, type, actor_kind, occurred_at, first_seen_at, run_id, disposition, rule_id)
                 VALUES (?, 'kairos', ?, 'item_changed', 'self', ?, ?, 0, 'noise', 'noise.lifecycle')",
                params![format!("item:{item_id}:{change}:{unique}"), format!("item:{item_id}"), now, now],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// 항목에 붙은 opened 이벤트의 종료 기록을 닫힌 순서로 준다. 접힌 이벤트는 이력일 뿐 종료 판정 대상이 아니다.
    pub fn linked_closures(&self, item_id: i64) -> KairosResult<Vec<LinkedClosure>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT disposition, close_reason, close_evidence_event, closed_at FROM events
                 WHERE item_id = ? AND disposition = 'opened' ORDER BY closed_at, id",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![item_id], |row| {
                let reason = row.get::<_, Option<String>>("close_reason")?;
                Ok(LinkedClosure {
                    close_reason: reason.map(|reason| enum_column(&reason, 1)).transpose()?,
                    close_evidence_event: row.get("close_evidence_event")?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 객체의 이벤트를 시각순으로 준다. 기준선 항목의 종료 증거 후보다.
    pub fn evidence_on_object(&self, object_key: &str) -> KairosResult<Vec<EvidenceEvent>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT dedupe_key, type, actor_kind, actor_id, thread_key, occurred_at FROM events
                 WHERE object_key = ? ORDER BY occurred_at, id",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![object_key], |row| {
                let Ok(event_type) = row.get::<_, String>("type")?.parse::<EventType>() else {
                    return Ok(None);
                };
                Ok(Some(EvidenceEvent {
                    dedupe_key: row.get("dedupe_key")?,
                    event_type,
                    actor_kind: enum_column(&row.get::<_, String>("actor_kind")?, 2)?,
                    actor_id: row.get("actor_id")?,
                    thread_key: row.get("thread_key")?,
                    occurred_at: row.get("occurred_at")?,
                }))
            })
            .map_err(sql)?;
        let candidates = rows.collect::<rusqlite::Result<Vec<_>>>().map_err(sql)?;
        Ok(candidates.into_iter().flatten().collect())
    }

    /// 항목에 붙은 opened 이벤트의 제목 재료. 최신이 먼저다.
    pub fn heads_of_item(&self, item_id: i64) -> KairosResult<Vec<EventHead>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT rule_id, text_head, permalink FROM events WHERE item_id = ? AND disposition = 'opened'
                 ORDER BY occurred_at DESC, id DESC",
            )
            .map_err(sql)?;
        let rows = statement.query_map(params![item_id], event_head).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub fn head_by_key(&self, dedupe_key: &str) -> KairosResult<Option<EventHead>> {
        self.connection
            .query_row(
                "SELECT rule_id, text_head, permalink FROM events WHERE dedupe_key = ?",
                params![dedupe_key],
                event_head,
            )
            .optional()
            .map_err(sql)
    }

    /// 객체에 T2, T3 사람의 글이 있는가. 이벤트가 없는 등급은 모르는 것이라 믿지 않는다.
    /// 보존 기한이나 정비로 events에서 빠진 글은 `outsider_marks`의 표시로 본다. 지운 행이 '외부 글 없음'이 되지 않는다.
    pub fn has_outsider_on_object(&self, object_key: &str) -> KairosResult<bool> {
        self.connection
            .query_row(
                "SELECT 1 FROM outsider_marks WHERE object_key = ?1
                 UNION ALL
                 SELECT 1 FROM events WHERE object_key = ?1 AND actor_kind = 'human'
                 AND coalesce(trust_tier, '') NOT IN ('T0', 'T1') LIMIT 1",
                params![object_key],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    /// T0 이후에 열렸고 아직 닫히지 않은 이벤트.
    pub fn would_show(&self) -> KairosResult<Vec<WouldShowEvent>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT dedupe_key, source, type, rule_id, occurred_at, catchup, permalink, text_head FROM events
                 WHERE disposition = 'opened' AND baseline = 0 AND close_reason IS NULL
                   AND (item_id IS NULL OR item_id IN (SELECT id FROM items WHERE state != 'closed'))
                 ORDER BY occurred_at",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok(WouldShowEvent {
                    dedupe_key: row.get("dedupe_key")?,
                    source: row.get("source")?,
                    event_type: row.get("type")?,
                    rule_id: row.get("rule_id")?,
                    occurred_at: row.get("occurred_at")?,
                    catchup: row.get::<_, i64>("catchup")? == 1,
                    permalink: row.get("permalink")?,
                    text_head: row.get("text_head")?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// `since` 이후 처음 본 notion_unknown 건수. 알림 메일 형식 변경의 신호다.
    pub fn count_notion_unknown_since(&self, since: &str) -> KairosResult<i64> {
        scalar(
            self.connection,
            "SELECT count(*) FROM events WHERE type = 'notion_unknown' AND first_seen_at >= ?",
            params![since],
        )
    }
}

fn event_head(row: &kairos_store::Row<'_>) -> rusqlite::Result<EventHead> {
    Ok(EventHead { rule_id: row.get(0)?, text_head: row.get(1)?, permalink: row.get(2)? })
}
