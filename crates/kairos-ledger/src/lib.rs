//! 수집 기록: 객체, 이벤트, 커서, 실행 기록과 그 판정.
//!
//! `SourceResult` 하나를 `Ledger::ingest`가 한 트랜잭션으로 저장한다. 역할은 이렇게 나뉜다.
//! - `ContractGuard` 소스가 낸 결과의 계약 검사
//! - `ObjectStore` 객체 저장, 스냅샷 해시, 일정과 리뷰 변화 이벤트, 빠진 객체 처리
//! - `EventRecorder` 이벤트 삽입과 분류
//! - `DeployWatch` 배포 위험 판정
//! - `ClosureSweeper` 이벤트 종료 판정
//! - `CursorKeeper` 커서 전진, 진행 표시, 후속 읽기 기록
//!
//! SQL은 `repo` 안의 표별 Repo에만 있다.

mod cleaning;
mod closure_sweeper;
mod contract_guard;
mod cursor_keeper;
mod deploy_watch;
mod event_recorder;
mod invariants;
mod ledger;
mod object_store;
mod off_dates;
mod repo;

pub use contract_guard::{ContractGuard, Verdict};
pub use deploy_watch::{UpcomingDeploy, upcoming_deploys};
pub use event_recorder::EVENT_RETENTION_DAYS;
pub use invariants::{Invariants, check_invariants};
pub use ledger::{IngestOptions, IngestReport, Ledger, MANUAL_SKIP};
pub use object_store::snapshot_hash;
pub use off_dates::OffDateBook;
pub use repo::{
    CoverageGapRow, CursorRepo, EventHead, EventRepo, ItemEvent, LinkedClosure, ObjectRepo, ObjectRow,
    OutsiderMarkRepo, RunSummary, SourceRunRepo, ThreadReadRepo, WouldShowEvent,
};
