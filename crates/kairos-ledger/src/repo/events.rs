use kairos_core::KairosResult;
use kairos_core::model::{ActorKind, EventType, SourceName, TrustTier};
use kairos_policy::{CloseReason, Disposition, EvidenceEvent, RuleId};
use kairos_store::row::{enum_column, scalar, sql};
use kairos_store::{Connection, OptionalExtension, params, rusqlite};

use super::required;

/// 아직 답하지 않은 opened 이벤트. 사용자가 끝이나 버리기로 닫은 항목의 이벤트는 답할 것이 남지 않았으므로 세지 않는다.
/// 이벤트에는 사용자 종료를 적을 수 없어 항목으로 판단한다. 지워진 항목의 이벤트도 같은 이유로 뺀다.
const OPEN_RULES: &str = "
  SELECT rule_id FROM events
  WHERE object_key = ?1 AND disposition = 'opened' AND baseline = 0 AND close_reason IS NULL
    AND (item_id IS NULL OR item_id IN (SELECT id FROM items WHERE state != 'closed'))
    AND occurred_at > coalesce(
      (SELECT max(occurred_at) FROM events WHERE object_key = ?1 AND actor_kind = 'self'), '')";

/// events에 넣을 한 행. 판정과 정제는 끝난 값이다.
pub(crate) struct NewEvent<'a> {
    pub dedupe_key: &'a str,
    pub source: SourceName,
    pub object_key: Option<&'a str>,
    pub thread_key: &'a str,
    pub event_type: EventType,
    pub actor_id: Option<&'a str>,
    pub actor_kind: ActorKind,
    pub mentions_me: bool,
    pub mention_count: i64,
    pub occurred_at: &'a str,
    pub first_seen_at: &'a str,
    pub run_id: i64,
    pub catchup: bool,
    pub baseline: bool,
    pub channel_id: Option<&'a str>,
    pub permalink: Option<&'a str>,
    pub text_head: Option<&'a str>,
    pub trust_tier: Option<TrustTier>,
    pub disposition: Disposition,
    pub rule_id: RuleId,
}

/// 종료를 판정할 열린 이벤트.
pub(crate) struct OpenEvent {
    pub id: i64,
    pub dedupe_key: String,
    pub object_key: Option<String>,
    pub thread_key: String,
    pub rule_id: String,
    pub occurred_at: String,
    pub actor_id: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct EventRepo<'a> {
    pub(super) connection: &'a Connection,
}

