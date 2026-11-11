//! 시험용 gh. 실제 gh를 호출하지 않는다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kairos_core::model::{CollectContext, CursorState, SourceName, SourceResult};
use kairos_core::ports::{CancelToken, FixedClock};
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_core::{Instant, js};
use serde_json::{Map, Value, json};

use crate::{GithubConfig, GithubSource, self_login};

pub(super) const NOW: &str = "2026-03-02T01:00:00.000Z";

pub(super) fn nodes() -> Value {
    let text = include_str!("../../tests/fixtures/github/nodes.json").replace("__LOGIN__", self_login());
    js::parse_json(&text).unwrap()
}

pub(super) fn by_id(id: &str) -> Value {
    let nodes = nodes();
    nodes.as_object().unwrap().values().find(|node| node["id"] == id).cloned().unwrap()
}

/// `{ ...base, ...patch }`.
pub(super) fn spread(base: &Value, patch: Value) -> Value {
    let mut merged = base.as_object().unwrap().clone();
    merged.extend(patch.as_object().unwrap().clone());
    Value::Object(merged)
}

/// I_synth_c를 본뜬 합성 이슈 count개. id는 `I_clone_0`부터, 번호는 100부터다. FakeOptions::overrides에 넣는다.
pub(super) fn clones(count: usize) -> (Vec<String>, Map<String, Value>) {
    let base = by_id("I_synth_c");
    let nodes: Map<String, Value> = (0..count)
        .map(|index| {
            let id = format!("I_clone_{index}");
            let url = format!("https://example.invalid/example-org/widget/issues/{}", 100 + index);
            (id.clone(), spread(&base, json!({ "id": id, "number": 100 + index, "url": url })))
        })
        .collect();
    (nodes.keys().cloned().collect(), nodes)
}

#[derive(Clone, Debug)]
pub(super) struct Call {
    pub(super) program: String,
    pub(super) args: Vec<String>,
    pub(super) env: BTreeMap<String, String>,
    pub(super) stdin: Option<String>,
    pub(super) has_cancel: bool,
    pub(super) timeout: Duration,
}

impl Call {
    pub(super) fn is_search(&self) -> bool {
        self.args.get(3).is_some_and(|arg| arg == "search/issues")
    }

    pub(super) fn is_graphql(&self) -> bool {
        self.args.get(1).is_some_and(|arg| arg == "graphql")
    }

    /// `-f` 뒤의 첫 값. 검색이면 `q=...`다.
    pub(super) fn first_field(&self) -> &str {
        let at = self.args.iter().position(|arg| arg == "-f").unwrap();
        &self.args[at + 1]
    }

    pub(super) fn query_text(&self) -> String {
        let body: Value = serde_json::from_str(self.stdin.as_deref().unwrap_or("{}")).unwrap();
        body["query"].as_str().unwrap_or_default().to_owned()
    }

    pub(super) fn is_expanded(&self) -> bool {
        self.is_graphql() && self.query_text() == *crate::query::EXPANDED_QUERY
    }

    /// 노드를 하나 이상 읽는 graphql 호출. 신원 확인 호출은 빈 목록이라 빠진다.
    pub(super) fn reads_nodes(&self) -> bool {
        self.is_graphql() && !self.node_ids().is_empty()
    }

    pub(super) fn node_ids(&self) -> Vec<String> {
        let body: Value = serde_json::from_str(self.stdin.as_deref().unwrap_or("{}")).unwrap();
        let ids = body["variables"]["ids"].as_array().cloned().unwrap_or_default();
        ids.iter().filter_map(Value::as_str).map(str::to_owned).collect()
    }
}

pub(super) type Hook = Box<dyn Fn(&ProcessRequest) -> Option<ProcessOutput> + Send + Sync>;

#[derive(Default)]
pub(super) struct FakeOptions {
    /// 검색어에 이 문자열이 들어 있으면 해당 id를 돌려준다.
    pub(super) search: Vec<(&'static str, Vec<String>)>,
    pub(super) viewer: Option<&'static str>,
    pub(super) search_remaining: Option<i64>,
    pub(super) fail_search: Option<&'static str>,
    pub(super) graphql_error: Option<&'static str>,
    pub(super) null_ids: Vec<&'static str>,
    pub(super) total_count: Option<i64>,
    /// 첫 질의에서 이 노드로 바꿔 돌려준다.
    pub(super) overrides: Map<String, Value>,
    /// 넓은 질의의 응답.
    pub(super) expanded: Map<String, Value>,
    /// 넓은 질의가 data와 함께 errors를 돌려준다.
    pub(super) expanded_errors: bool,
    /// 첫 질의의 응답에 이 errors를 붙인다. 묶음 안의 자리는 path가 가리킨다.
    pub(super) errors: Vec<Value>,
    /// 호출마다 먼저 불린다. Some을 돌려주면 그 값으로 끝난다.
    pub(super) hook: Option<Hook>,
}

pub(super) struct FakeGh {
    options: FakeOptions,
    calls: Mutex<Vec<Call>>,
}

fn ok(body: impl Into<String>) -> ProcessOutput {
    ProcessOutput::exited(0, body)
}

fn failed(stderr: &str) -> ProcessOutput {
    ProcessOutput { end: ProcessEnd::Exited(1), stdout: String::new(), stderr: stderr.to_owned() }
}

impl FakeGh {
    pub(super) fn new(options: FakeOptions) -> Arc<Self> {
        Arc::new(Self { options, calls: Mutex::new(Vec::new()) })
    }

