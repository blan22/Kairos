//! 일정 응답을 객체로 바꾼다.

use kairos_core::js;
use kairos_core::local_date::MAX_OFF_SPAN_DAYS;
use kairos_core::model::{NormalizedObject, ObjectKind, SourceName, TrustTier};
use serde_json::Value;

use super::dates::{add_days, all_day_date};
use super::{DEPLOY, HOLIDAYS, PERSONAL, deploy_tag, personal_calendar, self_name, text};
use crate::shared::{ParseFailure, trust_of};

const PUBLIC_HOLIDAY: &str = "공휴일";

fn is_leave_title(title: &str) -> bool {
    title.contains("연차") || title.contains("휴가")
}

fn is_part_day_title(title: &str) -> bool {
    ["반차", "오전", "오후"].iter().any(|word| title.contains(word))
}

/// 설계 8절의 구독 중인 그룹 캘린더. 일정 주최자 판정에만 쓴다. Gmail과 Slack 주소에는 쓰지 않는다.
fn is_trusted_calendar(address: &str) -> bool {
    kairos_core::profile::current().org.is_trusted_calendar(address)
}

fn org_calendar(pick: impl Fn(&'static kairos_core::profile::Org) -> &'static Option<String>) -> &'static str {
    pick(&kairos_core::profile::current().org).as_deref().unwrap_or_default()
}

fn field<'a>(event: &'a Value, name: &str, inner: &str) -> Option<&'a Value> {
    event.get(name).and_then(|value| value.get(inner))
}

fn fill_times(object: &mut NormalizedObject, event: &Value) -> Result<(), ParseFailure> {
    let Some(start_date) = text(field(event, "start", "date")) else {
        object.all_day = Some(false);
        object.start_at = text(field(event, "start", "dateTime")).map(str::to_owned);
        object.end_at = text(field(event, "end", "dateTime")).map(str::to_owned);
        return Ok(());
    };
    let start = all_day_date(start_date)?;
    object.end_date = Some(match text(field(event, "end", "date")) {
        Some(end_date) => all_day_date(end_date)?,
        None => add_days(&start, 1).ok_or(ParseFailure::new("calendar_date"))?,
    });
    object.all_day = Some(true);
    object.start_date = Some(start);
    Ok(())
}

/// 참석자 이메일은 저장하지 않는다. 수와 내 응답 상태만 남긴다.
fn fill_attendance(object: &mut NormalizedObject, event: &Value) {
    let listed = event.get("attendees").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let attendees: Vec<&Value> = listed.iter().filter(|attendee| !attendee.is_null()).collect();
    let is_true = |attendee: &Value, name: &str| attendee.get(name) == Some(&Value::Bool(true));
    let mine = attendees.iter().find(|attendee| is_true(attendee, "self"));
    let response = mine.and_then(|attendee| text(attendee.get("responseStatus")));
    object.attendee_count = Some(attendees.iter().filter(|attendee| !is_true(attendee, "resource")).count() as i64);
    object.my_response = Some(response.unwrap_or("accepted").to_owned());
}

fn trust_of_organizer(address: Option<&str>) -> TrustTier {
    match address {
        Some(address) if is_trusted_calendar(&address.to_lowercase()) => TrustTier::T1,
        _ => trust_of(address),
    }
}

/// 주최자 주소는 등급 판정에만 쓰고 저장하지 않는다.
fn trust_of_event(event: &Value) -> TrustTier {
    if field(event, "organizer", "self") == Some(&Value::Bool(true)) {
        return TrustTier::T0;
    }
    trust_of_organizer(text(field(event, "organizer", "email")))
}

fn is_free(event: &Value) -> bool {
    event.get("transparency").and_then(Value::as_str) == Some("transparent")
        || event.get("availability").and_then(Value::as_str) == Some("AVAILABILITY_FREE")
}

fn base(event: &Value, kind: ObjectKind) -> Result<NormalizedObject, ParseFailure> {
    let id = text(event.get("id")).ok_or(ParseFailure::new("calendar_event_id"))?;
    let mut object = NormalizedObject {
        text_head: text(event.get("summary")).map(str::to_owned),
        state: Some(text(event.get("status")).unwrap_or("confirmed").to_owned()),
        series_id: text(event.get("recurringEventId")).map(str::to_owned),
        ..NormalizedObject::new(format!("gcal:{id}"), SourceName::Calendar, kind, id)
    };
    fill_times(&mut object, event)?;
    Ok(object)
}