impl<'a> EventRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 이미 저장된 이벤트의 종류.
    pub(crate) fn stored_type(&self, dedupe_key: &str) -> KairosResult<Option<String>> {
        self.connection
            .query_row("SELECT type FROM events WHERE dedupe_key = ?", params![dedupe_key], |row| row.get(0))
            .optional()
            .map_err(sql)
    }

    pub(crate) fn delete(&self, dedupe_key: &str) -> KairosResult<()> {
        self.connection.execute("DELETE FROM events WHERE dedupe_key = ?", params![dedupe_key]).map_err(sql)?;
        Ok(())
    }

    pub(crate) fn open_rule_ids(&self, object_key: &str) -> KairosResult<Vec<String>> {
        let mut statement = self.connection.prepare(OPEN_RULES).map_err(sql)?;
        let rows = statement.query_map(params![object_key], |row| row.get(0)).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn insert(&self, event: &NewEvent<'_>) -> KairosResult<()> {
        self.connection
            .execute(
                "INSERT INTO events (
                   dedupe_key, source, object_key, thread_key, type, actor_id, actor_kind, mentions_me, mention_count,
                   occurred_at, first_seen_at, run_id, catchup, baseline, channel_id, permalink, text_head, trust_tier,
                   disposition, rule_id
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                params![
                    required(event.dedupe_key),
                    event.source.as_str(),
                    event.object_key,
                    event.thread_key,
                    event.event_type.as_str(),
                    event.actor_id,
                    event.actor_kind.as_str(),
                    event.mentions_me,
                    event.mention_count,
                    event.occurred_at,
                    event.first_seen_at,
                    event.run_id,
                    event.catchup,
                    event.baseline,
                    event.channel_id,
                    event.permalink,
                    event.text_head,
                    event.trust_tier.map(TrustTier::as_str),
                    event.disposition.as_str(),
                    event.rule_id.as_str(),
                ],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// 키로 저장된 이벤트의 발생 시각.
    pub(crate) fn occurred_at(&self, dedupe_key: &str) -> KairosResult<Option<String>> {
        self.connection
            .query_row("SELECT occurred_at FROM events WHERE dedupe_key = ?", params![dedupe_key], |row| row.get(0))
            .optional()
            .map_err(sql)
    }

    pub(crate) fn has_open_deploy_risk(&self, object_key: &str) -> KairosResult<bool> {
        self.connection
            .query_row(
                "SELECT 1 FROM events WHERE object_key = ? AND disposition = 'opened' AND close_reason IS NULL
                 AND type IN ('deploy_risk_d0', 'deploy_risk_d1')",
                params![object_key],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    pub(crate) fn open_events(&self, source: SourceName) -> KairosResult<Vec<OpenEvent>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, dedupe_key, object_key, thread_key, rule_id, occurred_at, actor_id FROM events
                 WHERE source = ? AND disposition = 'opened' AND close_reason IS NULL",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![source.as_str()], |row| {
                Ok(OpenEvent {
                    id: row.get("id")?,
                    dedupe_key: row.get("dedupe_key")?,
                    object_key: row.get("object_key")?,
                    thread_key: row.get("thread_key")?,
                    rule_id: row.get("rule_id")?,
                    occurred_at: row.get("occurred_at")?,
                    actor_id: row.get("actor_id")?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 같은 객체나 스레드의 다른 이벤트를 시각순으로 준다.
    /// 어떤 규칙도 증거로 삼지 않는 모르는 종류는 판정에 영향이 없어 뺀다.
    pub(crate) fn evidence_candidates(&self, opened: &OpenEvent) -> KairosResult<Vec<EvidenceEvent>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT dedupe_key, type, actor_kind, actor_id, thread_key, occurred_at FROM events
                 WHERE (object_key = ? OR thread_key = ?) AND dedupe_key != ? ORDER BY occurred_at, id",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![opened.object_key, opened.thread_key, opened.dedupe_key], |row| {
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

    pub(crate) fn close(
        &self,
        id: i64,
        reason: CloseReason,
        evidence: Option<&str>,
        closed_at: &str,
    ) -> KairosResult<()> {
        self.connection
            .execute(
                "UPDATE events SET close_reason = ?, close_evidence_event = ?, closed_at = ? WHERE id = ?",
                params![reason.as_str(), evidence, closed_at, id],
            )
            .map_err(sql)?;
        Ok(())
    }

    pub(crate) fn count_without_disposition(&self) -> KairosResult<i64> {
        scalar(self.connection, "SELECT count(*) FROM events WHERE disposition IS NULL OR rule_id IS NULL", [])
    }

    pub(crate) fn count_closures_without_evidence(&self) -> KairosResult<i64> {
        scalar(
            self.connection,
            "SELECT count(*) FROM events WHERE close_reason IN ('done_evidence', 'withdrawn')
             AND (close_evidence_event IS NULL
                  OR close_evidence_event NOT IN (SELECT dedupe_key FROM events))",
            [],
        )
    }

    pub(crate) fn count_unclassified(&self) -> KairosResult<i64> {
        scalar(self.connection, "SELECT count(*) FROM events WHERE disposition = 'unclassified'", [])
    }

    pub(crate) fn count_item_closures_without_evidence(&self) -> KairosResult<i64> {
        scalar(
            self.connection,
            "SELECT count(*) FROM items
             WHERE close_reason IN ('done_evidence', 'withdrawn')
               AND close_evidence_event NOT IN (SELECT dedupe_key FROM events)",
            [],
        )
    }
}
