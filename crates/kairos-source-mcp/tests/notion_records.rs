//! 스레드 기록.
#![allow(clippy::unwrap_used)]

mod common;
mod notion_support;

use common::*;
use kairos_core::model::{ActorKind, EventType, ObjectKind, Outcome, TrustTier};
use notion_support::*;
use serde_json::{Value, json};

/// 시험 프로필(`Profile::fixture`)의 본인 이름.
const SELF_NAME: &str = "김준서";

fn digest_events() -> Vec<kairos_core::model::NormalizedEvent> {
    parse(&[search(&["t1"]), thread_response("t1")]).events
}

fn mail_with(lines: &[String]) -> Value {
    message(&lines.join("\n"))
}

#[test]
fn 복수_행위자는_쉼표로_잇고_끝의_님을_지운다() {
    assert_eq!(blocks()[2].actors, ["이영희", "최민수"]);
    let stored = &digest_events()[2];
    assert_eq!(stored.actor_id.as_deref(), Some("이영희, 최민수"));
    assert_eq!(stored.actor_kind, ActorKind::Human);
}

#[test]
fn 본인이_끼어_있어도_다른_사람이_있으면_사람_활동이고_본인_단독만_self다() {
    let events = digest_events();
    assert_eq!(events[3].actor_id, Some(format!("이영희, {SELF_NAME}")));
    assert_eq!(events[3].actor_kind, ActorKind::Human);
    assert_eq!(events[4].actor_id.as_deref(), Some(SELF_NAME));
    assert_eq!(events[4].actor_kind, ActorKind::SelfActor);
}

#[test]
fn 이벤트는_키_시각_등급_링크가_계약대로다() {
    let parsed = parse(&[search(&["t1"]), thread_response("t1")]);
    assert_eq!((parsed.outcome, parsed.error_code.as_deref(), parsed.parsed_count), (Outcome::Ok, None, 8));
    assert_eq!(parsed.objects.len(), 1);
    let object = &parsed.objects[0];
    assert_eq!(object.object_key, "notion-mail:t1");
    assert_eq!((object.kind, object.external_id.as_str()), (ObjectKind::NotionMail, "t1"));
    assert_eq!(object.text_head, None);
    let first = &parsed.events[0];
    assert_eq!(first.dedupe_key, "notion-mail:t1:0");
    assert_eq!(parsed.events[7].dedupe_key, "notion-mail:t1:7");
    assert_eq!(first.object_key.as_deref(), Some("notion-mail:t1"));
    assert_eq!(first.occurred_at, DATE);
    assert_eq!(first.trust_tier, Some(TrustTier::T2));
    assert_eq!(first.permalink.as_deref(), Some("https://mail.google.com/"));
    assert_eq!(first.event_type, EventType::NotionComment);
}

#[test]
fn permalink는_gmail_주소만_받는다() {
    let thread = json!({
        "id": "t1",
        "viewUrl": "https://mg.mail.notion.com/c/x",
        "messages": [message_with(&digest(), json!({ "viewUrl": "https://mg.mail.notion.com/c/y" }))],
    });
    let stored = parse(&[search(&["t1"]), json_response(&format!("{SEARCH}!t1"), &thread)]);
    assert_eq!(stored.objects[0].permalink, None);
    assert!(stored.events.iter().all(|event| event.permalink.is_none()));
}

/// 저장 뒤의 DB 덤프 대신 소스가 내는 값 전체를 본다.
#[test]
fn 추적_링크_본문_댓글_내용_메일_제목_미리보기를_저장하지_않는다() {
    let summary = thread_response_with("t2", vec![message(&fixture("notion/summary.txt"))]);
    let parsed = parse(&[search(&["t1", "t2"]), thread_response("t1"), summary]);
    let stored = dump(&parsed);
    for forbidden in ["mg.mail.notion.com", "synthetic-", "저장금지", "합성 사용자", "합성 부제", "다음 댓글"]
    {
        assert!(!stored.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn 인기_페이지와_추천_페이지는_제목의_페이지_이름만_쓰고_promo다() {
    for subject in ["Contoso의 인기 페이지: 합성 인기 제목", "추천 페이지 - Contoso: 합성 인기 제목"]
    {
        let mail = message_with(
            "[| || | 합성 본문 제목 || | 부제 |](https://mg.mail.notion.com/c/x)",
            json!({ "subject": subject }),
        );
        let parsed = parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])]);
        assert_eq!(parsed.events.len(), 1);
        let event = &parsed.events[0];
        assert_eq!(event.event_type, EventType::NotionPromo);
        assert_eq!(event.text_head.as_deref(), Some("합성 인기 제목"));
        assert_eq!(event.actor_kind, ActorKind::Bot);
        assert_eq!(event.dedupe_key, "notion-mail:t1:0");
    }
}

