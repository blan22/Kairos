#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{McpSource, NormalizedObject, ObjectKind, Outcome, SourceResult, TrustTier};
use kairos_source_mcp::CalendarSource;
use kairos_source_mcp::calendar::deploy_calendar;
use regex::Regex;
use serde_json::{Value, json};

const NOW: &str = T0;

fn source() -> CalendarSource {
    CalendarSource::new(clock())
}

fn load(name: &str) -> Value {
    json_fixture(&format!("calendar/{name}.json"))
}

fn parse_as_at(query: &str, body: &Value, now: &str) -> SourceResult {
    let source = source();
    parse_one(&source, &[json_response(&format!("calendar:{query}"), body)], &context_of(&source, now, vec![]))
}

fn parse_as(query: &str, body: &Value) -> SourceResult {
    parse_as_at(query, body, NOW)
}

fn by_id<'a>(result: &'a SourceResult, id: &str) -> &'a NormalizedObject {
    result.objects.iter().find(|object| object.external_id == id).unwrap()
}

fn with(mut body: Value, key: &str, value: Value) -> Value {
    body[key] = value;
    body
}

fn meeting_body(events: Value) -> Value {
    json!({ "accessRole": "owner", "summary": SELF_EMAIL, "events": events })
}

fn new_meeting() -> Value {
    json!({
        "id": "syntheticnew",
        "status": "confirmed",
        "summary": "새 회의",
        "start": { "dateTime": "2026-03-02T12:30:00+09:00" },
        "end": { "dateTime": "2026-03-02T13:00:00+09:00" },
        "attendees": [{ "self": true, "responseStatus": "needsAction" }],
    })
}

fn invite(organizer: Value, summary: &str) -> Value {
    json!({
        "id": "syntheticinvite",
        "status": "confirmed",
        "summary": summary,
        "start": { "dateTime": "2026-03-03T12:30:00+09:00" },
        "end": { "dateTime": "2026-03-03T13:00:00+09:00" },
        "organizer": organizer,
    })
}

#[test]
fn 네_캘린더를_오프셋_없는_현지_시각으로_조회한다() {
    let source = source();
    let calls = source.plan(&context_of(&source, NOW, vec![]));
    let queries: Vec<&str> = calls.iter().map(|call| call.query.as_str()).collect();
    assert_eq!(queries, ["personal", "deploy", "holidays", "crew_off"]);
    assert_eq!(calls[0].args["calendarId"], SELF_EMAIL);
    assert_eq!(calls[0].args["startTime"], "2026-03-02T09:00:00");
    assert_eq!(calls[0].args["endTime"], "2026-03-16T10:00:00");
    assert_eq!(calls[1].args["fullText"], "[배포]");
    assert_eq!(calls[2].args["startTime"], "2026-03-02T00:00:00");
    let zoned = Regex::new(r"Z|[+-][0-9]{2}:[0-9]{2}$").unwrap();
    for call in &calls {
        assert_eq!(call.server, "claude.ai Google Calendar");
        assert_eq!(call.tool, "list_events");
        assert!(!zoned.is_match(call.args["startTime"].as_str().unwrap()));
    }
}

#[test]
fn 종일_일정은_앞_10자_날짜로만_다룬다() {
    let result = parse_as("personal", &load("personal"));
    let object = by_id(&result, "syntheticallday1");
    assert_eq!(object.all_day, Some(true));
    assert_eq!(object.start_date.as_deref(), Some("2026-03-05"));
    assert_eq!(object.end_date.as_deref(), Some("2026-03-06"));
    assert_eq!((object.start_at.as_deref(), object.end_at.as_deref()), (None, None));
    assert_eq!(object.busy, Some(false));
    assert_eq!(object.series_id.as_deref(), Some("syntheticseries"));
}

