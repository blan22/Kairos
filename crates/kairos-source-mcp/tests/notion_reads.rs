//! 검색 계획, 후속 읽기, 전진.
#![allow(clippy::unwrap_used)]

mod common;
mod notion_support;

use std::collections::BTreeMap;

use common::*;
use kairos_core::model::{
    CursorState, McpExpand, McpSource, Outcome, Source, SourceResult, ThreadReadOutcome, ThreadReadState,
};
use notion_support::*;
use serde_json::{Value, json};

fn planned(cursor: Option<CursorState>) -> Vec<(String, String)> {
    let source = source();
    let context = context_of(&source, T3, cursor.into_iter().collect());
    let calls = source.plan(&context);
    calls.into_iter().map(|call| (call.id, call.args["query"].as_str().unwrap().to_owned())).collect()
}

fn progress_cursor(days: i64, progress_days: i64) -> CursorState {
    CursorState { progress_at: Some(at(T3).minus_ms(progress_days * DAY_MS).to_iso()), ..cursor_at(days) }
}

fn entry(id: &str, query: &str) -> (String, String) {
    (id.to_owned(), format!("from:{SENDER} {query}"))
}

fn expand_of(states: Vec<ThreadReadState>) -> McpExpand {
    let source = source();
    let context = with_reads(context_of(&source, T3, vec![]), states);
    source.plan(&context).remove(0).expand.unwrap()
}

fn code(result: &SourceResult) -> Option<&str> {
    result.error_code.as_deref()
}

#[test]
fn 십오분_주기이고_검색은_본문과_제목을_요청하지_않으며_후속_읽기를_지시한다() {
    let source = source();
    assert_eq!(source.queries().len(), 1);
    let spec = &source.queries()[0];
    assert_eq!((spec.id.as_str(), spec.interval_seconds, spec.overlap_seconds), ("mail", 900, 0));
    assert!(source.needs_thread_reads());
    let calls = source.plan(&with_reads(context_of(&source, T3, vec![]), reads(&["t-old"])));
    assert_eq!(calls.len(), 1, "공백이 없으면 최근 구간 검색 하나다");
    let call = &calls[0];
    assert_eq!(call.id, SEARCH);
    assert_eq!((call.server.as_str(), call.tool.as_str()), ("claude.ai Gmail", "search_threads"));
    let args =
        json!({ "query": format!("from:{SENDER} newer_than:2d"), "pageSize": 50, "view": "THREAD_VIEW_METADATA_ONLY" });
    assert_eq!(Value::Object(call.args.clone()), args);
    let expand = call.expand.as_ref().unwrap();
    assert_eq!((expand.tool.as_str(), expand.max_reads), ("get_thread", 12));
    assert_eq!(expand.skip, BTreeMap::from([("t-old".to_owned(), 1)]));
    assert!(expand.retry_ids.is_empty());
}

#[test]
fn 공백이_이틀을_넘으면_최근_구간은_그대로_읽고_오래된_쪽부터_이틀씩_더_읽는다() {
    assert_eq!(planned(Some(cursor_at(1))), [entry(SEARCH, "newer_than:2d")]);
    assert_eq!(
        planned(Some(cursor_at(5))),
        [entry(SEARCH, "newer_than:2d"), entry(BACKLOG, "newer_than:6d older_than:4d")]
    );
    assert_eq!(planned(Some(cursor_at(90)))[1], entry(BACKLOG, "newer_than:30d older_than:28d"));
}

#[test]
fn 밀린_조각을_다_읽으면_partial이어도_읽은_표시만_전진하고_다음_틱의_창이_줄어든다() {
    let first = parse_known(&[search(&[]), old_search(&[])], &[], Some(cursor_at(5)));
    assert_eq!(
        (first.outcome, code(&first), first.watermark.clone()),
        (Outcome::Partial, Some("notion_backlog"), None)
    );
    assert_eq!(first.progress_at, Some(at(T3).minus_ms(4 * DAY_MS).to_iso()));
    let next = CursorState { progress_at: first.progress_at.clone(), ..cursor_at(5) };
    assert_eq!(planned(Some(next))[1], entry(BACKLOG, "newer_than:5d older_than:3d"));
}

