use std::sync::Arc;

use kairos_core::model::Outcome;
use kairos_core::ports::CancelToken;
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest};
use serde_json::json;

use super::fake::{FnRunner, config};
use crate::client::GhClient;
use crate::error::GhError;

fn code(error: &GhError) -> (Outcome, &str) {
    (error.outcome(), error.code())
}

fn stderr(end: ProcessEnd, text: &str) -> ProcessOutput {
    ProcessOutput { end, stdout: String::new(), stderr: text.to_owned() }
}

/// 토큰은 주고 그 뒤의 호출은 `reply`로 답하는 클라이언트.
fn client(reply: impl Fn(&ProcessRequest) -> ProcessOutput + Send + Sync + 'static) -> GhClient {
    let runner =
        FnRunner(
            move |request: &ProcessRequest| {
                if request.args[0] == "auth" { ProcessOutput::exited(0, " token\n") } else { reply(request) }
            },
        );
    GhClient::connect(Arc::new(runner), &config(), CancelToken::new()).unwrap()
}

fn connect(reply: impl Fn(&ProcessRequest) -> ProcessOutput + Send + Sync + 'static) -> Result<GhClient, GhError> {
    GhClient::connect(Arc::new(FnRunner(reply)), &config(), CancelToken::new())
}

fn search_error(reply: impl Fn(&ProcessRequest) -> ProcessOutput + Send + Sync + 'static) -> GhError {
    client(reply).search("q", 1).unwrap_err()
}

#[test]
fn 실패_분류는_원본의_표와_같다() {
    let cases: [(ProcessOutput, Outcome, &str); 16] = [
        (ProcessOutput::ended(ProcessEnd::Missing), Outcome::ToolMissing, "gh_missing"),
        (ProcessOutput::ended(ProcessEnd::TimedOut), Outcome::Timeout, "gh_timeout"),
        (ProcessOutput::ended(ProcessEnd::Cancelled), Outcome::Timeout, "gh_timeout"),
        (ProcessOutput::ended(ProcessEnd::OutputTooLarge), Outcome::ParseError, "gh_output_too_large"),
        (stderr(ProcessEnd::Exited(1), "API RATE LIMIT exceeded"), Outcome::RateLimited, "gh_rate_limited"),
        (stderr(ProcessEnd::Exited(1), "abuse detection"), Outcome::RateLimited, "gh_rate_limited"),
        (stderr(ProcessEnd::Exited(1), "HTTP 429"), Outcome::RateLimited, "gh_rate_limited"),
        (stderr(ProcessEnd::Exited(1), "http 403: forbidden"), Outcome::RateLimited, "gh_rate_limited"),
        (stderr(ProcessEnd::Exited(1), "Secondary limit"), Outcome::RateLimited, "gh_rate_limited"),
        (stderr(ProcessEnd::Exited(1), "HTTP 401: Bad credentials"), Outcome::Auth, "gh_auth"),
        (stderr(ProcessEnd::Exited(1), "run gh auth login"), Outcome::Auth, "gh_auth"),
        (stderr(ProcessEnd::Exited(1), "not logged in"), Outcome::Auth, "gh_auth"),
        (stderr(ProcessEnd::Exited(7), "무엇인가 실패"), Outcome::ParseError, "gh_exit_7"),
        (ProcessOutput::ended(ProcessEnd::Signaled), Outcome::ParseError, "gh_exit_null"),
        (ProcessOutput::ended(ProcessEnd::SpawnFailed), Outcome::ParseError, "gh_exit_null"),
        (stderr(ProcessEnd::SpawnFailed, "HTTP 429"), Outcome::RateLimited, "gh_rate_limited"),
    ];
    for (output, outcome, expected) in cases {
        let error = search_error(move |_| output.clone());
        assert_eq!(code(&error), (outcome, expected));
    }
}