#[test]
fn 시간_일정은_오프셋을_유지한다() {
    let result = parse_as("personal", &load("personal"));
    let object = by_id(&result, "syntheticmeeting1");
    assert_eq!(object.all_day, Some(false));
    assert_eq!(object.start_at.as_deref(), Some("2026-03-02T16:00:00+09:00"));
    assert_eq!(object.end_at.as_deref(), Some("2026-03-02T17:00:00+09:00"));
}

/// 저장 뒤의 DB 덤프 대신 소스가 내는 값 전체를 본다. 저장 쪽은 deferredTests에 있다.
#[test]
fn 참석자는_수와_내_응답만_남긴다_회의실은_세지_않는다() {
    let result = parse_as("personal", &load("personal"));
    let object = by_id(&result, "syntheticmeeting1");
    assert_eq!(object.attendee_count, Some(2));
    assert_eq!(object.my_response.as_deref(), Some("needsAction"));
    assert_eq!(object.busy, Some(true));
    let stored = dump(&result);
    for secret in ["secret-attendee", "organizer@", "저장되면 안 되는", "room@resource"] {
        assert!(!stored.contains(secret), "{secret}");
    }
}

#[test]
fn 출처_등급은_주최자로_판별하고_주소는_남기지_않는다() {
    let result = parse_as("personal", &load("personal"));
    assert_eq!(by_id(&result, "syntheticmeeting1").trust_tier, Some(TrustTier::T1));
    assert_eq!(by_id(&result, "syntheticallday1").trust_tier, Some(TrustTier::T0));
    assert_eq!(by_id(&result, "syntheticoutsider1").trust_tier, Some(TrustTier::T3));
    assert_eq!(by_id(&result, "syntheticoutsider1").my_response.as_deref(), Some("accepted"));
}

/// 응답의 신원을 그대로 내는 데까지다. 신원 불일치를 contract_break로 바꾸는 Ledger 쪽은 deferredTests에 있다.
#[test]
fn 신원_단언_summary와_access_role() {
    assert_eq!(parse_as("personal", &load("personal")).identity.as_deref(), Some(SELF_EMAIL));

    let shared = parse_as("personal", &with(load("personal"), "accessRole", json!("reader")));
    assert_eq!(shared.outcome, Outcome::ContractBreak);
    assert_eq!(shared.error_code.as_deref(), Some("calendar_access_role"));
    assert!(shared.objects.is_empty());

    let wrong = parse_as("personal", &with(load("personal"), "summary", json!("other@example.invalid")));
    assert_eq!(wrong.identity.as_deref(), Some("other@example.invalid"));
}

#[test]
fn 영건은_ok이고_형식이_다르면_parse_error다() {
    let empty =
        parse_as("personal", &json!({ "accessRole": "owner", "summary": SELF_EMAIL, "timeZone": "Asia/Seoul" }));
    assert_eq!((empty.outcome, empty.parsed_count, empty.snapshot), (Outcome::Ok, 0, true));
    assert!(empty.objects.is_empty());
    let code = |query: &str, body: Value| parse_as(query, &body).error_code;
    assert_eq!(code("personal", json!({})).as_deref(), Some("calendar_shape"));
    assert_eq!(code("personal", json!({ "accessRole": "owner", "events": "x" })).as_deref(), Some("calendar_shape"));
    assert_eq!(code("personal", meeting_body(json!([{ "summary": "id 없음" }]))).as_deref(), Some("calendar_event_id"));
    let holiday = json!({ "id": "x", "description": "공휴일", "start": { "date": "not-a-date" } });
    assert_eq!(
        code("holidays", json!({ "accessRole": "reader", "events": [holiday] })).as_deref(),
        Some("calendar_date")
    );
}

#[test]
fn 배포는_제목으로만_고르고_500_atlas는_deploy_객체를_낸다() {
    let result = parse_as("deploy", &load("deploy"));
    let objects: Vec<_> = result.objects.iter().map(|object| (object.external_id.as_str(), object.kind)).collect();
    assert_eq!(objects, [("syntheticdeploy1", ObjectKind::Deploy)]);
    assert_eq!(result.parsed_count, 2);
}