#[test]
fn 마지막_조각까지_읽어야_ok이고_watermark가_지금으로_간다() {
    let last = progress_cursor(5, 2);
    assert_eq!(planned(Some(last.clone()))[1], entry(BACKLOG, "newer_than:3d older_than:1d"));
    let done = parse_known(&[search(&[]), old_search(&[])], &[], Some(last));
    assert_eq!((done.outcome, code(&done), done.watermark.as_deref()), (Outcome::Ok, None, Some(T3)));
    assert_eq!(planned(Some(progress_cursor(5, 1))).len(), 1);
}

#[test]
fn 조각에_못_읽은_스레드나_남은_쪽이_있으면_표시가_전진하지_않는다() {
    let progress = |responses: &[_]| parse_known(responses, &[], Some(cursor_at(5))).progress_at;
    assert_eq!(progress(&[search(&[]), old_search(&["t1"])]), None);
    assert_eq!(progress(&[search(&[]), old_search_with(&[], json!({ "nextPageToken": "more" }))]), None);
    assert_eq!(progress(&[search(&[])]), None, "조각을 부르지 못한 틱");
}

#[test]
fn 삼십일_상한을_넘은_공백은_window_capped로_남긴다() {
    let capped = parse_known(&[search(&[]), old_search(&[])], &[], Some(cursor_at(90)));
    assert_eq!((capped.outcome, code(&capped)), (Outcome::Partial, Some("window_capped")));
    assert!(capped.progress_at.is_some());
}

#[test]
fn 밀린_메일이_있어도_최근_메일의_멘션을_같은_틱에_읽는다() {
    let result =
        parse_known(&[search(&["new1"]), thread_response("new1"), old_search(&["old1"])], &[], Some(cursor_at(5)));
    assert_eq!((result.outcome, code(&result)), (Outcome::Partial, Some("notion_read_cap")));
    assert_eq!(result.objects[0].external_id, "new1");
}

#[test]
fn 두_검색에_같은_스레드가_있어도_한_번만_저장하고_양쪽_다_처리한_것으로_본다() {
    let result = parse_known(&[search(&["t1"]), thread_response("t1"), old_search(&["t1"])], &[], Some(cursor_at(3)));
    assert_eq!(result.objects.len(), 1);
    assert!(result.progress_at.is_some());
}

/// plan-tick이 DB에서 채우는 쪽은 deferredTests에 있다. 채워진 맥락으로 건너뛰는 데까지 본다.
#[test]
fn 이미_저장한_스레드는_plan_tick이_db에서_찾아_건너뛴다_소스는_db를_열지_않는다() {
    assert_eq!(expand_of(reads(&["t1"])).skip, BTreeMap::from([("t1".to_owned(), 1)]));
}

/// 다음 틱의 상태를 DB 없이 직접 넘긴다. 저장은 deferredTests에 있다.
#[test]
fn 일부_읽기가_실패하면_읽은_것은_저장하고_partial이며_watermark가_멈춘다_다음_틱에_그_스레드만_읽는다() {
    let failed = error_response(&format!("{SEARCH}!t2"), "synthetic failure");
    let first = parse(&[search(&["t1", "t2"]), thread_response("t1"), failed]);
    assert_eq!((first.outcome, code(&first)), (Outcome::Partial, Some("mcp_tool_error")));
    assert_eq!(first.watermark, None);
    assert_eq!(first.objects.len(), 1);

    let stuck =
        ThreadReadState { message_count: None, failures: 1, first_failed_at: Some(T3.to_owned()), ..state("t2") };
    let states = vec![state("t1"), stuck];
    let expand = expand_of(states.clone());
    assert_eq!(expand.skip, BTreeMap::from([("t1".to_owned(), 1)]));
    assert_eq!(expand.retry_ids, ["t2"]);
    let second = parse_at(&[search(&["t1", "t2"]), thread_response("t2")], states, None, T3);
    assert_eq!((second.outcome, code(&second)), (Outcome::Ok, None));
    assert_eq!(second.objects.len(), 1);
}

#[test]
fn 상한으로_못_읽은_스레드는_partial이고_코드가_다르다() {
    let parsed = parse(&[search(&["t1", "t2", "t3"]), thread_response("t1")]);
    assert_eq!((parsed.outcome, code(&parsed)), (Outcome::Partial, Some("notion_read_cap")));
    assert_eq!((parsed.watermark, parsed.objects.len()), (None, 1));
}