#[test]
fn 실패_분류의_대소문자_무시는_ascii만이다() {
    // JS /i는 비ASCII 글자를 ASCII로 접지 않는다. ſ(U+017F)와 K(U+212A)는 s와 k가 아니다.
    for text in ["abu\u{17F}e", "\u{17F}econdary", "no oauth to\u{212A}en"] {
        let error = search_error(move |_| stderr(ProcessEnd::Exited(1), text));
        assert_eq!(code(&error), (Outcome::ParseError, "gh_exit_1"), "{text}");
    }
}

#[test]
fn 토큰_실패는_없음과_시간_초과만_그대로_두고_나머지는_gh_token이다() {
    let failure = |output: ProcessOutput| connect(move |_| output.clone()).err().unwrap();
    assert_eq!(code(&failure(ProcessOutput::ended(ProcessEnd::Missing))), (Outcome::ToolMissing, "gh_missing"));
    assert_eq!(code(&failure(ProcessOutput::ended(ProcessEnd::TimedOut))), (Outcome::Timeout, "gh_timeout"));
    assert_eq!(code(&failure(ProcessOutput::ended(ProcessEnd::Cancelled))), (Outcome::Timeout, "gh_timeout"));
    let cases = [
        ProcessOutput::ended(ProcessEnd::OutputTooLarge),
        ProcessOutput::exited(0, " \n\u{FEFF}"),
        stderr(ProcessEnd::Exited(1), "HTTP 429"),
        ProcessOutput::ended(ProcessEnd::Signaled),
    ];
    for output in cases {
        assert_eq!(code(&failure(output)), (Outcome::Auth, "gh_token"));
    }
}

