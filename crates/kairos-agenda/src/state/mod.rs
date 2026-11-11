//! 사용자가 정한 값, 표시 게이트, 소스 건강, state.json, 알림에서 연 항목.

mod approval;
mod builder;
mod control;
mod gate;
mod health;
mod reveal;

pub use approval::{SESSION_APPROVAL_FILE, SessionApproval, SessionLock};
pub use builder::{
    CountsView, EngineStatus, ItemSummary, KairosState, RunCounts, STATE_VERSION, StateBuilder, WouldShowCounts,
    WouldShowRow, pretty_json,
};
pub use control::{ControlFile, DisplayMode, FocusControl, INDEFINITE, KairosControl};
pub use gate::{DisplayGate, DisplayStart, GateReason, earliest_t0};
pub use health::{
    CoverageGap, GAP_NOTICE_DAYS, HealthStatus, MIN_ALERT_SECONDS, NOTION_FORMAT_THRESHOLD, NOTION_FORMAT_WINDOW_MS,
    SourceHealth, SourceHealthBook,
};
pub use reveal::{REVEAL_FILE, REVEAL_MAX_ITEMS, REVEAL_WINDOW_MS, RevealFile, RevealView, is_item_id};
