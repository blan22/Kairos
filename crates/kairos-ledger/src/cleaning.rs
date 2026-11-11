//! 저장 앞의 정제. 허용 필드의 문자열을 모두 정제하고 비면 None으로 둔다.

use kairos_core::model::{NormalizedEvent, NormalizedObject};
use kairos_core::text::{sanitize_external, sanitize_text};
use kairos_core::{Instant, KairosError, KairosResult};

pub(crate) const FIELD_LIMIT: usize = 512;

fn clean(value: Option<&str>) -> Option<String> {
    value.map(|text| sanitize_text(text, FIELD_LIMIT)).filter(|text| !text.is_empty())
}

/// 제목처럼 외부에서 온 앞부분. 링크와 주소도 가린다.
fn clean_head(value: Option<&str>) -> Option<String> {
    value.map(sanitize_external).filter(|text| !text.is_empty())
}

/// 필수 문자열이 정제로 비면 TS에서는 null이 되어 저장 단계의 NOT NULL에 걸린다. 빈 문자열로 그 자리를 지킨다.
pub(crate) fn cleaned_object(raw: &NormalizedObject) -> NormalizedObject {
    NormalizedObject {
        object_key: sanitize_text(&raw.object_key, FIELD_LIMIT),
        source: raw.source,
        kind: raw.kind,
        external_id: sanitize_text(&raw.external_id, FIELD_LIMIT),
        permalink: clean(raw.permalink.as_deref()),
        actor_id: clean(raw.actor_id.as_deref()),
        channel_id: clean(raw.channel_id.as_deref()),
        ts: clean(raw.ts.as_deref()),
        text_head: clean_head(raw.text_head.as_deref()),
        trust_tier: raw.trust_tier,
        all_day: raw.all_day,
        start_date: clean(raw.start_date.as_deref()),
        end_date: clean(raw.end_date.as_deref()),
        start_at: clean(raw.start_at.as_deref()),
        end_at: clean(raw.end_at.as_deref()),
        series_id: clean(raw.series_id.as_deref()),
        attendee_count: raw.attendee_count,
        my_response: clean(raw.my_response.as_deref()),
        busy: raw.busy,
        state: clean(raw.state.as_deref()),
        milestone_title: clean_head(raw.milestone_title.as_deref()),
        assigned_to_me: raw.assigned_to_me,
        authored_by_me: raw.authored_by_me,
        review_requested_me: raw.review_requested_me,
        is_draft: raw.is_draft,
        review_decision: clean(raw.review_decision.as_deref()),
        checks_state: clean(raw.checks_state.as_deref()),
        head_ref_oid: clean(raw.head_ref_oid.as_deref()),
    }
}

pub(crate) fn cleaned_event(raw: &NormalizedEvent) -> NormalizedEvent {
    NormalizedEvent {
        dedupe_key: sanitize_text(&raw.dedupe_key, FIELD_LIMIT),
        source: raw.source,
        event_type: raw.event_type,
        occurred_at: sanitize_text(&raw.occurred_at, FIELD_LIMIT),
        actor_kind: raw.actor_kind,
        actor_id: clean(raw.actor_id.as_deref()),
        object_key: clean(raw.object_key.as_deref()),
        thread_key: clean(raw.thread_key.as_deref()),
        mentions_me: raw.mentions_me,
        mention_count: raw.mention_count,
        channel_id: clean(raw.channel_id.as_deref()),
        permalink: clean(raw.permalink.as_deref()),
        text_head: clean_head(raw.text_head.as_deref()),
        trust_tier: raw.trust_tier,
    }
}

/// `new Date(text).toISOString()`. 해석할 수 없으면 TS처럼 실패한다.
pub(crate) fn canonical_time(text: &str) -> KairosResult<String> {
    Instant::parse(text).map(Instant::to_iso).ok_or_else(|| KairosError::new("invalid_time"))
}
