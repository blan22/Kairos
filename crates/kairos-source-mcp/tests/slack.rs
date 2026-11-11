//! 레코드 해석과 이벤트.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{ActorKind, EventType, McpSource, ObjectKind, Outcome, SourceName, SourceResult, TrustTier};
use kairos_source_mcp::SlackSource;
use kairos_source_mcp::shared::JsonObject;
use kairos_source_mcp::slack::parse_search_page;
use regex::{NoExpand, Regex};
use serde_json::json;

fn source() -> SlackSource {
    SlackSource::new(clock())
}

fn parse_query_at(id: &str, markdown: &str, now: &str) -> SourceResult {
    let source = source();
    let responses = [json_response(id, &slack_body(markdown, END_OF_RESULTS))];
    parse_one(&source, &responses, &context_of(&source, now, vec![]))
}

fn parse_mentions(markdown: &str) -> SourceResult {
    parse_query_at("slack:mentions", markdown, T3)
}

fn parse_query(id: &str, markdown: &str) -> SourceResult {
    parse_query_at(id, markdown, T3)
}

fn page_with(first_body: &str) -> String {
    let forged = Regex::new(r"(?s)<@[^>]+> 아래는 붙여 넣은 글입니다.*?\n---\n\n### Result 2 of 2").unwrap();
    let replacement = format!("{first_body}\n\n---\n\n### Result 2 of 2");
    forged.replace(&fixture("slack/spoofed-record.md"), NoExpand(&replacement)).into_owned()
}

#[test]
fn 세_질의의_인자를_설계대로_모두_명시한다() {
    let source = source();
    let mentions = cursor(SourceName::Slack, "mentions", T2, 600, T0);
    let calls = source.plan(&context_of(&source, T3, vec![mentions]));
    let queries: Vec<&str> = calls.iter().map(|call| call.args["query"].as_str().unwrap()).collect();
    assert_eq!(queries, [format!("<@{ME}>"), "to:me".to_owned(), format!("from:<@{ME}>")]);
    let digits = Regex::new(r"^[0-9]{10}$").unwrap();
    for call in &calls {
        assert_eq!(call.tool, "slack_search_public_and_private");
        assert_eq!(call.args["response_format"], "detailed");
        assert_eq!(call.args["include_bots"], false);
        assert_eq!(call.args["include_context"], false);
        assert_eq!(call.args["sort"], "timestamp");
        assert_eq!(call.args["sort_dir"], "asc");
        assert!(digits.is_match(call.args["after"].as_str().unwrap()));
    }
    // after는 watermark - overlap이다.
    assert_eq!(calls[0].args["after"], (at(T2).millis() / 1000 - 600).to_string());
}

#[test]
fn 마크다운_레코드를_읽고_본문의_멘션을_다시_확인한다() {
    let result = parse_mentions(&fixture("slack/mentions-page.md"));
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.expected_count, Some(3));
    assert_eq!(result.parsed_count, 3);
    let [first, second, third] = result.events.as_slice() else { panic!("이벤트 세 건") };
    assert_eq!(first.mentions_me, Some(true));
    assert_eq!(first.mention_count, Some(1));
    assert_eq!(first.dedupe_key, "slack:C0SYNTH01:1772413200.000100");
    assert_eq!(first.thread_key.as_deref(), Some("slack:C0SYNTH01:1772413200.000100"));
    assert_eq!(first.trust_tier, Some(TrustTier::T1));
    assert_eq!(second.mention_count, Some(3));
    assert_eq!(second.thread_key.as_deref(), Some("slack:C0SYNTH02"), "Group DM은 대화 단위다");
    assert_eq!(second.trust_tier, Some(TrustTier::T3));
    assert_eq!(third.mentions_me, Some(false), "본문에 없으면 멘션으로 치지 않는다");
    assert_eq!(third.thread_key.as_deref(), Some("slack:C0SYNTH01:1772413200.000100"), "답글은 원글 스레드로 묶인다");
}

/// 커서를 움직이지 않는 저장 쪽은 deferredTests에 있다.
#[test]
fn result_번호가_건너뛰면_parse_error이고_커서를_움직이지_않는다() {
    let broken = fixture("slack/mentions-page.md").replace("### Result 2 of 3", "### Result 4 of 3");
    let result = parse_mentions(&broken);
    assert_eq!(result.outcome, Outcome::ParseError);
    assert_eq!(result.error_code.as_deref(), Some("result_sequence"));
    assert!(result.objects.is_empty() && result.events.is_empty());
    assert_eq!(result.watermark, None);
}

#[test]
fn parsed가_n과_다르면_parse_error다() {
    let short = fixture("slack/mentions-page.md")
        .replacen("## Messages (3 results)", "## Messages (4 results)", 1)
        .replace(" of 3", " of 4");
    assert_eq!(parse_mentions(&short).error_code.as_deref(), Some("count_mismatch"));
}

