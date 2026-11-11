//! 레코드를 이벤트와 객체로 바꾼다.

use std::collections::HashSet;
use std::sync::LazyLock;

use kairos_core::model::{ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, TrustTier};
use kairos_core::text::sanitize_head;
use regex::Regex;

use super::{SOURCE, SlackRecord, me};
use crate::shared::{last_by_key, trust_of};

static USER_MENTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<@([A-Z0-9]+)(?:\|[^>]*)?>").expect("상수 정규식"));

/// 종료 증거를 찾는 단위. DM은 대화 전체, 그 외는 스레드다.
fn thread_key_of(record: &SlackRecord) -> String {
    match &record.thread_ts {
        _ if record.is_dm => format!("slack:{}", record.channel_id),
        Some(thread_ts) => format!("slack:{}:{thread_ts}", record.channel_id),
        None => format!("slack:{}:{}", record.channel_id, record.ts),
    }
}

fn common_event(record: &SlackRecord, dedupe_key: String, event_type: EventType) -> NormalizedEvent {
    let (actor_kind, trust_tier) = if record.user_id == me() {
        (ActorKind::SelfActor, TrustTier::T0)
    } else {
        (ActorKind::Human, trust_of(record.email.as_deref()))
    };
    NormalizedEvent {
        actor_id: Some(record.user_id.clone()),
        thread_key: Some(thread_key_of(record)),
        channel_id: Some(record.channel_id.clone()),
        trust_tier: Some(trust_tier),
        ..NormalizedEvent::new(dedupe_key, SOURCE, event_type, record.at.to_iso(), actor_kind)
    }
}

pub(super) fn mention_event(record: &SlackRecord) -> NormalizedEvent {
    let mentioned: HashSet<&str> =
        USER_MENTION.captures_iter(&record.text).filter_map(|found| found.get(1)).map(|found| found.as_str()).collect();
    let is_me = mentioned.contains(me());
    let dedupe_key = format!("slack:{}:{}", record.channel_id, record.ts);
    NormalizedEvent {
        // 검색 결과를 믿지 않고 본문에서 다시 확인한다. 없으면 기록만 남는다.
        mentions_me: Some(is_me),
        mention_count: Some(mentioned.len() as i64),
        permalink: Some(record.permalink.clone()),
        // DM 본문은 어떤 경로로도 남기지 않는다(설계 3.4). 내가 지목되지 않은 글도 남길 이유가 없다.
        text_head: (!record.is_dm && is_me).then(|| sanitize_head(&record.text)),
        ..common_event(record, dedupe_key, EventType::SlackMention)
    }
}

/// 본인 메시지는 종료 증거일 뿐이라 본문을 남기지 않는다.
pub(super) fn own_message_event(record: &SlackRecord) -> NormalizedEvent {
    let dedupe_key = format!("slack:{}:{}", record.channel_id, record.ts);
    NormalizedEvent {
        permalink: Some(record.permalink.clone()),
        ..common_event(record, dedupe_key, EventType::SlackMessage)
    }
}

/// 기록 전용(설계 3.4). 키를 나누지 않으면 같은 메시지의 멘션 이벤트가 중복으로 버려진다.
pub(super) fn dm_event(record: &SlackRecord) -> NormalizedEvent {
    let dedupe_key = format!("slack:dm:{}:{}", record.channel_id, record.ts);
    NormalizedEvent {
        object_key: Some(format!("slack:dm:{}", record.channel_id)),
        ..common_event(record, dedupe_key, EventType::SlackDmSeen)
    }
}

/// 대화 ID, 마지막 발화자, 시각만 둔다. 결과가 오름차순이라 뒤의 값이 마지막이다.
pub(super) fn dm_objects(records: &[SlackRecord]) -> Vec<NormalizedObject> {
    let latest = last_by_key(records.iter().map(|record| (record.channel_id.as_str(), record)));
    latest
        .into_iter()
        .map(|record| {
            let object_key = format!("slack:dm:{}", record.channel_id);
            NormalizedObject {
                actor_id: Some(record.user_id.clone()),
                channel_id: Some(record.channel_id.clone()),
                ts: Some(record.at.to_iso()),
                ..NormalizedObject::new(object_key, SOURCE, ObjectKind::DmConversation, record.channel_id.as_str())
            }
        })
        .collect()
}
