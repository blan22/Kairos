//! 설계 4.4절의 종료 규칙.

use kairos_core::Instant;
use kairos_core::model::{ActorKind, EventType, Outcome};
use kairos_core::string_enum;

use crate::rule::{ItemReason, RuleId, RuleObject};

string_enum! {
    pub enum CloseReason {
        DoneEvidence => "done_evidence",
        Withdrawn => "withdrawn",
        Expired => "expired",
    }
}

/// 종료 증거 후보. 시각은 저장된 원문이다.
#[derive(Clone, Debug, PartialEq)]
pub struct EvidenceEvent {
    pub dedupe_key: String,
    pub event_type: EventType,
    pub actor_kind: ActorKind,
    pub actor_id: Option<String>,
    pub thread_key: String,
    pub occurred_at: String,
}

/// 닫을지 판정할 이벤트. `rule_id`는 저장된 문자열이고 모르는 규칙은 닫지 않는다.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenedEvent {
    pub rule_id: String,
    pub thread_key: String,
    pub occurred_at: String,
    pub actor_id: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct ClosureInput<'a> {
    /// 이 판정을 일으킨 실행의 결과. ok가 아니면 어떤 종료도 만들지 않는다.
    pub run_outcome: Outcome,
    pub opened: &'a OpenedEvent,
    /// 같은 객체나 스레드의 이벤트. 결과에 없다는 사실은 입력이 될 수 없다.
    pub candidates: &'a [EvidenceEvent],
    pub object: Option<&'a RuleObject>,
    pub now: Instant,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClosureDecision {
    KeepOpen,
    /// `evidence`는 expired에서만 None이다.
    Close {
        reason: CloseReason,
        evidence: Option<String>,
    },
}

/// 증거 후보 하나가 닫는 사유. 사유마다 인정하는 증거가 다르다.
fn evidence_reason(reason: ItemReason, opened: &OpenedEvent, candidate: &EvidenceEvent) -> Option<CloseReason> {
    let kind = candidate.event_type;
    let by_me = candidate.actor_kind == ActorKind::SelfActor;
    let done = |proven: bool| proven.then_some(CloseReason::DoneEvidence);
    match reason {
        ItemReason::ReplyOwed => {
            let answered = matches!(
                kind,
                EventType::GhComment | EventType::GhReview | EventType::GhChangesRequested | EventType::SlackMessage
            );
            done(candidate.thread_key == opened.thread_key && by_me && answered)
        }
        ItemReason::ReviewOwed => {
            if kind == EventType::GhReviewRequestRemoved {
                return Some(CloseReason::Withdrawn);
            }
            let my_review = by_me && matches!(kind, EventType::GhReview | EventType::GhChangesRequested);
            done(my_review || matches!(kind, EventType::GhClosed | EventType::GhMerged))
        }
        ItemReason::Assigned => {
            if kind == EventType::GhUnassigned {
                return Some(CloseReason::Withdrawn);
            }
            done(kind == EventType::GhClosed)
        }
        ItemReason::MyPrAttention => {
            if matches!(kind, EventType::GhMerged | EventType::GhClosed) {
                return Some(CloseReason::DoneEvidence);
            }
            if opened.rule_id == RuleId::P1CiFailed.as_str() {
                return done(kind == EventType::GhCiRecovered);
            }
            // 코멘트 리뷰와 다른 사람의 리뷰는 재리뷰가 아니다. 변경 요청을 낸 사람의 승인이나 PR 판정 변화만 증거다.
            if kind == EventType::GhChangesCleared {
                return Some(CloseReason::DoneEvidence);
            }
            done(
                kind == EventType::GhReviewApproved
                    && opened.actor_id.is_some()
                    && candidate.actor_id == opened.actor_id,
            )
        }
        ItemReason::MeetingNew => {
            matches!(kind, EventType::CalCancelled | EventType::ObjectWithdrawn).then_some(CloseReason::Withdrawn)
        }
        ItemReason::DeployRisk => (kind == EventType::DeployCleared).then_some(CloseReason::Withdrawn),
        ItemReason::WorkOwed | ItemReason::Manual => None,
    }
}

/// 종료 판정기.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClosureJudge;

impl ClosureJudge {
    /// 일정이 끝났는가. 종일 일정은 끝 날짜(제외)로 판단한다.
    pub fn has_ended(object: Option<&RuleObject>, now: Instant) -> bool {
        let Some(object) = object else {
            return false;
        };
        if object.all_day {
            // 둘 다 YYYY-MM-DD이고 저장된 값은 원문이라 문자열로 비교한다.
            return object.end_date.as_deref().is_some_and(|end| now.local_date().to_string().as_str() >= end);
        }
        let end = object.end_at.as_deref().or(object.start_at.as_deref());
        end.and_then(Instant::parse).is_some_and(|end| end <= now)
    }

    /// 종료는 양성 증거로만 한다. 증거는 opened보다 늦은 이벤트여야 한다.
    /// 시각 경과(expired)는 일정 종료 시각이 지났다는 사실 자체가 근거이고 meeting_new, deploy_risk에만 있다.
    /// work_owed는 사용자 조작으로만 닫힌다.
    pub fn judge(input: &ClosureInput<'_>) -> ClosureDecision {
        if input.run_outcome != Outcome::Ok {
            return ClosureDecision::KeepOpen;
        }
        let Some(reason) = input.opened.rule_id.parse().ok().and_then(RuleId::reason) else {
            return ClosureDecision::KeepOpen;
        };
        let opened_at = Instant::parse(&input.opened.occurred_at);
        for candidate in input.candidates {
            // 해석할 수 없는 시각은 NaN이라 `<=`가 거짓이고 건너뛰지 않는다.
            if let (Some(at), Some(opened_at)) = (Instant::parse(&candidate.occurred_at), opened_at)
                && at <= opened_at
            {
                continue;
            }
            if let Some(reason) = evidence_reason(reason, input.opened, candidate) {
                return ClosureDecision::Close { reason, evidence: Some(candidate.dedupe_key.clone()) };
            }
        }
        if matches!(reason, ItemReason::MeetingNew | ItemReason::DeployRisk) && Self::has_ended(input.object, input.now)
        {
            return ClosureDecision::Close { reason: CloseReason::Expired, evidence: None };
        }
        ClosureDecision::KeepOpen
    }
}