/// 해시가 같으려면 객체 값이 같아야 한다. 그 값까지 본다. 변경 이벤트가 없는지는 deferredTests에 있다.
#[test]
fn 같은_배포가_개인_캘린더에도_보이면_값이_같아_변경으로_잡히지_않는다() {
    let on_deploy = parse_as("deploy", &load("deploy"));
    let on_personal = parse_as("personal", &load("personal"));
    assert_eq!(by_id(&on_deploy, "syntheticdeploy1"), by_id(&on_personal, "syntheticdeploy1"));
}

/// 쉬는 날을 불러오는 쪽은 deferredTests에 있다.
#[test]
fn 공휴일만_쉬는_날이다_기념일은_근무일이다() {
    let result = parse_as("holidays", &load("holidays"));
    let objects: Vec<_> = result.objects.iter().map(|object| (object.kind, object.start_date.as_deref())).collect();
    assert_eq!(objects, [(ObjectKind::DayOff, Some("2026-03-03"))]);
}

/// 쉬는 날을 불러오는 쪽은 deferredTests에 있다.
#[test]
fn 내_종일_연차만_쉬는_날이다_끝_날짜는_제외한다() {
    let result = parse_as("crew_off", &load("crew"));
    let ids: Vec<&str> = result.objects.iter().map(|object| object.external_id.as_str()).collect();
    assert_eq!(ids, ["syntheticleave1"]);
    assert_eq!(result.objects[0].end_date.as_deref(), Some("2026-03-11"));
}

/// 취소 이벤트는 Ledger의 일이다. 그 입력인 상태 값까지 본다.
#[test]
fn status가_cancelled로_오면_바로_취소로_잡힌다() {
    let mut cancelled = new_meeting();
    cancelled["status"] = json!("cancelled");
    let result = parse_as("personal", &meeting_body(json!([cancelled])));
    assert_eq!(result.objects[0].state.as_deref(), Some("cancelled"));
}

/// 부재 집계는 Ledger의 일이다. 스냅샷으로 내지 않는 데까지 본다.
#[test]
fn 실패한_스냅샷은_부재로_세지_않는다() {
    let source = source();
    let failed = [error_response("calendar:personal", "kairos:tool_missing")];
    let result = parse_one(&source, &failed, &context_of(&source, T1, vec![]));
    assert_eq!((result.outcome, result.snapshot), (Outcome::ToolMissing, false));
}

#[test]
fn 다음_쪽이_실패하면_partial이고_스냅샷_부재를_세지_않는다() {
    let source = source();
    let responses =
        [json_response("calendar:personal", &load("personal")), error_response("calendar:personal#2", "내부 오류")];
    let result = parse_one(&source, &responses, &context_of(&source, NOW, vec![]));
    assert_eq!((result.outcome, result.watermark), (Outcome::Partial, None));
    assert!(!result.objects.is_empty());
}

#[test]
fn 쪽을_이어_읽는다() {
    let source = source();
    let responses = [
        json_response("calendar:personal", &load("personal")),
        json_response("calendar:personal#2", &meeting_body(json!([new_meeting()]))),
    ];
    let result = parse_one(&source, &responses, &context_of(&source, NOW, vec![]));
    assert_eq!((result.outcome, result.pages, result.parsed_count), (Outcome::Ok, 2, 5));
}

#[test]
fn 응답이_없는_질의의_결과는_내지_않는다() {
    let source = source();
    assert_eq!(source.parse(&[], &context_of(&source, NOW, vec![])), Ok(vec![]));
}

