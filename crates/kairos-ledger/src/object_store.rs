use kairos_core::model::{ActorKind, EventType, NormalizedEvent, NormalizedObject, ObjectKind, SourceName};
use kairos_core::{Instant, KairosResult};
use kairos_policy::ClosureJudge;
use kairos_store::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::cleaning::{canonical_time, cleaned_object};
use crate::repo::{EventRepo, ObjectRepo, ObjectRow, ObjectStamp};

const CHANGES_REQUESTED: &str = "CHANGES_REQUESTED";

/// 허용 필드만의 해시. 재처리와 변경 감지에 쓴다.
///
/// 입력은 `JSON.stringify([필드 값...])`의 UTF-8이다. 필드 순서는 `NormalizedObject`의 선언 순서이고
/// 없는 값은 null이다. 이 값이 어긋나면 운영의 모든 일정이 변경으로 잡혀 가짜 알림이 쏟아진다.
/// `Value::to_string`의 이스케이프는 `JSON.stringify`와 같다. 정제가 끝난 객체를 넘긴다.
pub fn snapshot_hash(object: &NormalizedObject) -> String {
    let text = |value: &Option<String>| value.as_deref().map_or(Value::Null, Value::from);
    let flag = |value: Option<bool>| value.map_or(Value::Null, Value::Bool);
    let values = vec![
        Value::from(object.object_key.as_str()),
        Value::from(object.source.as_str()),
        Value::from(object.kind.as_str()),
        Value::from(object.external_id.as_str()),
        text(&object.permalink),
        text(&object.actor_id),
        text(&object.channel_id),
        text(&object.ts),
        text(&object.text_head),
        object.trust_tier.map_or(Value::Null, |tier| Value::from(tier.as_str())),
        flag(object.all_day),
        text(&object.start_date),
        text(&object.end_date),
        text(&object.start_at),
        text(&object.end_at),
        text(&object.series_id),
        object.attendee_count.map_or(Value::Null, Value::from),
        text(&object.my_response),
        flag(object.busy),
        text(&object.state),
        text(&object.milestone_title),
        flag(object.assigned_to_me),
        flag(object.authored_by_me),
        flag(object.review_requested_me),
        flag(object.is_draft),
        text(&object.review_decision),
        text(&object.checks_state),
        text(&object.head_ref_oid),
    ];
    format!("{:x}", Sha256::digest(Value::Array(values).to_string()))
}

fn derived_from(object: &NormalizedObject, event_type: EventType, dedupe_key: String, at: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: object.actor_id.clone(),
        object_key: Some(object.object_key.clone()),
        channel_id: object.channel_id.clone(),
        permalink: object.permalink.clone(),
        text_head: object.text_head.clone(),
        trust_tier: object.trust_tier,
        ..NormalizedEvent::new(dedupe_key, object.source, event_type, at, ActorKind::Human)
    }
}

/// 일정의 시작을 비교할 수 있는 값으로 바꾼다. 종일은 날짜 그대로, 시간은 표기가 달라도 같은 순간이면 같은 값이다.
fn start_of(all_day: bool, start_date: Option<&str>, start_at: Option<&str>) -> KairosResult<Option<String>> {
    if all_day {
        return Ok(start_date.map(str::to_owned));
    }
    start_at.filter(|text| !text.is_empty()).map(canonical_time).transpose()
}

/// 일정 스냅샷 비교. 키에 시작 시각이 없으므로 이동이 같은 객체의 변경으로 잡힌다.
fn diff_calendar_object(
    prior: Option<&ObjectRow>,
    next: &NormalizedObject,
    hash: &str,
    at: &str,
) -> KairosResult<Option<NormalizedEvent>> {
    let key = &next.object_key;
    let cancelled = next.state.as_deref() == Some("cancelled");
    let Some(prior) = prior else {
        let created = derived_from(next, EventType::CalCreated, format!("{key}#created"), at);
        return Ok((!cancelled).then_some(created));
    };
    if prior.snapshot_hash == hash {
        return Ok(None);
    }
    if cancelled {
        let withdrawn = derived_from(next, EventType::CalCancelled, format!("{key}#cancelled"), at);
        return Ok((prior.state.as_deref() != Some("cancelled")).then_some(withdrawn));
    }
    let before = start_of(prior.all_day == Some(true), prior.start_date.as_deref(), prior.start_at.as_deref())?;
    let after = start_of(next.all_day == Some(true), next.start_date.as_deref(), next.start_at.as_deref())?;
    if before != after {
        // 키에 값이 없으면 JS 템플릿 문자열처럼 null로 찍힌다. 저장된 키와 같아야 재수집이 멱등이다.
        let shown = |value: &Option<String>| value.clone().unwrap_or_else(|| "null".to_owned());
        let dedupe_key = format!("{key}#moved:{}>{}", shown(&before), shown(&after));
        return Ok(Some(derived_from(next, EventType::CalMoved, dedupe_key, at)));
    }
    Ok(Some(derived_from(next, EventType::CalUpdated, format!("{key}#updated:{}", &hash[..16]), at)))
}

