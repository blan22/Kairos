//! gh 실행 계층. 읽기 전용 호출만 만든다: auth token, api rate_limit, GET search/issues, graphql query.
//! 외부 문자열은 argv나 stdin 값으로만 간다. 셸을 거치지 않는다.
//! 토큰은 gh auth token --user kairos-dev로 받아 자식 프로세스의 GH_TOKEN으로만 넘긴다.

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use kairos_core::js;
use kairos_core::model::Outcome;
use kairos_core::ports::CancelToken;
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use regex::Regex;
use serde_json::{Value, json};

use crate::config::GithubConfig;
use crate::error::GhError;
use crate::json::text;
use crate::self_login;

const MAX_OUTPUT_BYTES: usize = 32 * 1024 * 1024;
const TOKEN_TIMEOUT: Duration = Duration::from_secs(8);
pub(crate) const CALL_TIMEOUT: Duration = Duration::from_secs(15);

static RATE_LIMITED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i-u:rate limit|abuse|HTTP 429|HTTP 403|secondary)").expect("상수 정규식"));
static AUTH_FAILED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u:HTTP 401|bad credentials|gh auth login|authentication|not logged in|no oauth token)")
        .expect("상수 정규식")
});

/// 프로세스가 스스로 끝나지 못했다. 그 밖의 비정상 종료는 stderr를 보고 가른다.
fn was_stopped(output: &ProcessOutput) -> bool {
    matches!(
        output.end,
        ProcessEnd::Missing | ProcessEnd::TimedOut | ProcessEnd::Cancelled | ProcessEnd::OutputTooLarge
    )
}

fn classify_failure(output: &ProcessOutput) -> GhError {
    match output.end {
        ProcessEnd::Missing => GhError::new(Outcome::ToolMissing, "gh_missing"),
        ProcessEnd::TimedOut | ProcessEnd::Cancelled => GhError::new(Outcome::Timeout, "gh_timeout"),
        ProcessEnd::OutputTooLarge => GhError::new(Outcome::ParseError, "gh_output_too_large"),
        ProcessEnd::Exited(_) | ProcessEnd::Signaled | ProcessEnd::SpawnFailed => classify_stderr(output),
    }
}

fn classify_stderr(output: &ProcessOutput) -> GhError {
    if RATE_LIMITED.is_match(&output.stderr) {
        return GhError::new(Outcome::RateLimited, "gh_rate_limited");
    }
    if AUTH_FAILED.is_match(&output.stderr) {
        return GhError::new(Outcome::Auth, "gh_auth");
    }
    let code = output.exit_code().map_or_else(|| "null".to_owned(), |code| code.to_string());
    GhError::new(Outcome::ParseError, format!("gh_exit_{code}"))
}

