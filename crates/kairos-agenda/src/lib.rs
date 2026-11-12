//! 항목, 표시 게이트, 카드와 알림. 사용자가 실제로 보는 것은 여기서 정해진다.
//!
//! - `ItemEngine` 저장된 이벤트로 항목을 갱신한다.
//! - `ItemActions`, `ItemTransitions` 사용자 조작과 상태 전이.
//! - `ControlFile`, `DisplayGate`, `SourceHealthBook`, `StateBuilder` 사용자가 정한 값과 표시 게이트, 건강, state.json.
//! - `CardChooser`, `CardBuilder`, `BudgetMeter`, `FocusTracker`, `Presenter` 지금 카드와 패널 JSON.
//! - `P0Alerts`, `P1Alerts`, `NativeNotifier`, `OrcaPanelOpener` 알림.
//! - `RevealFile` 알림을 눌러 연 항목(reveal.json).
//!
//! SQL은 `repo` 안의 표별 Repo에만 있다. 표시 JSON의 필드 순서는 패널 계약이다.

mod actions;
mod alerts;
mod cards;
mod engine;
mod item;
mod repo;
mod state;

pub use actions::{
    ActionContext, ActionResult, ActionStatus, IdSource, ItemActions, ItemTransitions, LaterWhen, RandomIds,
    TRIAGE_BATCH, TriageBatch, TriageDecision,
};
pub use alerts::*;
pub use cards::*;
pub use engine::{EngineReport, ItemEngine, RefreshInput};
pub use item::{Item, ItemCloseReason, ItemState, ItemTier, P1Notice, ParkTrigger};
pub use repo::{
    CardKind, ItemCounts, ItemRepo, ObjectRun, PendingCardRow, ReadOnlyRunRepo, ReadyKind, ReadyRow, RunView,
    SuggestionRepo, SuggestionResponse, TierCounts,
};
pub use state::*;