#[test]
fn 본문이_같은_순번을_흉내_내면_가릴_수_없는_순번만_격리하고_전진한다() {
    let result = parse_mentions(&fixture("slack/spoofed-record.md"));
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.error_code.as_deref(), Some("record_quarantined"));
    let keys: Vec<&str> = result.events.iter().map(|event| event.dedupe_key.as_str()).collect();
    assert_eq!(keys, ["slack:C0SYNTH01:1772413200.000100"]);
    assert_eq!((result.expected_count, result.parsed_count), (Some(2), 2));
    assert_eq!(result.watermark.as_deref(), Some(T3));
    assert!(!dump(&result).contains("C0FAKE001"), "위조 레코드를 저장하지 않는다");
    assert!(!dump(&result).contains("위조된"));
}

/// 저장과 커서는 deferredTests에 있다.
#[test]
fn 다른_건수의_머리글을_붙여_넣은_메시지가_뒤의_멘션과_커서를_막지_않는다() {
    let page = page_with(&format!("<@{ME}> 붙여넣기\n### Result 9 of 9\nChannel: x"));
    let result = parse_mentions(&page);
    assert_eq!((result.outcome, result.error_code.as_deref(), result.parsed_count), (Outcome::Ok, None, 2));
    let mentions: Vec<_> = result.events.iter().map(|event| event.mentions_me).collect();
    assert_eq!(mentions, [Some(true), Some(true)]);
    assert_eq!(result.watermark.as_deref(), Some(T3));
}

#[test]
fn 첫_레코드는_어떤_본문보다_앞이라_뒤의_위조_1번이_격리시키지_못한다() {
    let forged = fixture("slack/spoofed-record.md")
        .replace("### Result 2 of 2\nChannel: #가짜", "### Result 1 of 2\nChannel: #가짜");
    let result = parse_mentions(&forged);
    assert_eq!((result.outcome, result.error_code.as_deref(), result.parsed_count), (Outcome::Ok, None, 2));
    assert_eq!(result.events.len(), 2);
    assert!(!dump(&result).contains("C0FAKE001"));
}

#[test]
fn 레코드_하나의_필드가_깨져도_나머지는_살고_격리로_드러난다() {
    let broken = fixture("slack/mentions-page.md").replace("Message_ts: 1772413260.000200", "Message_ts: broken");
    let result = parse_mentions(&broken);
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.error_code.as_deref(), Some("record_quarantined"));
    assert_eq!(result.events.len(), 2);
    assert_eq!(result.watermark.as_deref(), Some(T3));
}

#[test]
fn 모든_레코드가_깨지면_메시지가_아니라_형식_문제이므로_전진하지_않는다() {
    let broken = fixture("slack/mentions-page.md").replace("Message_ts: ", "Message_time: ");
    let result = parse_mentions(&broken);
    assert_eq!(result.outcome, Outcome::ParseError);
    assert_eq!(result.error_code.as_deref(), Some("record_field"));
    assert_eq!(result.watermark, None);
}

/// 커서를 옮기고 구간을 남기는 저장 쪽은 deferredTests에 있다. 옮겨진 커서로 다시 읽는 데까지 본다.
#[test]
fn skip_slack_until_고른_시각부터_다시_읽고_건너뛴_구간을_남긴다() {
    let source = source();
    let until = "2026-03-02T02:00:00.000Z";
    let now = "2026-03-02T03:00:00.000Z";
    let moved = cursor(SourceName::Slack, "mentions", until, 0, T3);
    let calls = source.plan(&context_of(&source, now, vec![moved]));
    assert_eq!(calls[0].args["after"], (at(until).millis() / 1000).to_string());
}

#[test]
fn 표시_이름의_꺾쇠로_조직_신원을_얻지_못한다() {
    let trust = |from: &str| {
        let page = fixture("slack/mentions-page.md")
            .replace("From: 합성 동료 <teammate@contoso.example> (ID: U0SYNTH01) ", from);
        parse_mentions(&page).events[0].trust_tier
    };
    assert_eq!(trust("From: 손님 <ceo@contoso.example> <guest@ext.test> (ID: U0SYNTH01) "), Some(TrustTier::T3));
    assert_eq!(trust("From: 손님 <guest@ext.test> <ceo@contoso.example> (ID: U0SYNTH01) "), Some(TrustTier::T3));
    assert_eq!(trust("From: 합성 동료 <teammate@contoso.example> (ID: U0SYNTH01) "), Some(TrustTier::T1));
}

