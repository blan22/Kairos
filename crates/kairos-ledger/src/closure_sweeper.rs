use kairos_core::model::{Outcome, SourceName};
use kairos_core::{Instant, KairosResult};
use kairos_policy::{ClosureDecision, ClosureInput, ClosureJudge, OpenedEvent};
use kairos_store::Connection;

use crate::repo::{EventRepo, ObjectRepo, ObjectRow, OpenEvent};

/// 열린 이벤트의 종료 판정. ok 실행 안에서만 불린다. 부재가 아니라 저장된 이벤트와 시각으로만 닫는다.
pub(crate) struct ClosureSweeper<'a> {
    events: EventRepo<'a>,
    objects: ObjectRepo<'a>,
}

impl<'a> ClosureSweeper<'a> {
    pub(crate) const fn new(connection: &'a Connection) -> Self {
        Self { events: EventRepo::new(connection), objects: ObjectRepo::new(connection) }
    }

    pub(crate) fn sweep(&self, source: SourceName, now: Instant) -> KairosResult<usize> {
        let closed_at = now.to_iso();
        let mut closed = 0;
        for row in self.events.open_events(source)? {
            if let ClosureDecision::Close { reason, evidence } = self.judge(&row, now)? {
                self.events.close(row.id, reason, evidence.as_deref(), &closed_at)?;
                closed += 1;
            }
        }
        Ok(closed)
    }

    fn judge(&self, row: &OpenEvent, now: Instant) -> KairosResult<ClosureDecision> {
        let candidates = self.events.evidence_candidates(row)?;
        let object = match row.object_key.as_deref() {
            Some(key) => self.objects.get(key)?,
            None => None,
        };
        let object = object.as_ref().map(ObjectRow::to_rule_object);
        let opened = OpenedEvent {
            rule_id: row.rule_id.clone(),
            thread_key: row.thread_key.clone(),
            occurred_at: row.occurred_at.clone(),
            actor_id: row.actor_id.clone(),
        };
        Ok(ClosureJudge::judge(&ClosureInput {
            run_outcome: Outcome::Ok,
            opened: &opened,
            candidates: &candidates,
            object: object.as_ref(),
            now,
        }))
    }
}
