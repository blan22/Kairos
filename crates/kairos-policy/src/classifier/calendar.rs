use kairos_core::Instant;
use kairos_core::model::{EventType, ObjectKind};

use super::MEETING_WINDOW_MS;
use crate::rule::{RuleEvent, RuleId, RuleObject};

/// 빈 문자열은 JS에서 거짓이다.
fn present(value: Option<&str>) -> Option<&str> {
    value.filter(|text| !text.is_empty())
}

/// 끝나지 않았고 시작이 60분 안이면 참이다. 수집 지연으로 막 시작한 회의도 놓치지 않는다.
fn starts_soon(object: &RuleObject, now: Instant) -> bool {
    if object.all_day {
        return false;
    }
    let Some(start) = present(object.start_at.as_deref()).and_then(Instant::parse) else {
        return false;
    };
    // 해석할 수 없는 끝 시각은 NaN이라 어떤 비교도 거짓이다.
    let end = present(object.end_at.as_deref()).map_or(Some(start), Instant::parse);
    start.ms_since(now) <= MEETING_WINDOW_MS && end.is_some_and(|end| end > now)
}

pub(super) fn rule(event: &RuleEvent, object: Option<&RuleObject>, now: Instant) -> RuleId {
    let Some(object) = object else {
        return RuleId::NoiseLifecycle;
    };
    match object.kind {
        ObjectKind::Deploy => return RuleId::NoiseTeamCalendar,
        ObjectKind::Meeting => {}
        _ => return RuleId::NoiseLifecycle,
    }
    if matches!(event.event_type, EventType::CalCancelled | EventType::ObjectWithdrawn) {
        return RuleId::P2MeetingChange;
    }
    if object.my_response.as_deref() == Some("declined") {
        return RuleId::NoiseMeetingDeclined;
    }
    if object.busy == Some(false) {
        return RuleId::NoiseMeetingFree;
    }
    if event.event_type == EventType::CalUpdated {
        return if object.my_response.as_deref() == Some("needsAction") {
            RuleId::NoiseNeedsAction
        } else {
            RuleId::NoiseMeetingUpdate
        };
    }
    // P0는 응답 상태와 무관하다. needsAction이어도 새로 생긴 임박 회의는 놓치면 안 된다.
    if starts_soon(object, now) { RuleId::P0MeetingImminent } else { RuleId::P2MeetingChange }
}