#[test]
fn 그룹_캘린더_주최자는_id가_목록에_있을_때만_조직_등급이다() {
    let tier = |email: &str| {
        let body = meeting_body(json!([invite(json!({ "email": email }), "합성 회의")]));
        parse_as("personal", &body).objects[0].trust_tier
    };
    assert_eq!(tier("attacker-owned-abc@group.calendar.google.com"), Some(TrustTier::T3));
    assert_eq!(tier(deploy_calendar()), Some(TrustTier::T1));
    assert_eq!(tier(&deploy_calendar().to_uppercase()), Some(TrustTier::T1));
    assert_eq!(tier("teammate@contoso.example"), Some(TrustTier::T1));
}

#[test]
fn 외부_주최자의_배포는_배포가_아니라_일반_일정이고_등급도_t3다() {
    let title = "[배포] 합성 마일스톤";
    let evil = invite(json!({ "email": "evil@attacker.test" }), title);
    let external = parse_as("personal", &meeting_body(json!([evil])));
    assert_eq!(external.objects[0].kind, ObjectKind::Meeting);
    assert_eq!(external.objects[0].trust_tier, Some(TrustTier::T3));
    let group = invite(json!({ "email": "attacker-owned-abc@group.calendar.google.com" }), title);
    assert_eq!(parse_as("personal", &meeting_body(json!([group]))).objects[0].kind, ObjectKind::Meeting);
    let on_deploy_calendar = json!({ "accessRole": "reader", "summary": "합성", "events": [evil] });
    assert!(parse_as("deploy", &on_deploy_calendar).objects.is_empty());
}

/// 주소가 없는 주최자는 외부 주최자와 같다. 믿을 근거가 있어야 배포다.
#[test]
fn 주최자_주소가_없는_배포는_배포가_아니라_일반_일정이고_배포_캘린더에서는_버린다() {
    let title = "[배포] 합성 마일스톤";
    let mut unnamed = invite(json!({}), title);
    unnamed.as_object_mut().unwrap().remove("organizer");
    let events = [
        invite(json!({ "displayName": "합성 외부인" }), title),
        invite(json!({ "email": "" }), title),
        invite(json!({}), title),
        invite(json!({ "self": "true" }), title),
        unnamed,
    ];
    for (index, event) in events.into_iter().enumerate() {
        let personal = parse_as("personal", &meeting_body(json!([event.clone()])));
        let object = &personal.objects[0];
        assert_eq!((object.kind, object.trust_tier), (ObjectKind::Meeting, Some(TrustTier::T3)), "{index}");
        let on_deploy_calendar = json!({ "accessRole": "reader", "summary": "합성", "events": [event] });
        let deploy = parse_as("deploy", &on_deploy_calendar);
        assert_eq!((deploy.outcome, deploy.parsed_count), (Outcome::Ok, 1), "{index}");
        assert!(deploy.objects.is_empty(), "{index}");
    }
}

#[test]
fn 배포_캘린더_자신이_주최한_배포는_주소가_없어도_배포다() {
    let event = invite(json!({ "self": true }), "[배포] 합성 마일스톤");
    let on_deploy_calendar = json!({ "accessRole": "reader", "summary": "합성", "events": [event] });
    let result = parse_as("deploy", &on_deploy_calendar);
    assert_eq!(result.objects.len(), 1);
    assert_eq!((result.objects[0].kind, result.objects[0].trust_tier), (ObjectKind::Deploy, Some(TrustTier::T1)));
}

#[test]
fn 조직_주최자의_배포는_어느_캘린더에서_봐도_같은_값이다() {
    let organizers = [
        json!({ "email": deploy_calendar() }),
        json!({ "email": "teammate@contoso.example" }),
        json!({ "email": SELF_EMAIL, "self": true }),
    ];
    for organizer in organizers {
        let result = parse_as("personal", &meeting_body(json!([invite(organizer, "[배포] 합성 마일스톤")])));
        assert_eq!(result.objects[0].kind, ObjectKind::Deploy);
        assert_eq!(result.objects[0].trust_tier, Some(TrustTier::T1));
    }
}