/// 규칙 판정은 deferredTests에 있다.
#[test]
fn 동작_문구가_없는_메일도_버리지_않고_notion_unknown으로_남기며_제목을_저장하지_않는다() {
    let mail = message_with("새로운 형식의 합성 본문", json!({ "subject": "합성 사용자의 새 알림 저장금지" }));
    let parsed = parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])]);
    assert_eq!(parsed.events.len(), 1);
    assert_eq!(parsed.events[0].event_type, EventType::NotionUnknown);
    assert_eq!(parsed.events[0].text_head, None);
}

/// 다시 읽은 키가 같은 데까지 본다. 저장의 멱등은 deferredTests에 있다.
#[test]
fn 재수집은_멱등이다() {
    let keys = |events: Vec<kairos_core::model::NormalizedEvent>| -> Vec<String> {
        events.into_iter().map(|event| event.dedupe_key).collect()
    };
    assert_eq!(keys(digest_events()), keys(digest_events()));
}

/// 저장 뒤 블록 키의 안정성까지 본다. 저장 쪽은 deferredTests에 있다.
#[test]
fn 저장_뒤_메시지가_붙은_스레드는_다시_읽고_앞_블록의_키는_그대로다() {
    let one = "[| | 합성 제목 1 |](https://mg.mail.notion.com/c/x)\n| | 홍길동 이 페이지 페이지를 편집했습니다. |";
    let two =
        "[| | 합성 제목 2 |](https://mg.mail.notion.com/c/y)\n| | 홍길동 이 페이지페이지에서 나를 멘션했습니다. |";
    let grown = json_response(SEARCH, &json!({ "threads": [search_thread("t1", SENDER, 2)] }));
    let unread = parse_known(std::slice::from_ref(&grown), &["t1"], None);
    assert_eq!(unread.outcome, Outcome::Partial, "메시지가 늘어난 스레드는 저장했어도 읽어야 한다");
    let read = parse_known(&[grown, thread_response_with("t1", vec![message(one), message(two)])], &["t1"], None);
    assert_eq!(read.events[0].dedupe_key, "notion-mail:t1:0");
    assert_eq!(read.events[0].event_type, EventType::NotionEdit);
    assert_eq!(read.events[1].dedupe_key, "notion-mail:t1:1");
    assert_eq!(read.events[1].event_type, EventType::NotionMention);
}

/// 규칙 판정은 deferredTests에 있다. 규칙의 입력까지 본다.
#[test]
fn 모르는_문구라도_내용에_내_이름이_있으면_p1이고_없으면_p2로_열린다_제목은_저장하지_않는다() {
    let mail =
        |content: &str| mail_with(&[head("합성 제목 저장금지"), row("김합성 이 페이지를 공유했습니다."), row(content)]);
    let parsed = parse(&[
        search(&["t1", "t2"]),
        thread_response_with("t1", vec![mail("준서님 확인 부탁")]),
        thread_response_with("t2", vec![mail("합성 내용")]),
    ]);
    let inputs: Vec<_> =
        parsed.events.iter().map(|event| (event.event_type, event.mentions_me, event.text_head.clone())).collect();
    assert_eq!(inputs, [(EventType::NotionUnknown, Some(true), None), (EventType::NotionUnknown, Some(false), None)]);
}