#[test]
fn 검색이_저장된_큰_응답이라_후속_읽기가_없었어도_조용히_ok가_되지_않는다() {
    let parsed = parse(&[search(&["t1"])]);
    assert_eq!((parsed.outcome, code(&parsed)), (Outcome::Partial, Some("notion_read_cap")));
}

#[test]
fn 검색에_다음_쪽이_남으면_partial이다() {
    let parsed = parse(&[search_with(&["t1"], json!({ "nextPageToken": "more" })), thread_response("t1")]);
    assert_eq!((parsed.outcome, code(&parsed), parsed.watermark.clone()), (Outcome::Partial, Some("page_cap"), None));
}

#[test]
fn 검색_자체가_실패하면_저장_없이_그_실패로_드러난다() {
    let parsed = parse(&[error_response(SEARCH, "kairos:recover_missing")]);
    assert_eq!((parsed.outcome, code(&parsed)), (Outcome::ParseError, Some("large_response_missing")));
    assert!(parsed.events.is_empty());
}

#[test]
fn get_thread에_다른_주소만_있으면_저장하지_않고_건수_코드만_남기며_경보도_멈춤도_없다() {
    let other = thread_response_with("t2", vec![message_with(&digest(), json!({ "sender": "other@example.com" }))]);
    let parsed = parse(&[search(&["t1", "t2"]), thread_response("t1"), other]);
    assert_eq!((parsed.outcome, code(&parsed)), (Outcome::Ok, Some("notion_other_sender")));
    let ids: Vec<&str> = parsed.objects.iter().map(|object| object.external_id.as_str()).collect();
    assert_eq!(ids, ["t1"]);
    assert!(parsed.watermark.is_some());
    let reports: Vec<_> = parsed.thread_reads.iter().map(|read| (read.external_id.as_str(), read.outcome)).collect();
    assert_eq!(reports, [("t1", ThreadReadOutcome::Read), ("t2", ThreadReadOutcome::Read)]);
}

#[test]
fn 검색에_다른_주소만_보이면_읽지_않고_멈추지_않는다() {
    let listed = search_thread("t1", "\"notify@mail.notion.com\" <other@example.com>", 1);
    let parsed = parse(&[json_response(SEARCH, &json!({ "threads": [listed] }))]);
    assert_eq!((parsed.outcome, code(&parsed), parsed.objects.len()), (Outcome::Ok, Some("notion_other_sender"), 0));
}

#[test]
fn 표시_이름이_붙은_notion_주소는_놓치지_않고_읽어_저장한다() {
    let named = format!("Notion <{SENDER}>");
    let listed = json_response(SEARCH, &json!({ "threads": [search_thread("t1", &named, 1)] }));
    let unread = parse(std::slice::from_ref(&listed));
    assert_eq!((unread.outcome, code(&unread)), (Outcome::Partial, Some("notion_read_cap")));
    let read = parse(&[listed, thread_response_with("t1", vec![message_with(&digest(), json!({ "sender": named }))])]);
    assert_eq!((read.outcome, read.objects.len()), (Outcome::Ok, 1));
}

#[test]
fn 주소를_읽을_수_없는_발신자는_전진하지_않고_덮지_못한_구간으로_드러난다() {
    let broken = "Notion Notifications";
    let parsed = parse(&[
        json_response(SEARCH, &json!({ "threads": [search_thread("t1", broken, 1)] })),
        thread_response_with("t1", vec![message_with(&digest(), json!({ "sender": broken }))]),
    ]);
    assert_eq!((parsed.outcome, code(&parsed)), (Outcome::Partial, Some("notion_sender_mismatch")));
    assert_eq!((parsed.watermark, parsed.objects.len()), (None, 0));
}

#[test]
fn notion_메일_뒤에_답장이_붙은_스레드도_읽는다_검색_목록이_잘렸어도_마찬가지다() {
    let mixed =
        json!({ "id": "t1", "messageCount": 2, "messages": [{ "sender": SENDER }, { "sender": "me@example.com" }] });
    let cut = json!({ "id": "t2", "messageCount": 2, "messages": [{ "sender": "me@example.com" }] });
    let answered =
        || vec![message(&digest()), message_with("합성 답장 저장금지", json!({ "sender": "me@example.com" }))];
    let parsed = parse(&[
        json_response(SEARCH, &json!({ "threads": [mixed, cut] })),
        thread_response_with("t1", answered()),
        thread_response_with("t2", answered()),
    ]);
    assert_eq!((parsed.outcome, parsed.objects.len()), (Outcome::Ok, 2));
    assert_eq!(parsed.thread_reads[0].message_count, Some(2));
}