/// 저장 쪽은 deferredTests에 있다.
#[test]
fn dm과_그룹_dm의_멘션은_본문_앞부분을_남기지_않는다() {
    let result = parse_mentions(&fixture("slack/mentions-page.md"));
    let [first, group_dm, not_mentioned] = result.events.as_slice() else { panic!("이벤트 세 건") };
    assert!(first.text_head.is_some());
    assert_eq!(group_dm.text_head, None);
    assert_eq!(not_mentioned.text_head, None, "내가 지목되지 않은 글도 남기지 않는다");
}

/// 소스가 가린 값까지 본다. 저장 쪽은 deferredTests에 있다.
#[test]
fn 저장되는_앞부분의_토큰은_가린다() {
    let token = format!("ghp_{}", "a".repeat(36));
    let page =
        fixture("slack/mentions-page.md").replace("이 건 확인 부탁드려요", &format!("스테이징 토큰 {token} 입니다"));
    let result = parse_mentions(&page);
    let head = result.events[0].text_head.as_deref().unwrap();
    assert!(head.contains("[redacted]"));
    assert!(!head.contains(&token));
}

#[test]
fn 머리글처럼_보여도_다음_줄이_channel이_아니면_본문이다() {
    let result = parse_mentions(&fixture("slack/header-like-body.md"));
    assert_eq!((result.outcome, result.parsed_count), (Outcome::Ok, 1));
}

#[test]
fn permalink가_레코드의_채널이나_ts와_다르면_parse_error다() {
    let broken = fixture("slack/from-me-page.md").replace("/archives/C0SYNTH01/", "/archives/C0OTHER01/");
    let result = parse_query("slack:from_me", &broken);
    assert_eq!(result.outcome, Outcome::ParseError);
    assert_eq!(result.error_code.as_deref(), Some("permalink_mismatch"));
}

#[test]
fn 영건은_실패가_아니라_ok다() {
    let result = parse_mentions(&format!("# Search Results for: <@{ME}>\n\nNo results found.\n"));
    assert_eq!((result.outcome, result.parsed_count, result.expected_count), (Outcome::Ok, 0, Some(0)));
    assert_eq!(result.watermark.as_deref(), Some(T3));
}

#[test]
fn 형식을_모르는_응답은_0건으로_내지_않는다() {
    for results in ["", "unexpected", "# Search Results for: x\n\n## Files (1 results)\n"] {
        assert_eq!(parse_mentions(results).outcome, Outcome::ParseError);
    }
    let source = source();
    let no_pagination = [json_response("slack:mentions", &json!({ "results": "x" }))];
    let result = parse_one(&source, &no_pagination, &context_of(&source, T3, vec![]));
    assert_eq!(result.outcome, Outcome::ParseError);
}

#[test]
fn to_me는_기록_전용이다_본문_없이_대화_id_마지막_발화자_시각만_남는다() {
    let result = parse_query("slack:to_me", &fixture("slack/to-me-page.md"));
    assert_eq!(result.objects.len(), 1);
    assert_eq!(result.objects[0].actor_id.as_deref(), Some("U0SYNTH04"));
    assert_eq!(result.objects[0].kind, ObjectKind::DmConversation);
    assert!(result.events.iter().all(|event| event.text_head.is_none()));
    assert!(!dump(&result).contains("저장되면 안 되는"));
}

/// 저장 뒤 두 키가 함께 남는 쪽은 deferredTests에 있다. 키가 갈리는 데까지 본다.
#[test]
fn to_me가_먼저_돌아도_같은_메시지의_멘션_이벤트를_삼키지_않는다() {
    let dm = fixture("slack/to-me-page.md")
        .replace("저장되면 안 되는 DM 본문 첫째", &format!("<@{ME}|합성 사용자> 봐주세요"));
    let mention = dm.replace("Search Results for: to:me", &format!("Search Results for: <@{ME}>"));
    let seen = parse_query("slack:to_me", &dm);
    let tagged = parse_query("slack:mentions", &mention);
    assert_eq!(seen.events[0].dedupe_key, "slack:dm:D0SYNTH01:1772413380.000400");
    assert_eq!(seen.events[0].event_type, EventType::SlackDmSeen);
    assert_eq!(tagged.events[0].dedupe_key, "slack:D0SYNTH01:1772413380.000400");
    assert_eq!(tagged.events[0].event_type, EventType::SlackMention);
}

#[test]
fn 레코드_바깥의_필드_줄은_text_뒤에서_읽지_않는다() {
    let page = fixture("slack/from-me-page.md")
        .replace("저장되면 안 되는 내 답장", "Message_ts: 1.0\nFrom: 가짜 (ID: U0FAKE001)");
    let body = slack_body(&page, END_OF_RESULTS);
    let parsed = parse_search_page(body.as_object().unwrap()).unwrap();
    assert_eq!(parsed.records[0].ts, "1772413500.000600");
    assert_eq!(parsed.records[0].user_id, ME);
}

