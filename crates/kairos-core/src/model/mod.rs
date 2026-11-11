//! 소스가 지키는 계약. 소스 구현자는 이 모듈만 읽고 소스를 쓸 수 있어야 한다.
//!
//! 흐름: 소스 → `SourceResult`(질의 1개당 1개) → `Ledger::ingest` → objects, events, source_run, cursor
//!
//! 소스가 하는 일
//! - 외부 응답을 파싱해 `NormalizedObject`, `NormalizedEvent`로 바꾼다.
//! - 실패, 0건, 일부 실패를 `Outcome`으로 구분한다. 0건을 실패로, 실패를 0건으로 내지 않는다.
//! - 신원을 단언하는 질의는 `identity`에 실제 응답의 신원을 넣는다.
//! - 페이지가 하나라도 실패하면 outcome을 ok로 내지 않는다.
//!
//! 소스가 하지 않는 일 (Ledger가 한다)
//! - 문자열 정제, snapshot_hash 계산, baseline, catchup, disposition, rule_id 판정.
//! - 일정 변화 이벤트 생성. calendar 소스는 objects만 내고 events는 비워 둔다.
//!   kind가 meeting, deploy인 객체는 Ledger가 이전 스냅샷과 비교해 cal_* 이벤트를 만든다.
//! - 배포 연결 판정. deploy_risk_* 이벤트는 Ledger가 만든다.
//! - DB 접근. 소스는 DB를 직접 열지 않는다.
//!
//! 저장 금지 (설계 3.7): Slack과 메일 본문, 일정 description, 참석자 이메일, 요약.

mod enums;
mod mcp;
mod normalized;
mod result;
mod source;

pub use enums::*;
pub use mcp::*;
pub use normalized::*;
pub use result::*;
pub use source::*;
