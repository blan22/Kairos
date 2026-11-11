//! 이벤트 1건의 등급 판정. 소스 묶음마다 전략이 다르다.

mod activity;
mod calendar;
mod mail;
mod notion;

use kairos_core::Instant;
use kairos_core::model::{ActorKind, EventType};

use crate::rule::{Disposition, RuleEvent, RuleId, RuleObject, Tier};

pub const MEETING_WINDOW_MS: i64 = 60 * 60 * 1000;
pub const MULTI_MENTION_THRESHOLD: i64 = 3;

#[derive(Clone, Copy, Debug)]
pub struct ClassifyInput<'a> {
    pub event: &'a RuleEvent,
    pub object: Option<&'a RuleObject>,
    /// 판정 시각.
    pub now: Instant,
    /// 이 객체가 다가오는 [배포]에 연결돼 있는가.
    pub deploy_linked: bool,
    /// 같은 객체에서 아직 답하지 않은 opened 이벤트의 규칙. 객체당 reply_owed 1건으로 접는 데 쓴다.
    pub open_rule_ids: &'a [String],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Classification {
    pub disposition: Disposition,
    pub rule_id: RuleId,
}

fn judged(rule_id: RuleId) -> Classification {
    let disposition = match (rule_id, rule_id.tier()) {
        (RuleId::Unclassified, _) => Disposition::Unclassified,
        (_, Tier::Record) => Disposition::Noise,
        _ => Disposition::Opened,
    };
    Classification { disposition, rule_id }
}

/// 이벤트 판정기. 같은 입력이면 항상 같은 결과를 낸다.
#[derive(Clone, Copy, Debug, Default)]
pub struct Classifier;

impl Classifier {
    pub fn classify(input: &ClassifyInput<'_>) -> Classification {
        let event = input.event;
        match event.event_type {
            EventType::CalCreated
            | EventType::CalMoved
            | EventType::CalUpdated
            | EventType::CalCancelled
            | EventType::ObjectWithdrawn => judged(calendar::rule(event, input.object, input.now)),
            EventType::DeployRiskD0 => judged(RuleId::P0DeployDay),
            EventType::DeployRiskD1 => judged(RuleId::P1DeployD1),
            EventType::DeployCleared => judged(RuleId::NoiseLifecycle),
            EventType::MailThread => judged(mail::rule(event)),
            EventType::NotionMention
            | EventType::NotionComment
            | EventType::NotionEdit
            | EventType::NotionPromo
            | EventType::NotionUnknown => judged(notion::rule(event)),
            EventType::SlackDmSeen => judged(RuleId::NoiseDmLog),
            EventType::SlackMessage => judged(if event.actor_kind == ActorKind::SelfActor {
                RuleId::NoiseSelf
            } else {
                RuleId::NoiseLifecycle
            }),
            _ => {
                let rule_id = activity::rule(event, input.object, input.deploy_linked);
                let foldable = matches!(rule_id, RuleId::P2OwnedComment | RuleId::P1DeployOwnedComment);
                if foldable && input.open_rule_ids.iter().any(|open| open == rule_id.as_str()) {
                    Classification { disposition: Disposition::Folded, rule_id }
                } else {
                    judged(rule_id)
                }
            }
        }
    }
}
