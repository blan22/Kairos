//! 수집 모드를 거치는 소스: Calendar, Gmail, Slack, Notion
//!
//! 소스는 호출 계획을 내고(`McpSource::plan`) 모드가 돌려준 응답을 해석한다(`McpSource::parse`).
//! DB와 네트워크를 모른다. 쓰는 것은 `kairos-core`뿐이고 `finished_at`을 재는 시계만 생성자로 받는다.

pub mod calendar;
pub mod gmail;
pub mod notion;
pub mod shared;
pub mod slack;

pub use calendar::CalendarSource;
pub use gmail::GmailSource;
pub use notion::NotionSource;
pub use slack::SlackSource;
