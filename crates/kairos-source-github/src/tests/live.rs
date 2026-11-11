//! 세션 직전 재확인. 실제 gh를 부르지 않는다. 고정값은 합성이다.

use std::sync::{Arc, Mutex};

use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest};
use serde_json::{Value, json};

use super::fake::{FnRunner, config};
use crate::{LiveAuthorReader, LiveAuthors};

const URL: &str = "https://github.com/contoso/atlas/issues/1";
const PULL_URL: &str = "https://github.com/contoso/atlas/pull/1";

fn writing(login: Option<&str>, association: Option<&str>) -> Value {
    let author = login.map(|login| {
        let typename = if login.ends_with("[bot]") { "Bot" } else { "User" };
        json!({ "__typename": typename, "login": login })
    });
    let mut value = json!({ "author": author });
    if let Some(association) = association {
        value["authorAssociation"] = json!(association);
    }
    value
}

fn member(login: &str) -> Value {
    writing(Some(login), Some("MEMBER"))
}

fn connection(nodes: Vec<Value>, total: usize, next: Option<&str>) -> Value {
    json!({ "totalCount": total, "pageInfo": { "hasNextPage": next.is_some(), "endCursor": next }, "nodes": nodes })
}

fn complete(nodes: Vec<Value>) -> Value {
    let total = nodes.len();
    connection(nodes, total, None)
}

fn head(node: Value) -> ProcessOutput {
    ProcessOutput::exited(0, json!({ "data": { "repository": { "issueOrPullRequest": node } } }).to_string())
}

/// 조직의 사람이 연 이슈.
fn issue(comments: Value) -> Value {
    let mut node = member("synthetic-lead");
    for (key, value) in
        [("__typename", json!("Issue")), ("id", json!("I_1")), ("number", json!(1)), ("comments", comments)]
    {
        node[key] = value;
    }
    node
}

fn pull(comments: Value, reviews: Value, threads: Value) -> Value {
    let mut node = issue(comments);
    node["__typename"] = json!("PullRequest");
    node["id"] = json!("PR_1");
    node["reviews"] = reviews;
    node["reviewThreads"] = threads;
    node
}

fn page(field: &str, value: Value) -> ProcessOutput {
    ProcessOutput::exited(0, json!({ "data": { "node": { field: value } } }).to_string())
}

/// 받은 질의의 변수를 적는다. 첫 graphql은 `first`, 그 뒤는 (id, after)로 `more`에서 찾는다.
struct Script {
    first: ProcessOutput,
    more: Vec<((&'static str, &'static str), ProcessOutput)>,
    seen: Mutex<Vec<Value>>,
}

fn read_with(url: &str, script: Script) -> (Result<LiveAuthors, String>, Vec<Value>) {
    let script = Arc::new(script);
    let shared = script.clone();
    let runner = FnRunner(move |request: &ProcessRequest| {
        if request.args[0] == "auth" {
            return ProcessOutput::exited(0, "synthetic-token\n");
        }
        assert_eq!(request.args, ["api", "graphql", "--input", "-"]);
        let body: Value = serde_json::from_str(request.stdin.as_deref().unwrap()).unwrap();
        let variables = body["variables"].clone();
        shared.seen.lock().unwrap().push(variables.clone());
        if variables.get("owner").is_some() {
            return shared.first.clone();
        }
        let key = (variables["id"].as_str().unwrap().to_owned(), variables["after"].as_str().unwrap().to_owned());
        let found = shared.more.iter().find(|((id, after), _)| (*id, *after) == (key.0.as_str(), key.1.as_str()));
        found.map_or_else(|| panic!("시험에 없는 쪽: {key:?}"), |(_, output)| output.clone())
    });
    let reader = LiveAuthorReader::new(Arc::new(runner), config());
    let verdict = reader.read(url).map_err(|error| error.code().to_owned());
    let seen = script.seen.lock().unwrap().clone();
    (verdict, seen)
}

fn read(
    url: &str,
    first: ProcessOutput,
    more: Vec<((&'static str, &'static str), ProcessOutput)>,
) -> Result<LiveAuthors, String> {
    read_with(url, Script { first, more, seen: Mutex::new(Vec::new()) }).0
}

