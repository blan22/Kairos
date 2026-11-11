use std::sync::LazyLock;

use kairos_core::Instant;
use kairos_core::local_date::is_date_only;
use kairos_core::model::{EventType, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, SourceName, SourceResult};
use regex::Regex;

static OFFSET_TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?:[Zz]|[+-][0-9]{2}:[0-9]{2})$").expect("상수 정규식"));

/// 검증 뒤에 실제로 기록할 값. 소스가 낸 값과 다를 수 있다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verdict {
    pub outcome: Outcome,
    pub error_code: Option<String>,
    pub store: bool,
}

impl Verdict {
    pub(crate) fn fail(outcome: Outcome, error_code: &str) -> Self {
        Self { outcome, error_code: Some(error_code.to_owned()), store: false }
    }
}

fn object_breach(object: &NormalizedObject, source: SourceName) -> Option<&'static str> {
    if object.source != source || object.object_key.is_empty() || object.external_id.is_empty() {
        return Some("bad_object");
    }
    if object.kind == ObjectKind::DmConversation && object.text_head.is_some() {
        return Some("forbidden_field");
    }
    if object.all_day == Some(true) {
        let is_date = |value: &Option<String>| value.as_deref().is_some_and(is_date_only);
        if !is_date(&object.start_date) || !is_date(&object.end_date) {
            return Some("bad_all_day");
        }
        if object.start_at.is_some() || object.end_at.is_some() {
            return Some("bad_all_day");
        }
    } else if object.start_at.as_deref().is_some_and(|start| !ContractGuard::is_timestamp(start)) {
        return Some("bad_timed_event");
    }
    None
}

fn event_breach(event: &NormalizedEvent, source: SourceName) -> Option<&'static str> {
    if event.source != source || event.event_type.is_derived() || event.dedupe_key.is_empty() {
        return Some("bad_event");
    }
    if !ContractGuard::is_timestamp(&event.occurred_at) {
        return Some("bad_event");
    }
    if event.event_type == EventType::SlackDmSeen && event.text_head.is_some() {
        return Some("forbidden_field");
    }
    None
}

/// 소스가 낸 outcome을 믿기 전에 계약을 검사한다. 어기면 저장하지 않고 watermark도 움직이지 않는다.
///
/// 허용 키 검사는 없다. 소스 결과가 구조체라 허용 밖 필드가 애초에 생기지 않는다.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContractGuard;

impl ContractGuard {
    /// 오프셋이 있고 해석할 수 있는 시각인가.
    pub fn is_timestamp(value: &str) -> bool {
        OFFSET_TIMESTAMP.is_match(value) && Instant::parse(value).is_some()
    }

    pub fn judge(result: &SourceResult) -> Verdict {
        if !result.outcome.stores_records() {
            return Verdict { outcome: result.outcome, error_code: result.error_code.clone(), store: false };
        }
        if !Self::is_timestamp(&result.started_at) || !Self::is_timestamp(&result.finished_at) {
            return Verdict::fail(Outcome::ContractBreak, "bad_result");
        }
        if result.watermark.as_deref().is_some_and(|watermark| !Self::is_timestamp(watermark)) {
            return Verdict::fail(Outcome::ContractBreak, "bad_watermark");
        }
        let breach = result
            .objects
            .iter()
            .find_map(|object| object_breach(object, result.source))
            .or_else(|| result.events.iter().find_map(|event| event_breach(event, result.source)));
        if let Some(breach) = breach {
            return Verdict::fail(Outcome::ContractBreak, breach);
        }
        if let (Some(identity), Some(expected)) = (result.identity.as_deref(), result.source.self_identity())
            && identity != expected
        {
            return Verdict::fail(Outcome::ContractBreak, "identity_mismatch");
        }
        if result.outcome == Outcome::Ok
            && result.expected_count.is_some_and(|expected| expected != result.parsed_count)
        {
            return Verdict::fail(Outcome::ParseError, "count_mismatch");
        }
        Verdict { outcome: result.outcome, error_code: result.error_code.clone(), store: true }
    }
}