#[test]
fn 토큰은_앞뒤_공백을_지워_gh_token으로만_간다() {
    let gh = client(|request| {
        assert_eq!(request.env.as_ref().unwrap()["GH_TOKEN"], "token");
        ProcessOutput::exited(0, r#"{"resources":{"search":{"remaining":12}}}"#)
    });
    assert_eq!(gh.rate_remaining(), Ok(12.0));
}

#[test]
fn 프로세스_호출에_원본의_시간_제한과_출력_상한을_건다() {
    let runner = FnRunner(|request: &ProcessRequest| {
        assert_eq!(request.max_output, 32 * 1024 * 1024);
        let seconds = if request.args[0] == "auth" { 8 } else { 15 };
        assert_eq!(request.timeout.as_secs(), seconds);
        if request.args[0] == "auth" {
            ProcessOutput::exited(0, "token")
        } else {
            ProcessOutput::exited(0, r#"{"resources":{"search":{"remaining":1}}}"#)
        }
    });
    let gh = GhClient::connect(Arc::new(runner), &config(), CancelToken::new()).unwrap();
    gh.rate_remaining().unwrap();
}

#[test]
fn rate_limit_모양이_다르면_rate_limit_shape다() {
    for body in ["[]", "{}", r#"{"resources":{"search":{"remaining":"5"}}}"#, r#"{"resources":[]}"#] {
        let gh = client(move |_| ProcessOutput::exited(0, body));
        assert_eq!(code(&gh.rate_remaining().unwrap_err()), (Outcome::ParseError, "rate_limit_shape"), "{body}");
    }
    let gh = client(|_| ProcessOutput::exited(0, "not json"));
    assert_eq!(code(&gh.rate_remaining().unwrap_err()), (Outcome::ParseError, "gh_bad_json"));
}

#[test]
fn 검색_응답의_모양을_검사한다() {
    let shape = [
        "[]",
        r#"{"total_count":1}"#,
        r#"{"items":[]}"#,
        r#"{"items":{},"total_count":1}"#,
        r#"{"items":[],"total_count":"1"}"#,
    ];
    for body in shape {
        assert_eq!(code(&search_error(move |_| ProcessOutput::exited(0, body))), (Outcome::ParseError, "search_shape"));
    }
    for body in [
        r#"{"items":[{"node_id":""}],"total_count":1}"#,
        r#"{"items":[{"node_id":1}],"total_count":1}"#,
        r#"{"items":[null],"total_count":1}"#,
    ] {
        let error = search_error(move |_| ProcessOutput::exited(0, body));
        assert_eq!(code(&error), (Outcome::ParseError, "search_item_shape"));
    }
}

#[test]
fn 검색은_incomplete_results가_true일_때만_불완전이다() {
    let page = |flag: &'static str| {
        let body = format!(r#"{{"items":[],"total_count":0,"incomplete_results":{flag}}}"#);
        client(move |_| ProcessOutput::exited(0, body.clone())).search("q", 2).unwrap()
    };
    assert!(page("true").incomplete);
    assert!(!page("false").incomplete);
    assert!(!page("1").incomplete);
}

#[test]
fn 검색_argv는_원본과_같다() {
    let gh = client(|request| {
        let expected = [
            "api",
            "-X",
            "GET",
            "search/issues",
            "-f",
            "q=org:x is:open",
            "-f",
            "per_page=100",
            "-f",
            "page=2",
            "-f",
            "sort=updated",
            "-f",
            "order=desc",
        ];
        assert_eq!(request.args, expected);
        assert_eq!(request.stdin, None);
        ProcessOutput::exited(0, r#"{"items":[],"total_count":0}"#)
    });
    gh.search("org:x is:open", 2).unwrap();
}

#[test]
fn graphql_입력은_query가_앞이고_id는_변수로_간다() {
    let gh = client(|request| {
        assert_eq!(request.args, ["api", "graphql", "--input", "-"]);
        assert_eq!(request.stdin.as_deref(), Some(r#"{"query":"q\n1","variables":{"ids":["a"]}}"#));
        ProcessOutput::exited(0, r#"{"data":{"nodes":[]}}"#)
    });
    gh.graphql(&["a".to_owned()], "q\n1").unwrap();
}

#[test]
fn graphql은_종료_코드가_0이_아니어도_본문이_있으면_읽는다() {
    let gh = client(|_| ProcessOutput {
        end: ProcessEnd::Exited(1),
        stdout: json!({ "data": { "viewer": { "login": "x" }, "nodes": [null] }, "errors": [{}] }).to_string(),
        stderr: "HTTP 403".to_owned(),
    });
    let page = gh.graphql(&[], "q").unwrap();
    assert_eq!(page.viewer.as_deref(), Some("x"));
    assert_eq!(page.nodes, [serde_json::Value::Null]);
    assert_eq!(page.errors, [None]);
}

#[test]
fn graphql_오류는_path가_가리킨_노드의_자리로_남는다() {
    let errors = json!([
        { "path": ["nodes", 1, "comments"], "type": "FORBIDDEN" },
        { "path": ["viewer"] },
        { "path": ["nodes", "x"] },
        { "message": "path 없음" },
        { "path": ["nodes", 0] },
    ]);
    let body = json!({ "data": { "nodes": [null, null] }, "errors": errors }).to_string();
    let gh = client(move |_| ProcessOutput::exited(0, body.clone()));
    assert_eq!(gh.graphql(&[], "q").unwrap().errors, [Some(1), None, None, None, Some(0)]);
}

// 2차 한도의 403은 GraphQL이 아닌 본문을 stdout에 낸다. 노드의 형식 오류로 보면 그 노드를 격리해 watermark를 넘긴다.
#[test]
fn graphql_종료_코드가_0이_아니고_본문이_graphql_응답이_아니면_stderr로_분류한다() {
    let failed = |stdout: &str, stderr: &str| {
        let (stdout, stderr) = (stdout.to_owned(), stderr.to_owned());
        client(move |_| ProcessOutput { end: ProcessEnd::Exited(1), stdout: stdout.clone(), stderr: stderr.clone() })
    };
    let limited = failed(
        r#"{"message":"You have exceeded a secondary rate limit","documentation_url":"https://example.invalid"}"#,
        "gh: You have exceeded a secondary rate limit (HTTP 403)",
    );
    assert_eq!(code(&limited.graphql(&[], "q").unwrap_err()), (Outcome::RateLimited, "gh_rate_limited"));
    let html = failed("<html><body>502 Bad Gateway</body></html>", "gh: HTTP 502");
    assert_eq!(code(&html.graphql(&[], "q").unwrap_err()), (Outcome::ParseError, "gh_exit_1"));
    let timed_out = failed(r#"{"data":null,"errors":[{"message":"timeout"}]}"#, "gh: Something went wrong");
    assert_eq!(code(&timed_out.graphql(&[], "q").unwrap_err()), (Outcome::ParseError, "gh_exit_1"));
    let expired = failed(r#"{"message":"Bad credentials"}"#, "gh: Bad credentials (HTTP 401)");
    assert_eq!(code(&expired.graphql(&[], "q").unwrap_err()), (Outcome::Auth, "gh_auth"));
}

#[test]
fn graphql_종료_코드가_0이면_형식_오류는_그대로다() {
    let gh = client(|_| ProcessOutput::exited(0, "<html></html>"));
    assert_eq!(code(&gh.graphql(&[], "q").unwrap_err()), (Outcome::ParseError, "gh_bad_json"));
    let gh = client(|_| ProcessOutput::exited(0, r#"{"data":null}"#));
    assert_eq!(code(&gh.graphql(&[], "q").unwrap_err()), (Outcome::ParseError, "graphql_shape"));
}

#[test]
fn graphql_within은_준_시간_제한으로_부른다() {
    let gh = client(|request| {
        assert_eq!(request.timeout, std::time::Duration::from_millis(2_500));
        ProcessOutput::exited(0, r#"{"data":{"nodes":[]}}"#)
    });
    gh.graphql_within(&[], "q", std::time::Duration::from_millis(2_500)).unwrap();
}

#[test]
fn graphql_본문이_공백뿐이면_종료_코드로_분류한다() {
    let gh = client(|_| ProcessOutput {
        end: ProcessEnd::Exited(1),
        stdout: " \n".to_owned(),
        stderr: "HTTP 403".to_owned(),
    });
    assert_eq!(code(&gh.graphql(&[], "q").unwrap_err()), (Outcome::RateLimited, "gh_rate_limited"));
}

#[test]
fn 멈춘_프로세스는_본문이_있어도_실패다() {
    let gh = client(|_| ProcessOutput { end: ProcessEnd::TimedOut, stdout: "{}".to_owned(), stderr: String::new() });
    assert_eq!(code(&gh.graphql(&[], "q").unwrap_err()), (Outcome::Timeout, "gh_timeout"));
}

#[test]
fn graphql_모양이_다르면_한도_오류와_형식_오류를_가른다() {
    let limited = r#"{"errors":[{"type":"RATE_LIMITED"}]}"#;
    let gh = client(move |_| ProcessOutput::exited(0, limited));
    assert_eq!(code(&gh.graphql(&[], "q").unwrap_err()), (Outcome::RateLimited, "graphql_rate_limited"));
    for body in ["[]", "{}", r#"{"data":[]}"#, r#"{"data":{"nodes":{}}}"#, r#"{"errors":"RATE_LIMITED"}"#] {
        let gh = client(move |_| ProcessOutput::exited(0, body));
        assert_eq!(code(&gh.graphql(&[], "q").unwrap_err()), (Outcome::ParseError, "graphql_shape"), "{body}");
    }
}

#[test]
fn viewer는_문자열이면_빈_값도_그대로_둔다() {
    let viewer = |body: &'static str| client(move |_| ProcessOutput::exited(0, body)).graphql(&[], "q").unwrap().viewer;
    assert_eq!(viewer(r#"{"data":{"nodes":[],"viewer":{"login":""}}}"#).as_deref(), Some(""));
    assert_eq!(viewer(r#"{"data":{"nodes":[],"viewer":{"login":1}}}"#), None);
    assert_eq!(viewer(r#"{"data":{"nodes":[],"viewer":[{"login":"x"}]}}"#), None);
}

#[test]
fn 짝_없는_서로게이트가_있어도_응답을_읽는다() {
    let gh = client(|_| ProcessOutput::exited(0, r#"{"data":{"nodes":["\ud83d"]}}"#));
    assert_eq!(gh.graphql(&[], "q").unwrap().nodes, [json!("\u{FFFD}")]);
}