#[test]
fn 조직의_사람과_본인의_글뿐이면_통과한다() {
    let comments = complete(vec![member("synthetic-a"), writing(Some("kairos-dev"), Some("NONE"))]);
    let (verdict, seen) =
        read_with(URL, Script { first: head(issue(comments)), more: Vec::new(), seen: Mutex::new(Vec::new()) });
    assert_eq!(verdict, Ok(LiveAuthors::Insiders));
    assert_eq!(seen, [json!({ "owner": "contoso", "name": "atlas", "number": 1 })]);
}

#[test]
fn 다음_쪽의_t3_댓글을_찾는다() {
    let first: Vec<Value> = (0..100).map(|index| member(&format!("synthetic-{index}"))).collect();
    let second = complete(vec![member("synthetic-x"), writing(Some("synthetic-outsider"), Some("CONTRIBUTOR"))]);
    let more =
        vec![(("I_1", "c1"), page("comments", connection(second["nodes"].as_array().unwrap().clone(), 102, None)))];
    assert_eq!(read(URL, head(issue(connection(first, 102, Some("c1")))), more), Ok(LiveAuthors::Outsider));
}

#[test]
fn 모든_쪽을_읽고_수가_맞아야_통과한다() {
    let first: Vec<Value> = (0..100).map(|index| member(&format!("synthetic-{index}"))).collect();
    let more = vec![(("I_1", "c1"), page("comments", connection(vec![member("synthetic-last")], 101, None)))];
    assert_eq!(read(URL, head(issue(connection(first.clone(), 101, Some("c1")))), more), Ok(LiveAuthors::Insiders));

    let short = vec![(("I_1", "c1"), page("comments", connection(vec![member("synthetic-last")], 102, None)))];
    assert_eq!(read(URL, head(issue(connection(first, 102, Some("c1")))), short), Err("live_incomplete".to_owned()));
}

#[test]
fn pr은_리뷰와_리뷰_스레드의_글도_본다() {
    let thread = |author: Value| json!({ "id": "T_1", "comments": complete(vec![author]) });
    let quiet = || complete(vec![member("synthetic-a")]);
    let cases = [
        (
            "리뷰",
            pull(
                quiet(),
                complete(vec![writing(Some("synthetic-x"), Some("FIRST_TIME_CONTRIBUTOR"))]),
                complete(vec![]),
            ),
        ),
        ("스레드", pull(quiet(), quiet(), complete(vec![thread(writing(Some("synthetic-x"), Some("NONE")))]))),
        ("PR 댓글", pull(complete(vec![writing(Some("synthetic-x"), Some("MANNEQUIN"))]), quiet(), complete(vec![]))),
    ];
    for (name, node) in cases {
        assert_eq!(read(PULL_URL, head(node), Vec::new()), Ok(LiveAuthors::Outsider), "{name}");
    }
    let clean = pull(quiet(), quiet(), complete(vec![thread(member("synthetic-b"))]));
    assert_eq!(read(PULL_URL, head(clean), Vec::new()), Ok(LiveAuthors::Insiders));
}

#[test]
fn 리뷰_스레드의_다음_댓글_쪽도_읽는다() {
    let first: Vec<Value> = (0..100).map(|index| member(&format!("synthetic-{index}"))).collect();
    let thread = json!({ "id": "T_1", "comments": connection(first, 101, Some("t1")) });
    let node = pull(complete(vec![]), complete(vec![]), complete(vec![thread]));
    let more = vec![(
        ("T_1", "t1"),
        page("comments", connection(vec![writing(Some("synthetic-x"), Some("NONE"))], 101, None)),
    )];
    assert_eq!(read(PULL_URL, head(node), more), Ok(LiveAuthors::Outsider));
}

#[test]
fn 봇은_세지_않고_작성자나_연관_정보가_없는_사람은_믿지도_외부인으로_단정하지도_않는다() {
    let bot = complete(vec![writing(Some("synthetic[bot]"), Some("NONE"))]);
    assert_eq!(read(URL, head(issue(bot)), Vec::new()), Ok(LiveAuthors::Insiders), "봇은 수집 게이트와 같이 뺀다");
    let cases = [
        ("지운 계정", writing(None, Some("NONE"))),
        ("지운 조직원", writing(None, Some("MEMBER"))),
        ("ghost 작성자", writing(Some("ghost"), Some("NONE"))),
        ("연관 정보 없음", writing(Some("synthetic-x"), None)),
        ("빈 연관 정보", writing(Some("synthetic-x"), Some(""))),
        ("모르는 연관 정보", writing(Some("synthetic-x"), Some("SYNTHETIC_NEW"))),
    ];
    for (name, node) in cases {
        assert_eq!(read(URL, head(issue(complete(vec![node]))), Vec::new()), Ok(LiveAuthors::Unknown), "{name}");
    }
    let mut unknown_body = issue(complete(vec![]));
    unknown_body.as_object_mut().unwrap().remove("authorAssociation");
    assert_eq!(read(URL, head(unknown_body), Vec::new()), Ok(LiveAuthors::Unknown), "본문 작성자의 연관 정보 없음");
    let mut outsider_body = issue(complete(vec![]));
    outsider_body["authorAssociation"] = json!("NONE");
    assert_eq!(read(URL, head(outsider_body), Vec::new()), Ok(LiveAuthors::Outsider), "본문 작성자");
}

