use std::collections::HashSet;

use kairos_core::{Instant, KairosError, KairosResult, string_enum};
use kairos_policy::{PUSH_LIMIT, WorkSchedule, lunch_end_minutes, merge_group};
use kairos_store::Connection;

use super::budget_meter::BudgetUsage;
use super::focus_tracker::{FocusTracker, FocusView};
use super::views::{Hold, HoldReason, NextContext};
use crate::item::{Item, ItemTier};
use crate::repo::{CardKind, ItemRepo, PendingCardRow, SuggestionRepo};

const HOUR_MS: i64 = 3_600_000;

string_enum! {
    /// send 지금 알린다.
    /// hold 기다린다. 포커스, 점심, 근무 시간 밖이고 끝난 뒤 첫 틱에 한 번에 알린다.
    /// drop 이때 처음 잡힌 건은 알리지 않는다. 표시 게이트, 일시정지, 쉬는 날이다. 앞서 보류했거나 전달에 실패한 건은 남겨 열린 뒤 알린다.
    /// off 사용자가 P1 알림을 끈 것이다. 기다리던 건도 모두 알리지 않는다.
    /// 어느 쪽이든 항목은 패널에 남는다.
    pub enum P1AlertMode {
        Send => "send",
        Hold => "hold",
        Drop => "drop",
        Off => "off",
    }
}

/// 고른 카드.
#[derive(Clone, Debug)]
pub(super) struct Choice {
    pub kind: CardKind,
    pub base_id: String,
    pub rows: Vec<Item>,
    /// 줄을 접지 않는 앞쪽 건수.
    pub keep: usize,
    /// 이미 --seen으로 기록돼 응답을 기다리는 카드면 그 시각이다.
    pub shown_at: Option<String>,
}

pub(super) enum Decision {
    Card(Choice),
    Hold(Hold),
}

fn hold(reason: HoldReason, message: &str, until: Option<String>) -> Decision {
    Decision::Hold(Hold { reason, message: message.to_owned(), until })
}

/// 줄의 등급. 하나라도 높으면 카드 전체가 그 등급이다.
pub(super) fn tier_of_rows(rows: &[Item]) -> ItemTier {
    rows.iter().map(|row| row.tier).min().unwrap_or(ItemTier::P2)
}

fn from_pending(rows: Vec<PendingCardRow>, now: Instant) -> Option<Choice> {
    let first = rows.first()?;
    let kind = first.card_kind;
    let base_id = first.card_id.split('@').next().unwrap_or("").to_owned();
    let shown_at = first.shown_at.clone();
    let items: Vec<Item> = rows.into_iter().map(|row| row.item).collect();
    let keep = items.iter().filter(|row| row.is_carried(now)).count();
    Some(Choice { kind, base_id, rows: items, keep, shown_at: Some(shown_at) })
}

/// P1 이하를 지금 보이지 않게 하는 조건. 게이트와 예산은 따로다.
fn quiet_hold(context: &NextContext, focus: &FocusView) -> Option<Hold> {
    let now = context.now;
    let today = now.local_date();
    let schedule = WorkSchedule::new(&context.off_dates);
    let held = |reason, message: &str, until: Option<String>| Hold { reason, message: message.to_owned(), until };
    if !context.off_dates.is_workday(today) {
        let until = schedule.next_work_start(now).to_iso();
        return Some(held(HoldReason::DayOff, "쉬는 날입니다. 복귀일에 보여 줍니다", Some(until)));
    }
    if !schedule.is_work_hours(now) {
        return Some(held(HoldReason::OffHours, "근무 시간 밖입니다", Some(schedule.next_work_start(now).to_iso())));
    }
    if focus.active {
        return Some(held(HoldReason::Focus, "포커스 중입니다", focus.until.clone()));
    }
    if schedule.is_lunch(now) {
        let until = Instant::at_local(today, lunch_end_minutes()).to_iso();
        return Some(held(HoldReason::Lunch, "점심시간입니다", Some(until)));
    }
    None
}

/// 설계 6.3과 6.5의 순서로 지금 카드를 고른다.
#[derive(Clone, Copy, Debug)]
pub struct CardChooser<'a> {
    connection: &'a Connection,
}