/// 제목은 초대를 보낸 쪽이 정한다. [배포]는 배포 규칙(P0까지 간다)에 들이므로 주최자를 적극적으로 믿을 때만 배포다.
/// 믿는 주최자는 둘이다. organizer.self(이 일정이 보이는 캘린더가 주최자다. 개인 캘린더에서는 나, 배포 캘린더에서는
/// 배포 캘린더 자신)와 T1으로 판정되는 주소(신뢰 도메인과 목록의 그룹 캘린더)다. 주최자가 없거나 주소가 비었으면
/// 누가 만들었는지 모른다. 외부 주최자와 같이 배포로 보지 않는다.
fn deploy_tier(event: &Value) -> Option<TrustTier> {
    if !text(event.get("summary")).is_some_and(|title| title.contains(deploy_tag())) {
        return None;
    }
    let is_self = field(event, "organizer", "self") == Some(&Value::Bool(true));
    let organizer = trust_of_organizer(text(field(event, "organizer", "email")));
    (is_self || organizer == TrustTier::T1).then_some(TrustTier::T1)
}

/// [배포] 일정은 어느 캘린더에서 보이든 같은 값을 낸다. 캘린더마다 달라지는 값을 두면
/// 같은 객체의 해시가 번갈아 바뀌어 변경으로 잡힌다. 등급은 deploy_tier가 믿은 주최자에서만 온다.
fn to_deploy(event: &Value, tier: TrustTier) -> Result<NormalizedObject, ParseFailure> {
    Ok(NormalizedObject { trust_tier: Some(tier), ..base(event, ObjectKind::Deploy)? })
}

fn to_meeting(event: &Value) -> Result<NormalizedObject, ParseFailure> {
    let mut object = base(event, ObjectKind::Meeting)?;
    fill_attendance(&mut object, event);
    object.channel_id = Some(personal_calendar().to_owned());
    object.permalink = text(event.get("htmlLink")).map(str::to_owned);
    object.busy = Some(!is_free(event));
    object.trust_tier = Some(trust_of_event(event));
    Ok(object)
}

fn to_day_off(event: &Value, calendar: &str) -> Result<NormalizedObject, ParseFailure> {
    Ok(NormalizedObject {
        channel_id: Some(calendar.to_owned()),
        trust_tier: Some(TrustTier::T1),
        ..base(event, ObjectKind::DayOff)?
    })
}

/// 대한민국의 휴일에는 기념일도 섞여 있다. description 첫 줄이 공휴일인 것만 쉬는 날이다(측정).
fn is_public_holiday(event: &Value) -> bool {
    let first_line = text(event.get("description")).unwrap_or("").split('\n').next().unwrap_or("");
    js::trim(first_line) == PUBLIC_HOLIDAY
}

fn is_my_leave(event: &Value) -> bool {
    let title = text(event.get("summary")).unwrap_or("");
    text(field(event, "start", "date")).is_some()
        && title.contains(self_name())
        && is_leave_title(title)
        && !is_part_day_title(title)
}

/// 쉬는 날은 수집과 감시를 바꾼다. 비정상적으로 긴 기간은 믿지 않는다.
/// 시간 일정이 공휴일 설명을 달고 오면 날짜가 없다. 하루를 쉬는 일정이 아니다.
fn is_plausible_off(object: &NormalizedObject) -> bool {
    let (Some(start), Some(end)) = (&object.start_date, &object.end_date) else {
        return false;
    };
    // 9999년을 넘는 상한은 `+0`으로 시작해 어떤 날짜보다 앞선다.
    add_days(start, MAX_OFF_SPAN_DAYS).is_some_and(|limit| *end <= limit)
}

fn select<'a>(events: &[&'a Value], chosen: fn(&Value) -> bool) -> Vec<&'a Value> {
    events.iter().copied().filter(|event| chosen(event)).collect()
}

fn off_days(events: &[&Value], calendar: &str) -> Result<Vec<NormalizedObject>, ParseFailure> {
    let objects = events.iter().map(|event| to_day_off(event, calendar)).collect::<Result<Vec<_>, _>>()?;
    Ok(objects.into_iter().filter(is_plausible_off).collect())
}

/// null 일정은 호출자가 걸러 낸다.
pub(super) fn to_objects(query: &str, events: &[&Value]) -> Result<Vec<NormalizedObject>, ParseFailure> {
    match query {
        PERSONAL => events
            .iter()
            .map(|event| match deploy_tier(event) {
                Some(tier) => to_deploy(event, tier),
                None => to_meeting(event),
            })
            .collect(),
        // 배포 캘린더에서도 믿지 못하는 주최자의 [배포]는 버린다. 개인 캘린더에 초대로 오면 그쪽에서 일반 일정이 된다.
        DEPLOY => events.iter().filter_map(|event| deploy_tier(event).map(|tier| to_deploy(event, tier))).collect(),
        HOLIDAYS => off_days(&select(events, is_public_holiday), org_calendar(|org| &org.holiday_calendar)),
        _ => off_days(&select(events, is_my_leave), org_calendar(|org| &org.leave_calendar)),
    }
}
