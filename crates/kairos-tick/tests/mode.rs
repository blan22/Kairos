//! 수집 모드의 판정과 환경 시험.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::fakes::*;
use common::*;
use kairos_core::model::{Outcome, SourceName};
use kairos_core::process::{ProcessEnd, ProcessOutput, SystemProcessRunner};
use kairos_tick::{ClaudeMode, ClaudeProbe, ClaudeRun, RunVerdict, SystemClaudeProbe, judge_claude_run};
use serde_json::{Value, json};

fn run(over: Value) -> ClaudeRun {
    ClaudeRun { exit_code: Some(0), timed_out: false, missing: false, stdout: claude_stdout(over) }
}

fn judge(run: &ClaudeRun) -> RunVerdict {
    judge_claude_run(run)
}

#[test]
fn judge_claude_run은_drop_계약을_모두_만족할_때만_ok() {
    assert!(judge(&run(json!({}))).ok);
    assert!(judge(&run(json!({ "num_turns": 1 }))).spend);
    assert!(judge(&run(json!({ "total_cost_usd": 0.01 }))).spend);
    let mut no_cost: Value = serde_json::from_str(&claude_stdout(json!({}))).unwrap();
    no_cost.as_object_mut().unwrap().remove("total_cost_usd");
    assert!(judge(&ClaudeRun { stdout: no_cost.to_string(), ..run(json!({})) }).spend);
    assert_eq!(judge(&run(json!({ "result": "ok" }))).error_code.as_deref(), Some("drop_contract_broken"));
    assert!(!judge(&run(json!({ "is_error": true }))).ok);
    let unparsable = ClaudeRun { stdout: "not json".into(), ..run(json!({})) };
    assert_eq!(judge(&unparsable).error_code.as_deref(), Some("claude_output_unparsable"));
    assert_eq!(judge(&ClaudeRun { timed_out: true, ..run(json!({})) }).outcome, Outcome::Timeout);
    assert_eq!(judge(&ClaudeRun { missing: true, ..run(json!({})) }).outcome, Outcome::ToolMissing);
}

#[test]
fn 값의_형식이_다르면_지출로_보고_코드_0이_아니면_계약_위반이다() {
    assert!(judge(&run(json!({ "num_turns": "0" }))).spend);
    assert!(judge(&ClaudeRun { stdout: "[]".into(), ..run(json!({})) }).spend);
    assert!(judge(&ClaudeRun { stdout: "1".into(), ..run(json!({})) }).spend);
    let failed = ClaudeRun { exit_code: Some(1), ..run(json!({})) };
    assert_eq!(judge(&failed).error_code.as_deref(), Some("drop_contract_broken"));
    assert!(!judge(&failed).spend);
    let turns = judge(&run(json!({ "num_turns": 2, "total_cost_usd": 0.5 })));
    assert_eq!((turns.num_turns, turns.cost_usd), (Some(2.0), Some(0.5)));
    assert!(judge(&run(json!({ "num_turns": -0.0, "total_cost_usd": 0.0 }))).ok, "-0은 0이다");
}

