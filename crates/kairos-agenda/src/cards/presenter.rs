use kairos_core::{Instant, KairosResult};
use kairos_store::{Connection, Database};
use serde::Serialize;

use super::CONTRACT_VERSION;
use super::budget_meter::{BudgetMeter, BudgetUsage};
use super::card_builder::CardBuilder;
use super::card_chooser::{CardChooser, Choice, Decision, tier_of_rows};
use super::focus_tracker::{FocusTracker, FocusView};
use super::views::{Card, Hold, ItemView, NextContext, ReadyCard};
use crate::engine::{expire_cards, hold_cards, wake_parked};
use crate::item::Item;
use crate::repo::{ItemCounts, ItemRepo, ReadOnlyRunRepo, SuggestionRepo};
use crate::state::{DisplayGate, RevealView, SourceStatus};

/// 활성 run 수. 패널은 이 값이 0보다 크면 kairos runs를 부른다. 항목 상태와 무관하다.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RunsView {
    pub active: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NextResponse {
    pub schema: i64,
    pub ok: bool,
    pub at: Instant,
    pub display: DisplayGate,
    pub card: Option<Card>,
    pub hold: Option<Hold>,
    pub ready: Vec<ReadyCard>,
    pub runs: RunsView,
    pub counts: ItemCounts,
    pub budget: BudgetUsage,
    pub focus: FocusView,
    /// 건강 경고 한 줄. 예산에서 제외다.
    pub health: Option<String>,
    /// 10분 안에 알림을 눌러 연 항목. 없으면 키가 없다. 데이터 폴더의 파일이라 CLI가 채운다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal: Option<RevealView>,
    /// 소스별 수집 상태. 소스 구성을 아는 CLI가 채운다. 없으면 키가 없다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<SourceStatus>>,
    /// 열린 항목 중 가장 큰 ID. 호스트가 사용자가 아직 보지 않은 새 항목이 있는지 가린다. CLI가 채운다. 없으면 키가 없다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ListResponse {
    pub schema: i64,
    pub ok: bool,
    pub at: Instant,
    pub display: DisplayGate,
    pub items: Vec<ItemView>,
    /// 게이트 때문에 내지 않은 건수.
    pub hidden: usize,
    pub counts: ItemCounts,
    pub focus: FocusView,
    pub health: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NextOptions {
    pub seen: bool,
    pub ack: bool,
}

/// 실제로 그린 카드를 기록한다. 같은 ID가 다시 쓰여도 예산은 카드별로 세므로 시각을 붙인다.
fn record_seen(connection: &Connection, choice: &Choice, now: Instant) -> KairosResult<()> {
    let suggestions = SuggestionRepo::new(connection);
    let items = ItemRepo::new(connection);
    let at = now.to_iso();
    suggestions.supersede_waiting(&at)?;
    let card_id = format!("{}@{}", choice.base_id, now.millis());
    let tier = tier_of_rows(&choice.rows);
    for row in &choice.rows {
        suggestions.insert_seen(row.id, &card_id, choice.kind, tier, &at)?;
        items.mark_surfaced(row.id, &at)?;
        items.settle_p1_notice(row.id, &at)?;
    }
    Ok(())
}

fn next(connection: &Connection, context: &NextContext, options: NextOptions) -> KairosResult<NextResponse> {
    let now = context.now;
    let at = now.to_iso();
    let meter = BudgetMeter::new(connection);
    let chooser = CardChooser::new(connection);
    wake_parked(connection, &at)?;
    let focus = FocusTracker::new(connection).view(&context.control, now)?;
    if !context.gate.open || CardChooser::quiet_hold(context, &focus).is_some() {
        hold_cards(connection, &at)?;
    } else {
        expire_cards(connection, now)?;
    }
    if options.ack {
        SuggestionRepo::new(connection).ack_cards(&at)?;
    }
    let builder = CardBuilder::new(connection, now, context.control.sessions_allowed())?;
    let decision = chooser.choose(context, &meter.usage(now, &context.off_dates)?, &focus)?;
    let (card, hold) = match decision {
        Decision::Hold(hold) => (None, Some(hold)),
        Decision::Card(choice) => {
            if options.seen && choice.shown_at.is_none() {
                record_seen(connection, &choice, now)?;
            }
            let shown_at = choice.shown_at.clone().or_else(|| options.seen.then(|| at.clone()));
            (Some(builder.build_card(&choice, shown_at)?), None)
        }
    };
    Ok(NextResponse {
        schema: CONTRACT_VERSION,
        ok: true,
        at: now,
        display: context.gate.clone(),
        card,
        hold,
        ready: builder.ready_cards(options.seen, context.gate.open)?,
        runs: RunsView { active: ReadOnlyRunRepo::new(connection).active_count()? },
        counts: ItemRepo::new(connection).counts(now)?,
        budget: meter.usage(now, &context.off_dates)?,
        focus,
        health: context.health.clone(),
        reveal: None,
        sources: None,
        newest: None,
    })
}

fn list(connection: &Connection, context: &NextContext, all: bool) -> KairosResult<ListResponse> {
    let now = context.now;
    wake_parked(connection, &now.to_iso())?;
    let items = ItemRepo::new(connection);
    let rows = items.listed()?;
    let visible = context.gate.open || all;
    let builder = CardBuilder::new(connection, now, context.control.sessions_allowed())?;
    let views = if visible {
        rows.iter().map(|row| builder.view_item(row)).collect::<KairosResult<Vec<_>>>()?
    } else {
        Vec::new()
    };
    Ok(ListResponse {
        schema: CONTRACT_VERSION,
        ok: true,
        at: now,
        display: context.gate.clone(),
        items: views,
        hidden: if visible { 0 } else { rows.len() },
        counts: items.counts(now)?,
        focus: FocusTracker::new(connection).view(&context.control, now)?,
        health: context.health.clone(),
    })
}

/// 패널이 읽는 응답을 만든다.
#[derive(Clone, Copy, Debug)]
pub struct Presenter<'a> {
    database: &'a Database,
}

impl<'a> Presenter<'a> {
    pub const fn new(database: &'a Database) -> Self {
        Self { database }
    }

    pub fn next_response(&self, context: &NextContext, options: NextOptions) -> KairosResult<NextResponse> {
        self.database.transaction(|connection| next(connection, context, options))
    }

    /// 카드는 그리지 않고 세션 소식만 그리는 화면의 읽기. 낸 세션 소식을 확인으로 적고, 카드 노출과 예산은 건드리지 않는다.
    pub fn next_response_ready_seen(&self, context: &NextContext, options: NextOptions) -> KairosResult<NextResponse> {
        self.database.transaction(|connection| {
            let response = next(connection, context, options)?;
            let suggestions = SuggestionRepo::new(connection);
            let at = context.now.to_iso();
            for card in &response.ready {
                suggestions.mark_ready_seen(&card.run_id, &at)?;
            }
            Ok(response)
        })
    }

    /// 패널의 목록. 사용자가 스스로 여는 것이라 예산을 쓰지 않고 노출을 기록하지 않는다.
    pub fn list_response(&self, context: &NextContext, all: bool) -> KairosResult<ListResponse> {
        self.database.transaction(|connection| list(connection, context, all))
    }

    /// triage 줄. 세션 버튼은 내지 않는다.
    pub fn triage_views(&self, rows: &[Item], now: Instant) -> KairosResult<Vec<ItemView>> {
        let builder = CardBuilder::new(self.database.connection(), now, false)?;
        rows.iter().map(|row| builder.triage_view(row)).collect()
    }
}
