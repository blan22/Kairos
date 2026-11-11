//! 이벤트를 항목에 붙인다.

use std::collections::BTreeSet;

use kairos_core::{Instant, KairosError, KairosResult, OffDates};
use kairos_ledger::{EventRepo, ItemEvent, ObjectRepo, upcoming_deploys};
use kairos_policy::{DeployLinkStatus, DeployLinker, Disposition, ItemReason, P1_DUE_MS, work_start_minutes};
use kairos_store::Connection;

use super::EngineReport;
use super::close::close_item;
use crate::item::{Item, ItemCloseReason, ItemState, ItemTier, rule_target};
use crate::repo::{EventItem, ItemFold, ItemRepo, SuggestionResponse};

const WITHDRAWS_MEETING: [&str; 2] = ["cal_cancelled", "object_withdrawn"];

fn withdraws_meeting(event: &ItemEvent) -> bool {
    WITHDRAWS_MEETING.contains(&event.event_type.as_str())
}

fn invalid_time() -> KairosError {
    KairosError::new("invalid_time")
}

/// `new Date(text).toISOString()`. 해석할 수 없으면 던진다. 호출자의 저장점이 그 이벤트만 건너뛴다.
fn iso_of(text: &str) -> KairosResult<String> {
    Instant::parse(text).map(Instant::to_iso).ok_or_else(invalid_time)
}

fn due_at(
    connection: &Connection,
    tier: ItemTier,
    reason: ItemReason,
    event: &ItemEvent,
) -> KairosResult<Option<String>> {
    if matches!(reason, ItemReason::MeetingNew | ItemReason::DeployRisk) {
        let key = event.object_key.as_deref().unwrap_or("");
        let object = ObjectRepo::new(connection).get(key)?;
        let start_at = object.as_ref().and_then(|object| object.start_at.as_deref()).filter(|text| !text.is_empty());
        if let Some(start_at) = start_at {
            return iso_of(start_at).map(Some);
        }
        // ingest는 날짜 모양만 본다. 달력에 없는 날이어도 항목은 열어야 하므로 기한만 비운다.
        let start_date = object.as_ref().and_then(|object| object.start_date.as_deref());
        return match start_date.filter(|date| Instant::parse(date).is_some()) {
            Some(date) => {
                let hour = work_start_minutes() / 60;
                let minute = work_start_minutes() % 60;
                iso_of(&format!("{date}T{hour:02}:{minute:02}:00+09:00")).map(Some)
            }
            None => Ok(None),
        };
    }
    if tier == ItemTier::P2 {
        return Ok(None);
    }
    let occurred = Instant::parse(&event.occurred_at).ok_or_else(invalid_time)?;
    Ok(Some(occurred.plus_ms(P1_DUE_MS).to_iso()))
}

fn fold_into(items: ItemRepo<'_>, item: &Item, event: &ItemEvent, tier: ItemTier) -> KairosResult<()> {
    let wake = item.state == ItemState::Parked && (tier == ItemTier::P0 || item.is_parked_quietly());
    // 새 변화는 새 기회다. P2 사건은 카드가 없으므로 다음 묶음에 다시 싣는다.
    let fresh = tier != ItemTier::P2;
    let queued = if event.catchup || (!fresh && item.first_surfaced_at.is_some()) {
        true
    } else if fresh {
        false
    } else {
        item.bundle_queued
    };
    // 배포 단계는 (날짜, 단계)마다 한 번만 온다. 이미 알린 항목도 일정이 밀려 다시 온 단계는 새 위험이다.
    // 당일 뒤에 D-1이 오는 것은 밀렸을 때뿐이라 등급도 단계를 따른다. 그래야 D-1이 당일 알림으로 나가지 않는다.
    let deploy = item.reason == ItemReason::DeployRisk;
    items.fold(&ItemFold {
        id: item.id,
        tier: if deploy { tier } else { item.tier.higher(tier) },
        event_key: &event.dedupe_key,
        push_count: if fresh { 0 } else { item.push_count },
        bundle_queued: queued,
        state: if wake { ItemState::Open } else { item.state },
        park_trigger: if wake { None } else { item.park_trigger },
        parked_until: if wake { None } else { item.parked_until.as_deref() },
        rearm_p0: tier == ItemTier::P0,
        rearm_p1: deploy,
    })
}

fn open_item(
    connection: &Connection,
    key: &str,
    reason: ItemReason,
    tier: ItemTier,
    event: &ItemEvent,
) -> KairosResult<i64> {
    let due = due_at(connection, tier, reason, event)?;
    ItemRepo::new(connection).open_from_event(&EventItem {
        key,
        reason,
        tier,
        source: &event.source,
        channel_id: event.channel_id.as_deref(),
        opened_by_event: &event.dedupe_key,
        opened_at: &event.occurred_at,
        due_at: due.as_deref(),
        bundle_queued: event.catchup,
    })
}