/// 알 수 없는 글 뒤에 T3 글이 있으면 외부 글이다. 앞의 알 수 없는 글에서 멈추지 않는다.
#[test]
fn 알_수_없는_글이_있어도_끝까지_읽어_t3_글을_찾는다() {
    let first: Vec<Value> =
        (0..99).map(|index| member(&format!("synthetic-{index}"))).chain([writing(None, Some("NONE"))]).collect();
    let more = vec![(
        ("I_1", "c1"),
        page("comments", connection(vec![writing(Some("synthetic-outsider"), Some("CONTRIBUTOR"))], 101, None)),
    )];
    assert_eq!(read(URL, head(issue(connection(first.clone(), 101, Some("c1")))), more), Ok(LiveAuthors::Outsider));
    let quiet = vec![(("I_1", "c1"), page("comments", connection(vec![member("synthetic-last")], 101, None)))];
    assert_eq!(read(URL, head(issue(connection(first, 101, Some("c1")))), quiet), Ok(LiveAuthors::Unknown));
}

#[test]
fn 다_읽었다고_보일_수_없으면_오류다() {
    let ok_body = |body: Value| ProcessOutput::exited(0, body.to_string());
    let hidden = issue(json!({ "totalCount": 1, "pageInfo": { "hasNextPage": false }, "nodes": [null] }));
    let mut wrong_number = issue(complete(vec![]));
    wrong_number["number"] = json!(2);
    let mut no_comments = issue(Value::Null);
    no_comments.as_object_mut().unwrap().remove("comments");
    let cases: Vec<(&str, ProcessOutput, &str)> = vec![
        ("숨은 노드", head(hidden), "live_hidden_node"),
        ("다른 번호", head(wrong_number), "live_shape"),
        ("없는 객체", head(Value::Null), "live_not_found"),
        ("연결 없음", head(no_comments), "live_shape"),
        (
            "부분 오류",
            ok_body(json!({ "data": { "repository": null }, "errors": [{ "type": "FORBIDDEN" }] })),
            "live_graphql_errors",
        ),
        ("시간 초과", ProcessOutput::ended(ProcessEnd::TimedOut), "live_gh_timeout"),
        ("gh 없음", ProcessOutput::ended(ProcessEnd::Missing), "live_gh_missing"),
        ("깨진 JSON", ProcessOutput::exited(0, "{"), "live_gh_bad_json"),
    ];
    for (name, first, code) in cases {
        assert_eq!(read(URL, first, Vec::new()), Err(code.to_owned()), "{name}");
    }
}

#[test]
fn 쪽이_끝나지_않으면_상한에서_멈춘다() {
    let endless = |after: &'static str| page("comments", connection(vec![member("synthetic-a")], 10_000, Some(after)));
    let more = vec![(("I_1", "c"), endless("c"))];
    let first = head(issue(connection(vec![member("synthetic-a")], 10_000, Some("c"))));
    assert_eq!(read(URL, first, more), Err("live_too_long".to_owned()));
}

#[test]
fn 매핑_밖의_url은_gh를_부르지_않는다() {
    for url in ["https://github.com/acme/atlas/issues/1", "https://github.com/contoso/atlas/issues/1; x", "http://x"] {
        let runner = FnRunner(|_: &ProcessRequest| -> ProcessOutput { panic!("gh를 부르면 안 된다") });
        let reader = LiveAuthorReader::new(Arc::new(runner), config());
        assert_eq!(reader.read(url).map_err(|error| error.code().to_owned()), Err("live_bad_url".to_owned()), "{url}");
    }
}