/// PR의 판정이 변경 요청을 벗어났다는 증거. reviewDecision은 시각이 없어 전이를 본 실행의 시각을 쓴다.
/// 필수 리뷰가 없는 저장소는 값이 비어 있으므로 None은 증거가 아니다.
fn diff_review_decision(
    prior: Option<&ObjectRow>,
    next: &NormalizedObject,
    run_id: i64,
    at: &str,
) -> Option<NormalizedEvent> {
    if prior?.review_decision.as_deref() != Some(CHANGES_REQUESTED) {
        return None;
    }
    let decision = next.review_decision.as_deref().filter(|decision| !decision.is_empty())?;
    if decision == CHANGES_REQUESTED {
        return None;
    }
    let dedupe_key = format!("{}#changes_cleared:{run_id}", next.object_key);
    Some(derived_from(next, EventType::GhChangesCleared, dedupe_key, at))
}

/// 객체를 저장할 실행의 맥락.
pub(crate) struct ObjectRun<'a> {
    pub query: &'a str,
    /// 첫 ok 전의 객체는 시작 전의 것이다.
    pub baseline: bool,
    pub run_id: i64,
    pub now: Instant,
}

/// 객체 저장과 스냅샷 비교.
pub(crate) struct ObjectStore<'a> {
    objects: ObjectRepo<'a>,
    events: EventRepo<'a>,
}

impl<'a> ObjectStore<'a> {
    pub(crate) const fn new(connection: &'a Connection) -> Self {
        Self { objects: ObjectRepo::new(connection), events: EventRepo::new(connection) }
    }

    /// 일정이 왕복하면 같은 변화의 키가 되풀이된다. 스냅샷이 달라졌을 때만 여기 오므로 재수집이 아니라 새 변화다.
    /// 키가 같으면 기록이 조용히 버리므로 실행 번호로 구분한다.
    fn distinct(&self, mut event: NormalizedEvent, run_id: i64) -> KairosResult<NormalizedEvent> {
        if self.events.stored_type(&event.dedupe_key)?.is_some() {
            event.dedupe_key = format!("{}:{run_id}", event.dedupe_key);
        }
        Ok(event)
    }

    /// 정제해서 저장하고, 이전 스냅샷과 달라서 생긴 이벤트가 있으면 돌려준다.
    pub(crate) fn store(&self, raw: &NormalizedObject, run: &ObjectRun<'_>) -> KairosResult<Option<NormalizedEvent>> {
        let object = cleaned_object(raw);
        let hash = snapshot_hash(&object);
        let prior = self.objects.get(&object.object_key)?;
        let at = run.now.to_iso();
        let stamp = ObjectStamp {
            query: run.query,
            hash: &hash,
            baseline: run.baseline,
            first_seen_at: &at,
            run_id: run.run_id,
        };
        self.objects.upsert(&object, &stamp)?;
        if matches!(object.kind, ObjectKind::Meeting | ObjectKind::Deploy) {
            match diff_calendar_object(prior.as_ref(), &object, &hash, &at)? {
                Some(event) if prior.is_some() => self.distinct(event, run.run_id).map(Some),
                change => Ok(change),
            }
        } else {
            Ok(diff_review_decision(prior.as_ref(), &object, run.run_id, &at))
        }
    }

    /// ok 스냅샷에서 빠진 객체를 센다. 2회 연속이면 일정에 한해 철회 증거를 만든다.
    pub(crate) fn mark_missing(
        &self,
        source: SourceName,
        query: &str,
        run_id: i64,
        now: Instant,
    ) -> KairosResult<Vec<NormalizedEvent>> {
        self.objects.count_missing(source, query, run_id)?;
        let at = now.to_iso();
        let withdrawn = self
            .objects
            .newly_missing_schedules(source, query)?
            .into_iter()
            .filter(|row| !ClosureJudge::has_ended(Some(&row.to_rule_object()), now))
            .map(|row| {
                let dedupe_key = format!("{}#withdrawn:{run_id}", row.object_key);
                NormalizedEvent {
                    object_key: Some(row.object_key),
                    channel_id: row.channel_id,
                    permalink: row.permalink,
                    text_head: row.text_head,
                    trust_tier: row.trust_tier,
                    ..NormalizedEvent::new(
                        dedupe_key,
                        source,
                        EventType::ObjectWithdrawn,
                        at.as_str(),
                        ActorKind::Human,
                    )
                }
            })
            .collect();
        Ok(withdrawn)
    }
}