    pub(super) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    fn search(&self, args: &[String]) -> ProcessOutput {
        let at = args.iter().position(|arg| arg == "-f").unwrap();
        let q = &args[at + 1];
        if self.options.fail_search.is_some_and(|needle| q.contains(needle)) {
            return failed("HTTP 403: API rate limit exceeded");
        }
        let hit = self.options.search.iter().find(|(needle, _)| q.contains(needle));
        let ids = hit.map_or(&[][..], |(_, ids)| ids.as_slice());
        let items: Vec<Value> = ids.iter().map(|id| json!({ "node_id": id })).collect();
        let total_count = self.options.total_count.unwrap_or(ids.len() as i64);
        ok(json!({ "total_count": total_count, "incomplete_results": false, "items": items }).to_string())
    }

    fn graphql(&self, stdin: Option<&str>) -> ProcessOutput {
        if let Some(stderr) = self.options.graphql_error {
            return failed(stderr);
        }
        let body: Value = serde_json::from_str(stdin.unwrap_or("{}")).unwrap();
        let expanded = body["query"].as_str() == Some(crate::query::EXPANDED_QUERY.as_str());
        let node = |id: &str| -> Value {
            if expanded {
                return self.options.expanded.get(id).cloned().unwrap_or(Value::Null);
            }
            if self.options.null_ids.contains(&id) {
                return Value::Null;
            }
            let fixture = nodes().as_object().unwrap().values().find(|node| node["id"] == id).cloned();
            self.options.overrides.get(id).cloned().or(fixture).unwrap_or(Value::Null)
        };
        let ids = body["variables"]["ids"].as_array().unwrap();
        let found: Vec<Value> = ids.iter().map(|id| node(id.as_str().unwrap())).collect();
        let viewer = self.options.viewer.unwrap_or(self_login());
        let data = json!({ "viewer": { "login": viewer }, "nodes": found });
        if expanded && self.options.expanded_errors {
            return ok(json!({ "data": data, "errors": [{ "type": "FORBIDDEN" }] }).to_string());
        }
        if !expanded && !ids.is_empty() && !self.options.errors.is_empty() {
            return ok(json!({ "data": data, "errors": self.options.errors }).to_string());
        }
        ok(json!({ "data": data }).to_string())
    }
}

impl ProcessRunner for FakeGh {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        self.calls.lock().unwrap().push(Call {
            program: request.program.clone(),
            args: request.args.clone(),
            env: request.env.clone().unwrap_or_default(),
            stdin: request.stdin.clone(),
            has_cancel: request.cancel.is_some(),
            timeout: request.timeout,
        });
        if let Some(output) = self.options.hook.as_ref().and_then(|hook| hook(request)) {
            return output;
        }
        let arg = |index: usize| request.args.get(index).map(String::as_str);
        match (arg(0), arg(1), arg(3)) {
            (Some("auth"), ..) => ok("synthetic-token\n"),
            (_, Some("rate_limit"), _) => {
                let remaining = self.options.search_remaining.unwrap_or(30);
                ok(json!({ "resources": { "search": { "remaining": remaining } } }).to_string())
            }
            (.., Some("search/issues")) => self.search(&request.args),
            (_, Some("graphql"), _) => self.graphql(request.stdin.as_deref()),
            _ => failed("unexpected"),
        }
    }
}

/// 호출 전부를 함수 하나로 답하는 러너.
pub(super) struct FnRunner<F>(pub(super) F);

impl<F: Fn(&ProcessRequest) -> ProcessOutput + Send + Sync> ProcessRunner for FnRunner<F> {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        (self.0)(request)
    }
}

pub(super) fn config() -> GithubConfig {
    GithubConfig::from_vars(|key| match key {
        "PATH" => Some("/synthetic/bin".to_owned()),
        "HOME" => Some("/synthetic/home".to_owned()),
        "LANG" => Some("ko_KR.UTF-8".to_owned()),
        _ => None,
    })
}

pub(super) fn source(runner: Arc<dyn ProcessRunner>) -> GithubSource {
    GithubSource::new(runner, Arc::new(FixedClock(now())), config())
}

pub(super) fn now() -> Instant {
    Instant::parse(NOW).unwrap()
}

pub(super) fn context(cursors: Vec<(&str, CursorState)>) -> CollectContext {
    CollectContext {
        now: now(),
        cursors: cursors.into_iter().map(|(id, cursor)| (id.to_owned(), Some(cursor))).collect(),
        thread_reads: None,
        cancel: CancelToken::new(),
    }
}

pub(super) fn cursor(watermark: &str, t0: Option<&str>) -> CursorState {
    CursorState {
        source: SourceName::Github,
        query: "mentions".to_owned(),
        watermark: Some(watermark.to_owned()),
        overlap_seconds: 300,
        t0: t0.map(str::to_owned),
        first_run_at: None,
        progress_at: None,
    }
}

pub(super) fn query<'a>(results: &'a [SourceResult], id: &str) -> &'a SourceResult {
    results.iter().find(|result| result.query == id).unwrap()
}

pub(super) fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub(super) fn search_table() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("review-requested", ids(&["PR_synth_b"])),
        ("assignee:", ids(&["I_synth_c"])),
        ("author:", ids(&["PR_synth_a", "PR_synth_b"])),
        ("mentions:", ids(&["I_synth_c", "PR_synth_a"])),
        ("is:closed", ids(&["PR_synth_d", "I_synth_e"])),
    ]
}
