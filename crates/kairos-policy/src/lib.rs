//! 판단 규칙: 등급, 종료, 배포 연결, 근무 시간과 예산.
//!
//! 설계 5절의 등급 규칙과 4.4절의 종료 규칙, 6.3절과 6.5절의 시간과 예산 값을 둔다.
//! DB와 시계, 모델 출력에 의존하지 않는 순수 함수만 둔다.
//! 어떤 모델 출력도 입력으로 받지 않으므로 P0는 규칙으로만 만들어진다.
//! 판정 결과는 표시되지 않고 `events.disposition`, `events.rule_id`로 기록된다.

mod budget;
mod classifier;
mod closure;
mod deploy;
mod rule;
mod schedule;

pub use budget::*;
pub use classifier::{Classification, Classifier, ClassifyInput, MEETING_WINDOW_MS, MULTI_MENTION_THRESHOLD};
pub use closure::{CloseReason, ClosureDecision, ClosureInput, ClosureJudge, EvidenceEvent, OpenedEvent};
pub use deploy::{DeployCandidate, DeployLink, DeployLinkStatus, DeployLinker, DeployStage};
pub use rule::{Disposition, ItemReason, RuleEvent, RuleId, RuleObject, Tier, tier_of};
pub use schedule::WorkSchedule;
