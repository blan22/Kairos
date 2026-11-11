use kairos_core::model::{ActorKind, EventType};

use crate::rule::{RuleEvent, RuleId};

/// 행위자 판정은 소스가 끝냈다. 봇 규칙을 타지 않으므로 멘션이 noise로 떨어지는 경로가 없다.
pub(super) fn rule(event: &RuleEvent) -> RuleId {
    if event.event_type == EventType::NotionPromo {
        return RuleId::NoiseNotionPromo;
    }
    // 멘션 문구는 정의상 남이 나를 부른 것이다. 표시 이름이 같아도 본인으로 지우지 않는다.
    if event.event_type == EventType::NotionMention {
        return RuleId::P1NotionMention;
    }
    if event.actor_kind == ActorKind::SelfActor {
        return RuleId::NoiseSelf;
    }
    if event.mentions_me {
        return RuleId::P1NotionMention;
    }
    match event.event_type {
        EventType::NotionUnknown => RuleId::P2NotionUnknown,
        EventType::NotionComment => RuleId::P2NotionComment,
        _ => RuleId::NoiseNotionEdit,
    }
}
