//! 패널이 읽는 뷰 타입. 필드 순서가 JSON 키 순서이고 이 값이 패널 계약이다.

use kairos_core::{Instant, OffDates, string_enum};
use kairos_policy::ItemReason;
use serde::Serialize;

use super::playbook::Playbook;
use crate::item::{ItemState, ItemTier, ParkTrigger};
use crate::repo::{CardKind, ReadyKind, RunView};
use crate::state::{DisplayGate, GateReason, KairosControl};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CardAction {
    /// start, later, done, keep, ack, cancel, triage_keep, triage_drop, move
    pub id: String,
    pub label: String,
    /// kairos 뒤에 그대로 붙일 인자 배열. 셸을 거치지 말고 argv로 부른다.
    pub argv: Vec<String>,
}

impl CardAction {
    pub(super) fn new(id: &str, label: &str, argv: &[&str]) -> Self {
        Self { id: id.to_owned(), label: label.to_owned(), argv: argv.iter().map(|part| (*part).to_owned()).collect() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployView {
    pub date: kairos_core::LocalDate,
    pub linked_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: i64,
    pub tier: ItemTier,
    pub reason: ItemReason,
    pub reason_label: String,
    pub state: ItemState,
    pub source: Option<String>,
    pub source_label: String,
    pub title: String,
    pub url: Option<String>,
    pub opened_at: String,
    pub due_at: Option<String>,
    pub parked_until: Option<String>,
    pub park_trigger: Option<ParkTrigger>,
    pub baseline: bool,
    /// 어제 이전에 열린 P1이다.
    pub carried: bool,
    pub push_count: i64,
    pub deploy: Option<DeployView>,
    pub playbook: Option<Playbook>,
    pub scope: Option<String>,
    pub run: Option<RunView>,
    pub actions: Vec<CardAction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub kind: CardKind,
    pub tier: ItemTier,
    pub headline: String,
    /// 그릴 줄. 묶음은 7줄까지이고 나머지는 more로 접는다.
    pub items: Vec<ItemView>,
    pub more: usize,
    /// 접힌 줄까지 포함한 전체 ID.
    pub item_ids: Vec<i64>,
    pub actions: Vec<CardAction>,
    /// 이미 --seen으로 기록돼 응답을 기다리는 카드다.
    pub pending: bool,
    pub shown_at: Option<String>,
}

string_enum! {
    /// 앞의 넷은 표시 게이트가 닫혔다. day_off, off_hours, focus, lunch는 지금 P1 이하를 보류한다.
    /// budget_hour, budget_day는 예산을 다 썼다. nothing은 지금 낼 제안이 없다.
    pub enum HoldReason {
        Paused => "paused",
        DisplayOff => "display_off",
        NoCollection => "no_collection",
        Recording => "recording",
        DayOff => "day_off",
        OffHours => "off_hours",
        Focus => "focus",
        Lunch => "lunch",
        BudgetHour => "budget_hour",
        BudgetDay => "budget_day",
        Nothing => "nothing",
    }
}

impl From<GateReason> for HoldReason {
    fn from(reason: GateReason) -> Self {
        match reason {
            GateReason::Paused => Self::Paused,
            GateReason::DisplayOff => Self::DisplayOff,
            GateReason::NoCollection => Self::NoCollection,
            GateReason::Recording => Self::Recording,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Hold {
    pub reason: HoldReason,
    pub message: String,
    /// 풀리는 시각. 모르면 None이다.
    pub until: Option<String>,
}

/// 세션 쪽 카드. 예산과 무관하고 run마다 1회만 나온다. 게이트가 닫히면 실패 카드만 나온다.
/// 키 순서는 TS의 펼침 순서를 따른다. 공통 값이 앞이고 종류, 문구, 버튼이 뒤다.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadyCard {
    pub run_id: String,
    pub item_id: i64,
    pub title: String,
    pub url: Option<String>,
    /// question 질문 대기, done 턴이 끝남, ended 세션 끝, attention ps에 보이지 않음, failed 뒤에서 실패해 항목을 되돌림.
    pub kind: Option<ReadyKind>,
    /// ready_kind 없이 표시된 행은 TS에서 undefined라 키가 빠진다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    pub actions: Vec<CardAction>,
}

/// 카드를 고르는 데 쓰는 값.
#[derive(Clone, Debug)]
pub struct NextContext {
    pub now: Instant,
    pub control: KairosControl,
    pub off_dates: OffDates,
    pub gate: DisplayGate,
    pub health: Option<String>,
}
