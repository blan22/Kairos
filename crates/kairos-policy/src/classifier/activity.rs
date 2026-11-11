use kairos_core::model::{ActorKind, EventType};

use super::MULTI_MENTION_THRESHOLD;
use crate::rule::{RuleEvent, RuleId, RuleObject};

fn is_owned(object: Option<&RuleObject>) -> bool {
    object.is_some_and(|object| object.assigned_to_me || object.authored_by_me)
}

pub(super) fn rule(event: &RuleEvent, object: Option<&RuleObject>, deploy_linked: bool) -> RuleId {
    let authored_by_me = object.is_some_and(|object| object.authored_by_me);
    // CI 실패는 행위자와 무관하다. 봇이 승격되는 유일한 경로이고, 내 푸시로 찍혀도 놓치면 안 된다.
    if event.event_type == EventType::GhCiFailed {
        return if authored_by_me { RuleId::P1CiFailed } else { RuleId::NoiseBot };
    }
    match event.actor_kind {
        ActorKind::SelfActor => return RuleId::NoiseSelf,
        ActorKind::Bot => return RuleId::NoiseBot,
        ActorKind::Human => {}
    }

    match event.event_type {
        EventType::GhChangesRequested if authored_by_me => return RuleId::P1ChangesRequested,
        EventType::GhChangesRequested | EventType::GhComment | EventType::GhReview | EventType::SlackMention => {}
        EventType::GhReviewRequested => return RuleId::P2ReviewRequested,
        EventType::GhReviewRerequested => return RuleId::P1ReviewRerequested,
        EventType::GhAssigned => return RuleId::P2Assigned,
        _ => return RuleId::NoiseLifecycle,
    }

    if event.mentions_me {
        return if event.mention_count >= MULTI_MENTION_THRESHOLD {
            RuleId::P2MultiMention
        } else {
            RuleId::P1DirectMention
        };
    }
    if is_owned(object) {
        return if deploy_linked { RuleId::P1DeployOwnedComment } else { RuleId::P2OwnedComment };
    }
    RuleId::NoiseInvolves
}