#[test]
fn 범위를_넘는_숫자는_js의_infinity처럼_지출이다() {
    let huge = ClaudeRun { stdout: r#"{"num_turns":1e400,"total_cost_usd":0}"#.into(), ..run(json!({})) };
    let verdict = judge(&huge);
    assert!(verdict.spend);
    assert_eq!(verdict.error_code.as_deref(), Some("model_called"));
}

#[test]
fn 실행_끝의_모양이_판정_입력으로_바뀐다() {
    let of = |end: ProcessEnd| ClaudeRun::from(ProcessOutput::ended(end));
    assert!(of(ProcessEnd::TimedOut).timed_out);
    assert!(of(ProcessEnd::OutputTooLarge).timed_out, "execFile의 maxBuffer도 killed다");
    assert!(of(ProcessEnd::Missing).missing);
    assert_eq!(of(ProcessEnd::Signaled).exit_code, None);
    let failed = of(ProcessEnd::SpawnFailed);
    assert_eq!(judge(&failed).error_code.as_deref(), Some("claude_output_unparsable"));
}

#[test]
fn claude_인자는_스파이크_권장_조합이고_모델_포트를_막는다() {
    let args = ClaudeMode::args("/x/plugin");
    for flag in ["--tools", "--no-session-persistence", "--setting-sources", "--plugin-dir"] {
        assert!(args.iter().any(|arg| arg == flag), "{flag}");
    }
    let after = |flag: &str| args[args.iter().position(|arg| arg == flag).unwrap() + 1].clone();
    assert_eq!(after("--permission-mode"), "dontAsk");
    assert_eq!(after("--plugin-dir"), "/x/plugin");
    assert_eq!(after("--tools"), "");
    assert_eq!(after("--settings"), r#"{"enabledPlugins":{"slack@claude-plugins-official":true}}"#);
    assert_eq!(ClaudeMode::env(&BTreeMap::new(), BTreeMap::new())["ANTHROPIC_BASE_URL"], "http://224.0.0.0:9");
}

/// 훅 모듈이 적재되지 않으면 프롬프트가 이 주소로 간다. 루프백 포트는 다른 로컬 사용자가 바인드할 수 있다.
/// 멀티캐스트 목적지는 커널이 TCP 연결을 거절하므로 누가 어디서 듣고 있어도 요청이 닿지 않는다.
#[test]
fn 모델_주소는_어떤_로컬_리스너도_받을_수_없는_멀티캐스트다() {
    let url = ClaudeMode::env(&BTreeMap::new(), BTreeMap::new())["ANTHROPIC_BASE_URL"].clone();
    let authority = url.strip_prefix("http://").unwrap();
    assert!(!authority.contains('/') && !authority.contains('@'), "{url}");
    let (host, port) = authority.rsplit_once(':').unwrap();
    let ip: std::net::Ipv4Addr = host.parse().expect("이름 풀이를 거치지 않는 IP 리터럴이어야 한다");
    assert!(ip.is_multicast(), "{ip}");
    let port: u16 = port.parse().unwrap();

    // 로컬 리스너를 임시 포트에 두어도 같은 포트의 멀티캐스트 주소로는 연결이 만들어지지 않는다.
    // 0.0.0.0에 바인드하면 macOS 방화벽이 묻거나 네트워크를 막은 샌드박스에서 실패하므로 루프백에 둔다.
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let open_port = listener.local_addr().unwrap().port();
    for port in [port, open_port] {
        let tried = std::net::TcpStream::connect_timeout(&(ip, port).into(), Duration::from_secs(1));
        assert!(tried.is_err(), "{ip}:{port}에 연결됐다");
    }
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "리스너가 연결을 받았다");
}

/// 호출자가 넘긴 값이 모델 요청을 막는 값을 덮지 못한다. 덮이면 훅이 샐 때 실제 모델 호출이 나간다.
#[test]
fn 호출자_값은_모델_요청을_막는_환경을_덮지_못한다() {
    let base: BTreeMap<String, String> =
        [("PATH", "/usr/bin"), ("ANTHROPIC_BASE_URL", "https://api.anthropic.com"), ("CLAUDE_CODE_MAX_RETRIES", "10")]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .into();
    let extra: BTreeMap<String, String> = [
        ("ANTHROPIC_BASE_URL", "https://api.anthropic.com"),
        ("CLAUDE_CODE_MAX_RETRIES", "10"),
        ("MAX_MCP_OUTPUT_TOKENS", "1000"),
        ("KAIROS_HOME", "/t"),
    ]
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .into();
    let env = ClaudeMode::env(&base, extra);
    assert_eq!(env["ANTHROPIC_BASE_URL"], "http://224.0.0.0:9");
    assert_eq!(env["CLAUDE_CODE_MAX_RETRIES"], "0");
    assert_eq!(env["KAIROS_HOME"], "/t", "다른 값은 그대로 더한다");
    assert_eq!(env["MAX_MCP_OUTPUT_TOKENS"], "1000", "막는 값이 아닌 것은 호출자가 바꿀 수 있다");
}

#[test]
fn 수집기_환경에_ssh_auth_sock을_넘기지_않는다() {
    let base: BTreeMap<String, String> =
        [("PATH", "/usr/bin"), ("HOME", "/h"), ("SSH_AUTH_SOCK", "/tmp/agent"), ("SECRET", "x")]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .into();
    let env = ClaudeMode::env(&base, BTreeMap::from([("KAIROS_HOME".to_owned(), "/t".to_owned())]));
    assert_eq!(
        env.keys().map(String::as_str).collect::<Vec<_>>(),
        ["ANTHROPIC_BASE_URL", "CLAUDE_CODE_MAX_RETRIES", "HOME", "KAIROS_HOME", "MAX_MCP_OUTPUT_TOKENS", "PATH"]
    );
}

/// 이식 뒤 달라진 약속이다. 바이너리가 직접 실행되므로 KAIROS_NODE를 넘기지 않는다.
#[test]
fn 수집_모드_환경은_홈과_계약_시험과_시작_시각을_넘기고_kairos_node는_넘기지_않는다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let seen = Arc::new(std::sync::Mutex::new(None));
    let keep = seen.clone();
    let mode = runner(move |request| {
        *keep.lock().unwrap() = Some(request.clone());
        dropped(json!({}))
    });
    let base =
        BTreeMap::from([("PATH".to_owned(), "/usr/bin".to_owned()), ("KAIROS_NODE".to_owned(), "/n".to_owned())]);
    let source = mcp_fake(SourceName::Slack, tick_query());
    tick(&home, T1, vec![source]).claude(&plugin, mode).env(base).run();
    let request = seen.lock().unwrap().clone().unwrap();
    let env = request.env.unwrap();
    assert_eq!(request.program, "/x/claude");
    assert_eq!(env["KAIROS_HOME"], home.root().to_string_lossy());
    assert_eq!(env["KAIROS_CONTRACT_TEST"], "0");
    assert_eq!(env["KAIROS_TICK_STARTED_AT"], at(T1).millis().to_string());
    assert!(!env.contains_key("KAIROS_NODE"));
    assert_eq!(request.cwd.unwrap(), home.file("collect-cwd"));
    assert_eq!(request.timeout, Duration::from_millis(20_000));
    assert!(request.stdin.is_none());
}