fn parse_json(text: &str) -> Result<Value, GhError> {
    js::parse_json(text).map_err(|_| GhError::new(Outcome::ParseError, "gh_bad_json"))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SearchPage {
    pub(crate) total_count: f64,
    pub(crate) incomplete: bool,
    pub(crate) ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GraphqlPage {
    pub(crate) viewer: Option<String>,
    pub(crate) nodes: Vec<Value>,
    /// errors 항목마다 path가 가리킨 nodes의 자리. 노드를 가리키지 않는 항목은 None이다.
    pub(crate) errors: Vec<Option<usize>>,
}

/// 오류의 path가 `["nodes", 자리, ...]`면 그 자리다. 질의의 노드 목록은 별칭 없이 `nodes`다.
fn node_index(error: &Value) -> Option<usize> {
    match error.get("path")?.as_array()?.as_slice() {
        [head, index, ..] if head.as_str() == Some("nodes") => usize::try_from(index.as_u64()?).ok(),
        _ => None,
    }
}

pub(crate) struct GhClient {
    runner: Arc<dyn ProcessRunner>,
    program: String,
    env: BTreeMap<String, String>,
    cancel: CancelToken,
}

impl GhClient {
    /// 토큰을 받아 호출할 준비를 한다.
    pub(crate) fn connect(
        runner: Arc<dyn ProcessRunner>,
        config: &GithubConfig,
        cancel: CancelToken,
    ) -> Result<Self, GhError> {
        let mut client = Self { runner, program: config.gh_path().to_owned(), env: config.base_env(), cancel };
        let token = client.fetch_token()?;
        // 토큰은 이 환경 변수로만 자식 프로세스에 간다. argv와 로그에 넣지 않는다.
        client.env.insert("GH_TOKEN".to_owned(), token);
        Ok(client)
    }

    fn run(&self, args: &[&str], stdin: Option<String>, timeout: Duration) -> ProcessOutput {
        // 이미 취소됐으면 원본은 프로세스의 결과와 상관없이 aborted로 끝낸다.
        let already_cancelled = self.cancel.is_cancelled();
        let mut request = ProcessRequest::new(self.program.as_str(), args.iter().copied(), timeout)
            .env(self.env.clone())
            .max_output(MAX_OUTPUT_BYTES)
            .cancel(self.cancel.clone());
        request.stdin = stdin;
        let output = self.runner.run(&request);
        if already_cancelled { ProcessOutput::ended(ProcessEnd::Cancelled) } else { output }
    }

    fn fetch_token(&self) -> Result<String, GhError> {
        let output = self.run(&["auth", "token", "--user", self_login()], None, TOKEN_TIMEOUT);
        let token = js::trim(&output.stdout);
        if was_stopped(&output) || !output.success() || token.is_empty() {
            let failure = classify_failure(&output);
            let keep = matches!(failure.outcome(), Outcome::ToolMissing | Outcome::Timeout);
            return Err(if keep { failure } else { GhError::new(Outcome::Auth, "gh_token") });
        }
        Ok(token.to_owned())
    }

    fn call(&self, args: &[&str]) -> Result<String, GhError> {
        let output = self.run(args, None, CALL_TIMEOUT);
        if was_stopped(&output) || !output.success() {
            return Err(classify_failure(&output));
        }
        Ok(output.stdout)
    }

    pub(crate) fn rate_remaining(&self) -> Result<f64, GhError> {
        let body = parse_json(&self.call(&["api", "rate_limit"])?)?;
        let search = body.get("resources").and_then(|resources| resources.get("search"));
        search
            .and_then(|search| search.get("remaining"))
            .and_then(Value::as_f64)
            .ok_or_else(|| GhError::new(Outcome::ParseError, "rate_limit_shape"))
    }

    pub(crate) fn search(&self, query: &str, page: usize) -> Result<SearchPage, GhError> {
        let (query, page) = (format!("q={query}"), format!("page={page}"));
        let args = [
            "api",
            "-X",
            "GET",
            "search/issues",
            "-f",
            &query,
            "-f",
            "per_page=100",
            "-f",
            &page,
            "-f",
            "sort=updated",
            "-f",
            "order=desc",
        ];
        let body = parse_json(&self.call(&args)?)?;
        let bad_shape = || GhError::new(Outcome::ParseError, "search_shape");
        let items = body.get("items").and_then(Value::as_array).ok_or_else(bad_shape)?;
        let total_count = body.get("total_count").and_then(Value::as_f64).ok_or_else(bad_shape)?;
        let ids = items
            .iter()
            .map(|item| text(item.get("node_id")).map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| GhError::new(Outcome::ParseError, "search_item_shape"))?;
        let incomplete = body.get("incomplete_results").and_then(Value::as_bool) == Some(true);
        Ok(SearchPage { total_count, incomplete, ids })
    }

    /// 변수를 직접 주는 질의. errors가 하나라도 있거나 gh가 실패로 끝나면 실패다. 일부만 읽은 답을 쓰지 않는다.
    pub(crate) fn query(&self, query: &str, variables: Value) -> Result<Value, GhError> {
        let payload = json!({ "query": query, "variables": variables }).to_string();
        let output = self.run(&["api", "graphql", "--input", "-"], Some(payload), CALL_TIMEOUT);
        if was_stopped(&output) || !output.success() {
            return Err(classify_failure(&output));
        }
        let mut body = parse_json(&output.stdout)?;
        if body.get("errors").is_some_and(|errors| !errors.is_null()) {
            return Err(GhError::new(Outcome::ParseError, "graphql_errors"));
        }
        match body.get_mut("data").map(Value::take) {
            Some(data @ Value::Object(_)) => Ok(data),
            _ => Err(GhError::new(Outcome::ParseError, "graphql_shape")),
        }
    }

    pub(crate) fn graphql(&self, ids: &[String], query: &str) -> Result<GraphqlPage, GhError> {
        self.graphql_within(ids, query, CALL_TIMEOUT)
    }

    /// 시간 제한을 정해 부르는 `graphql`. 노드 읽기는 소스 상한 안에 남은 시간만 준다.
    pub(crate) fn graphql_within(
        &self,
        ids: &[String],
        query: &str,
        timeout: Duration,
    ) -> Result<GraphqlPage, GhError> {
        let payload = json!({ "query": query, "variables": { "ids": ids } }).to_string();
        let output = self.run(&["api", "graphql", "--input", "-"], Some(payload), timeout);
        // gh는 GraphQL errors가 붙은 응답에도 0이 아닌 코드로 끝난다. 그래서 본문이 있으면 읽는다.
        // 다만 본문이 GraphQL 응답이 아니면 HTTP 오류다(2차 한도의 403, 5xx의 HTML). 그때는 stderr로 가른다.
        // 노드의 형식 오류로 보면 한도에 걸린 동안 나눠 읽기로 호출을 늘리고, 무고한 노드까지 의심한다.
        let failed = !output.success();
        if was_stopped(&output) || (failed && js::trim(&output.stdout).is_empty()) {
            return Err(classify_failure(&output));
        }
        let mut body =
            parse_json(&output.stdout).map_err(|error| if failed { classify_stderr(&output) } else { error })?;
        let errors = body.get("errors").and_then(Value::as_array);
        let error_nodes = errors.map_or_else(Vec::new, |errors| errors.iter().map(node_index).collect());
        let rate_limited = errors.is_some_and(|errors| {
            errors.iter().any(|error| error.get("type").and_then(Value::as_str) == Some("RATE_LIMITED"))
        });
        let viewer = body.pointer("/data/viewer/login").and_then(Value::as_str).map(str::to_owned);
        let nodes = body.get_mut("data").filter(|data| data.is_object()).and_then(|data| data.get_mut("nodes"));
        match nodes.map(Value::take) {
            Some(Value::Array(nodes)) => Ok(GraphqlPage { viewer, nodes, errors: error_nodes }),
            _ if rate_limited => Err(GhError::new(Outcome::RateLimited, "graphql_rate_limited")),
            _ if failed => Err(classify_stderr(&output)),
            _ => Err(GhError::new(Outcome::ParseError, "graphql_shape")),
        }
    }
}