impl<'a> CardChooser<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub(super) fn quiet_hold(context: &NextContext, focus: &FocusView) -> Option<Hold> {
        quiet_hold(context, focus)
    }

    /// 지금 일반 카드가 사용자에게 가려져 있는가. 수집 틱의 만료 판정에도 쓴다.
    pub fn cards_hidden(&self, context: &NextContext) -> KairosResult<bool> {
        if !context.gate.open {
            return Ok(true);
        }
        let focus = FocusTracker::new(self.connection).view(&context.control, context.now)?;
        Ok(quiet_hold(context, &focus).is_some())
    }

    /// P1 Mac 알림의 시간 판단. 지금 카드의 보류를 그대로 쓴다. 예산은 보지 않는다.
    pub fn p1_alert_mode(&self, context: &NextContext) -> KairosResult<P1AlertMode> {
        if !context.control.p1_notify_on() {
            return Ok(P1AlertMode::Off);
        }
        if !context.gate.open {
            return Ok(P1AlertMode::Drop);
        }
        let focus = FocusTracker::new(self.connection).view(&context.control, context.now)?;
        Ok(match quiet_hold(context, &focus) {
            None => P1AlertMode::Send,
            Some(quiet) if quiet.reason == HoldReason::DayOff => P1AlertMode::Drop,
            Some(_) => P1AlertMode::Hold,
        })
    }

    /// 응답을 기다리는 카드 한 장. 가장 나중의 노출과 같은 카드의 줄이다.
    fn pending_card(&self) -> KairosResult<Vec<PendingCardRow>> {
        let rows = SuggestionRepo::new(self.connection).pending()?;
        let Some(latest) = rows.last().map(|row| row.card_id.clone()) else {
            return Ok(Vec::new());
        };
        Ok(rows.into_iter().filter(|row| row.card_id == latest).collect())
    }

    fn bundle_candidates(&self, now: Instant, overflow: bool) -> KairosResult<(Vec<Item>, usize)> {
        let items = ItemRepo::new(self.connection);
        let p2 = items.bundle_p2()?;
        let p1 = items.bundle_p1(overflow)?;
        let (carried, fresh): (Vec<Item>, Vec<Item>) = p1.into_iter().partition(|row| row.is_carried(now));
        let keep = carried.len();
        Ok(([carried, fresh, p2].concat(), keep))
    }

    /// P0는 게이트 뒤의 모든 보류와 예산을 넘는다.
    pub(super) fn choose(
        &self,
        context: &NextContext,
        budget: &BudgetUsage,
        focus: &FocusView,
    ) -> KairosResult<Decision> {
        let NextContext { now, gate, .. } = context;
        let now = *now;
        if let Some(reason) = gate.reason.filter(|_| !gate.open) {
            let until = gate.starts_at.map(Instant::to_iso);
            return Ok(hold(reason.into(), gate.message.as_deref().unwrap_or(""), until));
        }

        let pending = self.pending_card()?;
        let items = ItemRepo::new(self.connection);
        if pending.iter().any(|row| row.item.tier == ItemTier::P0)
            && let Some(choice) = from_pending(pending.clone(), now)
        {
            return Ok(Decision::Card(choice));
        }
        // 사용자 요청: 같은 등급 안에서는 최신이 맨 위다. 넘어온 일은 carried가 따로 먼저 낸다.
        if let Some(urgent) = items.open_p0()?.pop() {
            let base_id = format!("item-{}", urgent.id);
            return Ok(Decision::Card(Choice {
                kind: CardKind::Item,
                base_id,
                rows: vec![urgent],
                keep: 0,
                shown_at: None,
            }));
        }

        if let Some(quiet) = quiet_hold(context, focus) {
            return Ok(Decision::Hold(quiet));
        }
        if let Some(choice) = from_pending(pending, now) {
            return Ok(Decision::Card(choice));
        }
        let today = now.local_date();

        let hour_left = budget.p1_hour_used < budget.p1_hour_limit;
        let day_left = budget.p1_day_used < budget.p1_day_limit;
        let p1 = items.open_p1()?;
        let regular: Vec<Item> =
            p1.iter().filter(|row| row.push_count < PUSH_LIMIT && !row.bundle_queued).cloned().collect();
        let suggestions = SuggestionRepo::new(self.connection);
        let exposed: HashSet<i64> = suggestions.exposed_since(&now.start_of_local_day().to_iso())?;
        let carried: Vec<Item> =
            p1.iter().filter(|row| row.is_carried(now) && !exposed.contains(&row.id)).cloned().collect();
        if hour_left && day_left {
            if !carried.is_empty() {
                let keep = carried.len();
                return Ok(Decision::Card(Choice {
                    kind: CardKind::Carryover,
                    base_id: format!("carry-{today}"),
                    rows: carried,
                    keep,
                    shown_at: None,
                }));
            }
            let newest_first: Vec<Item> = regular.iter().rev().cloned().collect();
            let group: Vec<Item> = merge_group(&newest_first).into_iter().cloned().collect();
            if let Some(anchor) = group.first() {
                let base_id = format!("item-{}", anchor.id);
                return Ok(Decision::Card(Choice {
                    kind: CardKind::Item,
                    base_id,
                    rows: group,
                    keep: 0,
                    shown_at: None,
                }));
            }
        }

        if let Some(slot) = WorkSchedule::new(&context.off_dates).bundle_slot(now) {
            let slot = slot.to_iso();
            if !suggestions.bundle_seen_since(&slot)? {
                let (rows, keep) = self.bundle_candidates(now, !day_left)?;
                if !rows.is_empty() {
                    return Ok(Decision::Card(Choice {
                        kind: CardKind::Bundle,
                        base_id: format!("bundle-{slot}"),
                        rows,
                        keep,
                        shown_at: None,
                    }));
                }
            }
        }
        // 여기까지 온 대기 건은 예산 때문이다. 예산이 남았으면 위에서 카드가 됐다.
        let waiting = regular.len() + carried.len() > 0;
        if waiting && !day_left {
            let until = Some(budget.next_bundle_at.to_iso());
            return Ok(hold(HoldReason::BudgetDay, "오늘 P1 표시를 다 썼습니다. 다음 묶음으로 보냅니다", until));
        }
        if waiting {
            let last = suggestions.last_p1_shown()?;
            // 표시 기록이 없으면 Date.parse(null)이 NaN이고 toISOString이 던진다.
            let last = last.as_deref().and_then(Instant::parse).ok_or_else(|| KairosError::new("invalid_time"))?;
            let until = Some(last.plus_ms(HOUR_MS).to_iso());
            return Ok(hold(HoldReason::BudgetHour, "시간당 P1 표시를 다 썼습니다", until));
        }
        Ok(hold(HoldReason::Nothing, "지금 낼 제안이 없습니다", None))
    }
}
