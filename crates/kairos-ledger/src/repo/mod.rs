//! 표별 Repo. SQL은 이 모듈 안에만 둔다. 트랜잭션 안에서도 쓰이므로 모두 `&Connection`을 받는다.

mod cursors;
mod events;
mod item_events;
mod objects;
mod outsider_marks;
mod source_runs;
mod thread_reads;

pub use cursors::CursorRepo;
pub use events::EventRepo;
pub(crate) use events::{NewEvent, OpenEvent};
pub use item_events::{EventHead, ItemEvent, LinkedClosure, WouldShowEvent};
pub(crate) use objects::{DayOffScope, ObjectStamp};
pub use objects::{ObjectRepo, ObjectRow};
pub use outsider_marks::OutsiderMarkRepo;
pub(crate) use source_runs::NewRun;
pub use source_runs::{CoverageGapRow, RunSummary, SourceRunRepo};
pub use thread_reads::ThreadReadRepo;

/// 정제에서 비워진 필수 값은 TS에서 null이 된다. 그대로 NULL로 넘겨 NOT NULL 제약이 막게 한다.
fn required(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}