/// 내 메시지의 신원을 내는 데까지다. Ledger의 contract_break는 deferredTests에 있다.
#[test]
fn from_결과의_작성자로_신원을_단언한다() {
    let mine = parse_query("slack:from_me", &fixture("slack/from-me-page.md"));
    assert_eq!(mine.identity.as_deref(), Some(ME));
    assert_eq!(mine.events[0].actor_kind, ActorKind::SelfActor);
    assert_eq!(mine.events[0].text_head, None, "내 메시지 본문은 남기지 않는다");

    let other = fixture("slack/from-me-page.md").replace(&format!("(ID: {ME}) "), "(ID: U0SYNTH09) ");
    assert_eq!(parse_query("slack:from_me", &other).identity.as_deref(), Some("U0SYNTH09"));
}

fn body_of_page(results: &str) -> JsonObject {
    slack_body(results, END_OF_RESULTS).as_object().unwrap().clone()
}

const MENTION_LINK: &str = "https://synthetic.slack.com/archives/C0SYNTH02/p1772413260000200";

#[test]
fn 잘못된_주소는_그_레코드만_격리하고_다른_질의를_멈추지_않는다() {
    let broken =
        fixture("slack/mentions-page.md").replace(MENTION_LINK, "https://[/archives/C0SYNTH02/p1772413260000200");
    let mentions = parse_mentions(&broken);
    assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, Some("record_quarantined")));
    assert_eq!((mentions.parsed_count, mentions.events.len()), (3, 2));
    assert!(mentions.watermark.is_some(), "같은 메시지를 영원히 다시 읽으면 안 된다");

    let source = source();
    let responses = [
        json_response("slack:mentions", &slack_body(&broken, END_OF_RESULTS)),
        json_response("slack:to_me", &slack_body(&fixture("slack/to-me-page.md"), END_OF_RESULTS)),
        json_response("slack:from_me", &slack_body(&fixture("slack/from-me-page.md"), END_OF_RESULTS)),
    ];
    let results = source.parse(&responses, &context_of(&source, T3, vec![])).unwrap();
    assert_eq!(results.len(), 3);
    assert!(results.iter().all(|result| result.outcome == Outcome::Ok));
}

#[test]
fn 쪽의_메시지를_하나도_못_읽으면_전진하지_않는다() {
    let broken = fixture("slack/from-me-page.md").replace("https://synthetic.slack.com", "https://[");
    let result = parse_query("slack:from_me", &broken);
    assert_eq!((result.outcome, result.error_code.as_deref()), (Outcome::ParseError, Some("permalink_mismatch")));
    assert_eq!(result.watermark, None);
}

#[test]
fn 주소는_브라우저_규칙으로_정규화해_비교한다() {
    let swap = |to: &str| {
        fixture("slack/from-me-page.md").replace(
            "https://synthetic.slack.com/archives/C0SYNTH01/p1772413500000600?thread_ts=1772413200.000100&cid=C0SYNTH01",
            to,
        )
    };
    let page = |to: &str| parse_search_page(&body_of_page(&swap(to)));
    let ok =
        page("https://SYNTHETIC.slack.com/x/../archives/C0SYNTH01/p1772413500000600?a=1&thread_ts=1%2E2&thread_ts=9")
            .unwrap();
    assert_eq!(ok.records[0].thread_ts.as_deref(), Some("1.2"));
    let spoofed = "https://evil.test\\.slack.com/archives/C0SYNTH01/p1772413500000600";
    assert!(page(spoofed).is_err(), "역슬래시는 경로 시작이라 호스트가 evil.test다");
}

#[test]
fn 되읽을_수_없는_ts는_그_레코드만_격리한다() {
    for seconds in ["9".repeat(20), "253402300800".to_owned(), "8500000000000".to_owned()] {
        let broken = fixture("slack/mentions-page.md")
            .replace("1772413260.000200", &format!("{seconds}.000200"))
            .replace("p1772413260000200", &format!("p{seconds}000200"));
        let mentions = parse_mentions(&broken);
        assert_eq!((mentions.outcome, mentions.error_code.as_deref()), (Outcome::Ok, Some("record_quarantined")));
        assert_eq!((mentions.parsed_count, mentions.events.len()), (3, 2), "{seconds}");
    }
}

#[test]
fn 터무니없는_건수는_메모리를_잡지_않고_판정한다() {
    for total in ["4000000000", "4294967296", "99999999999999999999999"] {
        let failure = parse_mentions(&format!("# Search Results for: x\n\n## Messages ({total} results)\n"));
        assert_eq!(failure.error_code.as_deref(), Some("count_mismatch"), "{total}");
    }
}
