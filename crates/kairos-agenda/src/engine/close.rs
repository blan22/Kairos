//! 항목의 종료.

use std::collections::BTreeSet;

use kairos_core::model::Outcome;
use kairos_core::{Instant, KairosResult};
use kairos_ledger::EventRepo;
use kairos_policy::{CloseReason, ClosureDecision, ClosureInput, ClosureJudge, ItemReason, OpenedEvent, RuleId};
use kairos_store::Connection;

use crate::item::{Item, ItemCloseReason};
use crate::repo::{ItemRepo, SuggestionRepo, SuggestionResponse};

/// 항목을 닫고 미응답 노출에 응답을 적는다.
pub(crate) fn close_item(
    connection: &Connection,
    id: i64,
    reason: ItemCloseReason,
    evidence: Option<&str>,
    now: &str,
    response: SuggestionResponse,
) -> KairosResult<()> {
    ItemRepo::new(connection).close(id, reason, evidence, now)?;
    answer(connection, id, response, now)
}

/// 미응답 노출에 응답을 적는다. 사용자가 다룬 P1은 알림도 끝낸다.
pub(crate) fn answer(connection: &Connection, id: i64, response: SuggestionResponse, now: &str) -> KairosResult<()> {
    SuggestionRepo::new(connection).answer(id, response, now)?;
    ItemRepo::new(connection).settle_p1_notice(id, now)
}

const fn baseline_rule(reason: ItemReason) -> Option<RuleId> {
    match reason {
        ItemReason::Assigned => Some(RuleId::P2Assigned),
        ItemReason::ReviewOwed => Some(RuleId::P2ReviewRequested),
        ItemReason::MyPrAttention => Some(RuleId::P1ChangesRequested),
        _ => None,
    }
}

/// 붙은 이벤트가 없는 항목의 종료. 기준선 항목과 거기서 올라온 항목이다. 그 소스가 이번 틱에 ok일 때만 판정한다.
fn close_baseline(
    connection: &Connection,
    item: &Item,
    now: Instant,
    ok_sources: &BTreeSet<String>,
) -> KairosResult<bool> {
    let Some(rule) = baseline_rule(item.reason) else {
        return Ok(false);
    };
    if !item.source.as_ref().is_some_and(|source| ok_sources.contains(source)) {
        return Ok(false);
    }
    let candidates = EventRepo::new(connection).evidence_on_object(&item.object_key)?;
    let opened = OpenedEvent {
        rule_id: rule.as_str().to_owned(),
        thread_key: item.object_key.clone(),
        occurred_at: item.opened_at.clone(),
        actor_id: None,
    };
    let decision = ClosureJudge::judge(&ClosureInput {
        run_outcome: Outcome::Ok,
        opened: &opened,
        candidates: &candidates,
        object: None,
        now,
    });
    let ClosureDecision::Close { reason, evidence } = decision else {
        return Ok(false);
    };
    if reason == CloseReason::Expired {
        return Ok(false);
    }
    close_item(connection, item.id, reason.into(), evidence.as_deref(), &now.to_iso(), SuggestionResponse::Closed)?;
    Ok(true)
}

/// 붙은 이벤트가 모두 닫힌 항목을 닫는다. 이벤트가 닫혔다는 것은 ok 실행의 양성 증거나 시각 경과를 뜻한다.
pub(super) fn close_resolved(
    connection: &Connection,
    now: Instant,
    ok_sources: &BTreeSet<String>,
) -> KairosResult<usize> {
    let events = EventRepo::new(connection);
    let at = now.to_iso();
    let mut closed = 0;
    for item in ItemRepo::new(connection).live_all()? {
        // 접힌 이벤트는 이력일 뿐 종료 판정 대상이 아니다. 세면 항목이 영원히 닫히지 않는다.
        let linked = events.linked_closures(item.id)?;
        let Some(last) = linked.last() else {
            if close_baseline(connection, &item, now, ok_sources)? {
                closed += 1;
            }
            continue;
        };
        if linked.iter().any(|row| row.close_reason.is_none()) {
            continue;
        }
        let Some(reason) = last.close_reason else {
            continue;
        };
        close_item(
            connection,
            item.id,
            reason.into(),
            last.close_evidence_event.as_deref(),
            &at,
            SuggestionResponse::Closed,
        )?;
        closed += 1;
    }
    Ok(closed)
}