#[test]
fn 검색_결과_밖의_스레드_응답은_받지_않는다() {
    let parsed = parse(&[search(&["t1"]), thread_response("t1"), thread_response("stranger")]);
    let ids: Vec<&str> = parsed.objects.iter().map(|object| object.external_id.as_str()).collect();
    assert_eq!(ids, ["t1"]);
}

#[test]
fn 응답의_id가_요청과_다르면_그_스레드는_읽지_못한_것이다() {
    let wrong = json_response(&format!("{SEARCH}!t1"), &json!({ "id": "other", "messages": [message(&digest())] }));
    let parsed = parse(&[search(&["t1"]), wrong]);
    assert_eq!(
        (parsed.outcome, code(&parsed), parsed.objects.len()),
        (Outcome::Partial, Some("notion_thread_shape"), 0)
    );
}

#[test]
fn 스레드_하나의_해석_실패가_다른_스레드를_막지_않는다() {
    let broken = text_response(&format!("{SEARCH}!t2"), "not json");
    let parsed = parse(&[search(&["t1", "t2"]), thread_response("t1"), broken]);
    assert_eq!((parsed.outcome, code(&parsed), parsed.objects.len()), (Outcome::Partial, Some("not_json"), 1));
}

/// 다음 틱의 상태를 DB 없이 직접 넘긴다. 저장과 plan-tick은 deferredTests에 있다.
#[test]
fn 두_통짜리_스레드는_저장한_메시지_수까지_다시_읽지_않아_ok가_막히지_않는다() {
    let ids: Vec<String> = (0..13).map(|index| format!("t{index}")).collect();
    let two =
        |id: &str| json!({ "id": id, "messageCount": 2, "messages": [{ "sender": SENDER }, { "sender": SENDER }] });
    let listing = json_response(SEARCH, &json!({ "threads": ids.iter().map(|id| two(id)).collect::<Vec<_>>() }));
    let tick = |read: &[String], states: Vec<ThreadReadState>| {
        let mut responses = vec![listing.clone()];
        let body = || vec![message(&digest()), message(&digest())];
        responses.extend(read.iter().map(|id| thread_response_with(id, body())));
        parse_at(&responses, states, None, T3)
    };
    let stored = |ids: &[String]| -> Vec<ThreadReadState> {
        ids.iter().map(|id| ThreadReadState { message_count: Some(2), ..state(id) }).collect()
    };
    let first = tick(&ids[..12], vec![]);
    assert_eq!(first.outcome, Outcome::Partial);
    assert!(first.thread_reads.iter().all(|read| read.message_count == Some(2)));
    assert_eq!(tick(&ids[12..], stored(&ids[..12])).outcome, Outcome::Ok);
    assert_eq!(expand_of(stored(&ids)).skip.get("t0"), Some(&2));
    assert_eq!(tick(&[], stored(&ids)).outcome, Outcome::Ok);
}

#[test]
fn 한_통짜리_저장_스레드는_읽지_않아도_ok다() {
    let parsed = parse_known(&[search(&["t1"])], &["t1"], None);
    assert_eq!((parsed.outcome, code(&parsed), parsed.events.len()), (Outcome::Ok, None, 0));
}

/// 다음 틱의 상태를 DB 없이 직접 넘긴다. 저장은 deferredTests에 있다.
#[test]
fn 본문이_비면_읽은_것으로_치지_않고_다음_틱에_다시_읽어_저장한다() {
    let mut no_body = message("");
    no_body.as_object_mut().unwrap().remove("plaintextBody");
    for body in [no_body, message(""), message("   \n ")] {
        let parsed = parse(&[search(&["t1"]), thread_response_with("t1", vec![body])]);
        assert_eq!((parsed.outcome, code(&parsed), parsed.objects.len()), (Outcome::Partial, Some("notion_body"), 0));
    }
    let partly = parse(&[search(&["t1"]), thread_response_with("t1", vec![message(""), message(&digest())])]);
    assert_eq!(partly.objects.len(), 0);
    assert_eq!(partly.thread_reads[0].outcome, ThreadReadOutcome::Failed);
    let retry = ThreadReadState { message_count: None, failures: 1, ..state("t1") };
    let expand = expand_of(vec![retry.clone()]);
    assert!(expand.skip.is_empty());
    assert_eq!(expand.retry_ids, ["t1"]);
    let read = parse_at(&[search(&["t1"]), thread_response("t1")], vec![retry], None, T3);
    assert_eq!(read.objects.len(), 1);
}