/// 다시 읽어 키가 그대로인 데까지 본다. 저장과 rereadNotion은 deferredTests에 있다.
#[test]
fn 파서를_고친_뒤_다시_읽으면_unknown_블록만_새로_판정되고_다른_블록은_그대로다() {
    let edit = "이합성 이 페이지 페이지를 편집했습니다.";
    let read = |second: &str| {
        let mail = mail_with(&[head("합성 제목 1"), row(edit), head("합성 제목 2"), row(second)]);
        parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])]).events
    };
    let before = read("김합성 이 페이지를 공유했습니다.");
    assert_eq!(before[1].event_type, EventType::NotionUnknown);
    let after = read("김합성 이 페이지페이지에서 나를 멘션했습니다.");
    assert_eq!(after[0], before[0]);
    assert_eq!((after[1].dedupe_key.as_str(), after[1].event_type), ("notion-mail:t1:1", EventType::NotionMention));
}

/// 저장 뒤의 DB 덤프 대신 소스가 내는 값 전체를 본다.
#[test]
fn 머리_모양의_내용_줄은_unknown이_되어도_글자를_저장하지_않는다() {
    let mail = mail_with(&[head("합성 제목"), row(EDIT), "| | 합성 내용 |".to_owned(), head("금지본문-C 머리 모양")]);
    assert!(!dump(&parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])])).contains("금지본문"));
}

/// 저장 뒤의 DB 덤프 대신 소스가 내는 값 전체를 본다.
#[test]
fn 내용_줄이_동작_문구로_끝나도_내용을_행위자로_저장하지_않는다() {
    let content =
        format!("홍길동 금지본문-A ghp_abcdefghijklmnop1234 {}이 페이지 페이지를 편집했습니다.", "긴 내용 ".repeat(40));
    let mail = mail_with(&[head("합성 제목"), row("이합성 이 페이지 블록에서 댓글을 남겼습니다."), row(&content)]);
    let parsed = parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])]);
    let stored = format!("{:?}", parsed.events);
    for forbidden in ["금지본문", "ghp_"] {
        assert!(!stored.contains(forbidden), "{forbidden}");
    }
    assert_eq!(parsed.events[0].event_type, EventType::NotionComment);
}

#[test]
fn 행위자는_120자까지만_저장하고_토큰을_가린다() {
    let names: Vec<String> = (0..9).map(|index| format!("합성이름ghp_abcdefghij{index}")).collect();
    let phrase = format!("{} 이 페이지 페이지를 편집했습니다.", names.join(", "));
    let mail = mail_with(&[head("합성 제목"), row(&phrase)]);
    let parsed = parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])]);
    let actor = parsed.events[0].actor_id.as_deref().unwrap();
    assert!(actor.chars().count() <= 120);
    assert!(!actor.contains("ghp_"));
}

#[test]
fn 본인이_끼어_있는_묶음은_본인_단독이_아니다() {
    let phrase = format!("{SELF_NAME} 외 2명 이 페이지 블록에서 댓글을 남겼습니다.");
    let parsed =
        parse(&[search(&["t1"]), thread_response_with("t1", vec![mail_with(&[head("합성 제목"), row(&phrase)])])]);
    assert_eq!(parsed.events[0].actor_kind, ActorKind::Human);
}

/// 규칙 판정은 deferredTests에 있다. 규칙의 입력까지 본다.
#[test]
fn 같은_표시_이름의_사람이_나를_멘션해도_noise가_아니다() {
    let phrase = format!("{SELF_NAME} 이 페이지페이지에서 나를 멘션했습니다.");
    let parsed =
        parse(&[search(&["t1"]), thread_response_with("t1", vec![mail_with(&[head("합성 제목"), row(&phrase)])])]);
    let event = &parsed.events[0];
    assert_eq!(
        (event.event_type, event.actor_kind, event.mentions_me),
        (EventType::NotionMention, ActorKind::SelfActor, Some(true))
    );
}

#[test]
fn 메일_한_통의_블록은_상한까지만_내되_나를_부른_블록을_먼저_살리고_순번을_유지한다() {
    let pages: Vec<String> = (0..150)
        .map(|index| {
            let action =
                if index == 149 { "김합성 이 페이지페이지에서 나를 멘션했습니다." } else { EDIT };
            [head(&format!("합성 {index}")), row(action)].join("\n")
        })
        .collect();
    let parsed = parse(&[search(&["t1"]), thread_response_with("t1", vec![mail_with(&pages)])]);
    assert_eq!(
        (parsed.outcome, parsed.error_code.as_deref(), parsed.events.len()),
        (Outcome::Ok, Some("notion_block_cap"), 100)
    );
    let last = parsed.events.last().unwrap();
    assert_eq!(last.dedupe_key, "notion-mail:t1:149");
    assert_eq!(last.event_type, EventType::NotionMention);
}