/// 접힌 댓글은 새 항목을 만들지 않는다. 조용히 대기 중인 항목만 깨우고, 이력으로 붙여 다시 보지 않게 한다.
fn assign_folded(connection: &Connection, event: &ItemEvent, report: &mut EngineReport) -> KairosResult<()> {
    let Some((reason, tier)) = rule_target(&event.rule_id) else {
        return Ok(());
    };
    let items = ItemRepo::new(connection);
    let key = Item::key_of(event.object_key.as_deref(), &event.thread_key);
    let Some(existing) = items.live(key, reason)? else {
        return Ok(());
    };
    if existing.is_parked_quietly() {
        fold_into(items, &existing, event, tier)?;
    }
    EventRepo::new(connection).link_item(event.id, existing.id)?;
    report.folded += 1;
    Ok(())
}

fn assign_opened(
    connection: &Connection,
    event: &ItemEvent,
    now: &str,
    ok_sources: &BTreeSet<String>,
    report: &mut EngineReport,
) -> KairosResult<()> {
    let Some((reason, tier)) = rule_target(&event.rule_id) else {
        return Ok(());
    };
    let items = ItemRepo::new(connection);
    let events = EventRepo::new(connection);
    let key = Item::key_of(event.object_key.as_deref(), &event.thread_key);
    report.lifted += items.lift_baseline(key)?;
    let existing = items.live(key, reason)?;
    let withdraws = reason == ItemReason::MeetingNew && withdraws_meeting(event);
    match existing {
        Some(existing) if withdraws => {
            // ok가 아닌 실행은 어떤 종료도 만들지 못한다(4.4). 이벤트를 남겨 두면 다음 ok 틱에 여기로 다시 온다.
            if !ok_sources.contains(&event.source) {
                return Ok(());
            }
            // 취소와 철회는 이 항목의 종료 증거다. 같은 사건이 항목을 살려 두면 4.4의 withdrawn이 성립하지 않는다.
            close_item(
                connection,
                existing.id,
                ItemCloseReason::Withdrawn,
                Some(&event.dedupe_key),
                now,
                SuggestionResponse::Closed,
            )?;
            events.link_item(event.id, existing.id)?;
            report.closed += 1;
        }
        Some(existing) => {
            fold_into(items, &existing, event, tier)?;
            events.link_item(event.id, existing.id)?;
            report.folded += 1;
        }
        None => {
            // 이벤트 종료로 항목이 먼저 닫힌 경우다. 같은 철회가 새 항목을 만들면 안 된다.
            let withdrawn = if withdraws { items.withdrawn_meeting(key)? } else { None };
            if let Some(withdrawn) = withdrawn {
                events.link_item(event.id, withdrawn)?;
            } else {
                events.link_item(event.id, open_item(connection, key, reason, tier, event)?)?;
                report.opened += 1;
            }
        }
    }
    Ok(())
}

/// 한 건의 예외가 틱 전체의 항목 갱신을 되돌리지 않게 이벤트마다 격리한다. 건너뛴 건은 다음 틱에 다시 본다.
pub(super) fn assign_events(
    connection: &Connection,
    now: &str,
    ok_sources: &BTreeSet<String>,
    report: &mut EngineReport,
) -> KairosResult<()> {
    let items = ItemRepo::new(connection);
    for event in EventRepo::new(connection).awaiting_item()? {
        let assigned = items.isolated(|| {
            if event.disposition == Disposition::Folded {
                assign_folded(connection, &event, report)
            } else {
                assign_opened(connection, &event, now, ok_sources, report)
            }
        })?;
        if !assigned {
            report.skipped += 1;
        }
    }
    Ok(())
}

/// 배포 D-1과 당일에 연결된 기준선 항목을 올린다. 배포 연결은 baseline보다 우선한다.
pub(super) fn lift_deploy_linked(connection: &Connection, now: Instant, off_dates: &OffDates) -> KairosResult<usize> {
    let today = now.local_date();
    let items = ItemRepo::new(connection);
    let mut lifted = 0;
    for deploy in upcoming_deploys(connection, now)? {
        if deploy.link.status != DeployLinkStatus::Linked
            || DeployLinker::stage(deploy.date, today, off_dates).is_none()
        {
            continue;
        }
        for key in &deploy.link.object_keys {
            lifted += items.lift_baseline(key)?;
        }
    }
    Ok(lifted)
}