#[test]
fn 개인_캘린더_summary가_없으면_신원_단언_실패다() {
    let missing = parse_as("personal", &json!({ "accessRole": "owner", "events": [] }));
    assert_eq!(missing.outcome, Outcome::ContractBreak);
    assert_eq!(missing.error_code.as_deref(), Some("calendar_identity_missing"));
    assert_eq!(missing.watermark, None);
}

fn named_leave(end: &str) -> Value {
    let mut body = load("crew");
    let mut event = body["events"][0].clone();
    event["end"] = json!({ "date": end });
    body["events"] = json!([event]);
    body
}

#[test]
fn 비정상적으로_긴_쉬는_날은_받지_않는다() {
    assert_eq!(parse_as("crew_off", &named_leave("2026-03-23T00:00:00Z")).objects.len(), 1);
    assert_eq!(parse_as("crew_off", &named_leave("9999-12-31T00:00:00Z")).objects.len(), 0);
}

#[test]
fn null_일정은_나머지를_쓰되_ok로_내지_않아_취소로_오판하지_않는다() {
    let meeting = json!({ "id": "x", "start": { "dateTime": "2026-03-03T10:00:00+09:00" } });
    let personal = parse_as("personal", &meeting_body(json!([null, meeting])));
    assert_eq!(
        (personal.outcome, personal.error_code.as_deref(), personal.objects.len()),
        (Outcome::Partial, Some("record_null"), 1)
    );
    assert_eq!((personal.parsed_count, personal.watermark.as_deref()), (2, None));
    for query in ["deploy", "holidays", "crew_off"] {
        let result = parse_as(query, &json!({ "accessRole": "reader", "events": [null] }));
        assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Partial, Some("record_null")));
    }
}

#[test]
fn null_참석자는_세지_않는다() {
    let attendees = json!([{ "self": true, "responseStatus": "declined" }, null, { "email": "a@synthetic.test" }]);
    let event = json!({ "id": "x", "attendees": attendees, "start": { "dateTime": "2026-03-03T10:00:00+09:00" } });
    let result = parse_as("personal", &meeting_body(json!([event])));
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.objects[0].attendee_count, Some(2));
    assert_eq!(result.objects[0].my_response.as_deref(), Some("declined"));
}

#[test]
fn 숫자_일정은_id_없는_일정이고_고르는_질의에서는_빠진다() {
    assert_eq!(parse_as("personal", &meeting_body(json!([5]))).error_code.as_deref(), Some("calendar_event_id"));
    assert!(parse_as("deploy", &json!({ "accessRole": "reader", "events": [5] })).objects.is_empty());
}

#[test]
fn 공휴일_설명을_단_시간_일정은_쉬는_날이_아니다() {
    let timed = json!({ "id": "x", "description": "공휴일", "start": { "dateTime": "2026-03-03T10:00:00+09:00" } });
    let result = parse_as("holidays", &json!({ "accessRole": "reader", "events": [timed] }));
    assert_eq!((result.outcome, result.objects.len()), (Outcome::Ok, 0));
    let unnamed = json!({ "description": "공휴일" });
    let result = parse_as("holidays", &json!({ "accessRole": "reader", "events": [timed, unnamed] }));
    assert_eq!(result.error_code.as_deref(), Some("calendar_event_id"), "해석 실패가 먼저다");
}

#[test]
fn 달력에_없는_날짜도_모양이_맞으면_그대로_낸다() {
    let holiday = json!({ "id": "x", "description": " 공휴일 ", "start": { "date": "2026-02-30" } });
    let result = parse_as("holidays", &json!({ "accessRole": "reader", "events": [holiday] }));
    assert_eq!(result.objects[0].start_date.as_deref(), Some("2026-02-30"));
    assert_eq!(result.objects[0].end_date.as_deref(), Some("2026-03-03"));
}
