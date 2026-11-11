//! 수집 틱의 지휘: 잠금, 수집 모드, 소스 등록, 건강 진단
//!
//! - `TickRunner` 3분마다 launchd가 부르는 틱 하나. 판정 순서가 이 크레이트의 핵심이다.
//! - `CollectLock`, `HaltFlag` 틱이 겹치지 않게 막고, 모델이 불린 뒤 사람이 볼 때까지 멈춘다.
//! - `ClaudeMode` 수집 모드 실행의 인자, 환경, 판정.
//! - `SourceRegistry` 소스 목록. `NotConnectedSource`가 빈자리를 채운다.
//! - `TickPlanner`, `ResponseIngestor` 수집 모드가 부르는 plan-tick과 ingest.
//! - `Doctor`, `WatchFile` 사람과 감시자가 읽는 건강 상태.
//!
//! 표준 출력에는 쓰지 않는다. 보고와 줄은 값으로 돌려주고 kairos-cli가 찍는다.
//! SQL은 `TickRunRepo`에만 있다.

mod doctor;
mod halt;
mod ingestor;
mod lock;
mod mode;
mod planner;
mod registry;
mod repo;
mod tick;
mod watch;

pub use doctor::{Doctor, DoctorConfig, DoctorReport, IDENTITY_STALE_DAYS, LOGIN_WARN_DAYS, permission_problems};
pub use halt::{Halt, HaltFlag, js_number_text};
pub use ingestor::{
    IngestPayload, IngestRow, IngestSummary, MAX_PAGE_VALUE, ResponseIngestor, is_ingest_payload, page_info_of,
};
pub use lock::{CollectLock, LOCK_LIVE_PID_MS, LOCK_STALE_MS};
pub use mode::{ClaudeMode, ClaudeProbe, ClaudeRun, DROP_RESULT, RunVerdict, SystemClaudeProbe, judge_claude_run};
pub use planner::{DUE_TOLERANCE_SECONDS, OFF_HOURS_INTERVAL_SECONDS, PLAN_VERSION, TickPlan, TickPlanner};
pub use registry::{SourceFailure, SourceRegistry, not_connected_results};
pub use repo::TickRunRepo;
pub use tick::{
    AUTH_CHECK_INTERVAL_MS, AuthFile, AuthRecord, CollectReport, CollectStatus, EngineItems, ItemRefresher, ModeReport,
    OutcomeRow, SOURCE_LIMIT_MS, SleepProbe, TOTAL_LIMIT_MS, TickConfig, TickRunner, install_quiet_panic_hook,
};
pub use watch::{WatchFile, watch_path, watch_window_line, watch_window_path};