// 아래는 `Number()` 변환과 읽지 못한 값의 처리를 고정한다.

fn occurred(date: Option<Value>, internal: Option<Value>) -> String {
    let mut mail = message(&digest());
    let fields = mail.as_object_mut().unwrap();
    fields.remove("date");
    fields.remove("internalDate");
    if let Some(date) = date {
        fields.insert("date".to_owned(), date);
    }
    if let Some(internal) = internal {
        fields.insert("internalDate".to_owned(), internal);
    }
    parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])]).events.remove(0).occurred_at
}

#[test]
fn internal_date는_number_변환을_따른다() {
    let epoch = "1970-01-01T00:00:00.000Z";
    assert_eq!(occurred(None, Some(json!("1772413200000"))), "2026-03-02T01:00:00.000Z");
    assert_eq!(occurred(None, Some(json!(1_772_413_200_000_i64))), "2026-03-02T01:00:00.000Z");
    assert_eq!(occurred(None, Some(json!(" 1772413200000\n"))), "2026-03-02T01:00:00.000Z");
    assert_eq!(occurred(None, Some(json!("0x10"))), "1970-01-01T00:00:00.016Z");
    assert_eq!(occurred(None, Some(json!("1.5e3"))), "1970-01-01T00:00:01.500Z");
    assert_eq!(occurred(None, Some(json!(1.9))), "1970-01-01T00:00:00.001Z");
    assert_eq!(occurred(None, Some(json!(""))), epoch);
    assert_eq!(occurred(None, Some(Value::Null)), epoch);
    assert_eq!(occurred(None, Some(json!([]))), epoch);
    assert_eq!(occurred(None, Some(json!(["7"]))), "1970-01-01T00:00:00.007Z");
    assert_eq!(occurred(None, Some(json!(true))), "1970-01-01T00:00:00.001Z");
    for odd in [
        json!("abc"),
        json!("-0x10"),
        json!("inf"),
        json!("nan"),
        json!("Infinity"),
        json!({}),
        json!([1, 2]),
        json!([true]),
    ] {
        assert_eq!(occurred(None, Some(odd.clone())), T3, "{odd}");
    }
    assert_eq!(occurred(None, None), T3);
}

#[test]
fn 문자열_date가_있으면_internal_date를_보지_않는다() {
    assert_eq!(occurred(Some(json!("해석 불가")), Some(json!("1772413200000"))), T3);
    assert_eq!(occurred(Some(json!(5)), Some(json!("1772413200000"))), "2026-03-02T01:00:00.000Z");
}

#[test]
fn 되읽을_수_없는_internal_date는_지금_시각으로_둔다() {
    let cases = [
        ("9000000000000000", T3),
        ("8300000000000000", T3),
        ("253402300800000", T3),
        ("-62167219200001", T3),
        ("253402300799999", "9999-12-31T23:59:59.999Z"),
    ];
    for (internal_date, expected) in cases {
        let mut mail = message(&digest());
        let fields = mail.as_object_mut().unwrap();
        fields.remove("date");
        fields.insert("internalDate".to_owned(), json!(internal_date));
        let result = parse(&[search(&["t1"]), thread_response_with("t1", vec![mail])]);
        assert_eq!(result.outcome, Outcome::Ok, "{internal_date}");
        assert_eq!(result.objects[0].ts.as_deref(), Some(expected), "{internal_date}");
    }
}

#[test]
fn null_스레드는_나머지를_읽되_전진하지_않고_id_없는_스레드는_결과가_된다() {
    let listed = json!({ "threads": [null, search_thread("t1", SENDER, 1)] });
    let result = parse(&[json_response(SEARCH, &listed), thread_response("t1")]);
    assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::Partial, Some("record_null")));
    assert_eq!((result.objects.len(), result.watermark.as_deref()), (1, None));
    let nameless = parse(&[json_response(SEARCH, &json!({ "threads": [{ "messages": [] }] }))]);
    assert_eq!((nameless.outcome, nameless.error_code.as_deref()), (Outcome::ParseError, Some("gmail_thread_id")));
}