#[test]
fn claude의_stdin을_바로_닫는다_열어_두면_입력을_기다린다() {
    let run = ClaudeMode::launch(
        &SystemProcessRunner,
        "/bin/cat",
        Vec::new(),
        BTreeMap::new(),
        Path::new("/"),
        Duration::from_secs(5),
    );
    assert_eq!((run.timed_out, run.exit_code), (false, Some(0)));
}

fn probe(stdout: &'static str, bin: &str) -> SystemClaudeProbe {
    let answer = runner(move |_| ProcessOutput::exited(0, stdout));
    SystemClaudeProbe::new(answer, Some(bin.to_owned()), BTreeMap::new())
}

#[test]
fn 로그인_만료는_문자열이고_시각으로_읽힐_때만_값이다() {
    let expiry = |stdout| probe(stdout, "/x/claude").login_expiry().unwrap();
    assert_eq!(expiry(r#"{"loginExpiresAt":"2026-04-30T00:00:00.000Z"}"#).as_deref(), Some("2026-04-30T00:00:00.000Z"));
    assert_eq!(expiry(r#"{"loginExpiresAt":"합성"}"#), None);
    assert_eq!(expiry(r#"{"loginExpiresAt":1}"#), None);
    assert_eq!(expiry("null"), None);
    assert_eq!(expiry("not json"), None);
    let empty = SystemClaudeProbe::new(missing_runner(), None, BTreeMap::new());
    assert_eq!(empty.login_expiry().unwrap(), None);
    assert_eq!(empty.fingerprint().unwrap(), None);
}

#[test]
fn 지문은_경로_mtime_버전이고_버전을_못_읽어도_값이다() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    let bin = home.file("claude");
    std::fs::write(&bin, "").unwrap();
    let path = bin.to_string_lossy().into_owned();
    let print = probe(" 1.2.3 (Claude Code)\n", &path).fingerprint().unwrap().unwrap();
    assert!(print.starts_with(&format!("{path}|")) && print.ends_with("|1.2.3 (Claude Code)"), "{print}");
    let silent = SystemClaudeProbe::new(missing_runner(), Some(path.clone()), BTreeMap::new());
    assert!(silent.fingerprint().unwrap().unwrap().ends_with('|'));
    assert_eq!(probe("x", "/nonexistent/claude").fingerprint().unwrap(), None);
}

#[test]
fn 출력이_json_null이면_지출로_보고_닫힌_쪽으로_실패한다() {
    let run = ClaudeRun { exit_code: Some(0), timed_out: false, missing: false, stdout: "null".into() };
    let verdict = judge(&run);
    assert!(!verdict.ok);
    assert!(verdict.spend);
    assert_eq!(verdict.error_code.as_deref(), Some("model_called"));
}