fn minutes_after(minutes: i64) -> String {
    at(T3).plus_ms(minutes * 60_000).to_iso()
}

/// 실패 횟수를 쌓는 저장은 deferredTests에 있다. 쌓인 상태로 판정을 본다.
#[test]
fn 같은_스레드가_한_시간_넘게_실패하면_격리하고_고지한_뒤_전진한다_메시지가_붙으면_다시_읽는다() {
    let failing =
        ThreadReadState { message_count: None, failures: 4, first_failed_at: Some(T3.to_owned()), ..state("bad") };
    let fail = error_response(&format!("{SEARCH}!bad"), "synthetic failure");
    let responses = [search(&["bad"]), fail];
    let early = parse_at(&responses, vec![failing.clone()], None, &minutes_after(12));
    assert_eq!(early.outcome, Outcome::Partial, "횟수만 차고 한 시간이 지나지 않았다");
    let few = ThreadReadState { failures: 3, ..failing.clone() };
    assert_eq!(parse_at(&responses, vec![few], None, &minutes_after(70)).outcome, Outcome::Partial, "시간만 지났다");

    let quarantined = parse_at(&responses, vec![failing], None, &minutes_after(70));
    assert_eq!((quarantined.outcome, code(&quarantined)), (Outcome::Ok, Some("notion_quarantined")));
    assert_eq!(quarantined.thread_reads[0].outcome, ThreadReadOutcome::Quarantined);
    assert_eq!(quarantined.thread_reads[0].message_count, Some(1));

    let settled = ThreadReadState { quarantined_at: Some(minutes_after(70)), ..state("bad") };
    assert_eq!(expand_of(vec![settled.clone()]).skip, BTreeMap::from([("bad".to_owned(), 1)]));
    let later = parse_at(&[search(&["bad"])], vec![settled.clone()], None, &minutes_after(73));
    assert_eq!((later.outcome, code(&later)), (Outcome::Ok, Some("notion_quarantined")));
    let grown = json_response(SEARCH, &json!({ "threads": [search_thread("bad", SENDER, 2)] }));
    assert_eq!(parse_at(&[grown], vec![settled], None, &minutes_after(76)).outcome, Outcome::Partial);
}

#[test]
fn 인증_실패_한도_시작_못_한_호출은_스레드의_실패로_세지_않는다() {
    for text in ["401 unauthorized", "429 rate limit", "kairos:timeout"] {
        let parsed = parse(&[search(&["t1"]), error_response(&format!("{SEARCH}!t1"), text)]);
        assert_eq!(parsed.outcome, Outcome::Partial, "{text}");
        assert!(parsed.thread_reads.is_empty(), "{text}");
    }
    let hung = parse(&[search(&["t1"]), error_response(&format!("{SEARCH}!t1"), "kairos:hung")]);
    assert_eq!(hung.thread_reads[0].outcome, ThreadReadOutcome::Failed);
}

/// plan-tick과 저장은 deferredTests에 있다. 첫 시도 커서로 창을 본다.
#[test]
fn 첫_ok가_늦어지면_창이_첫_시도까지_넓어져_읽지_못한_메일이_창에서_빠지지_않는다() {
    let first_try = CursorState { watermark: None, t0: None, first_run_at: Some(T3.to_owned()), ..cursor_at(0) };
    let source = source();
    let later = at(T3).plus_ms(3 * DAY_MS).to_iso();
    let queries: Vec<String> = source
        .plan(&context_of(&source, &later, vec![first_try]))
        .iter()
        .map(|call| call.args["query"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(queries, [format!("from:{SENDER} newer_than:2d"), format!("from:{SENDER} newer_than:4d older_than:2d")]);
}
