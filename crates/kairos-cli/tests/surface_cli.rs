//! 표시면 계약(hello, watch)과 MCP 서버. 호스트가 부르는 모양을 프로세스로 고정한다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use common::*;
use serde_json::{Value, json};

fn append_user_config(cli: &Cli, text: &str) {
    let path = cli.root.join("config.toml");
    let mut content = fs::read_to_string(&path).unwrap();
    content.push_str(text);
    fs::write(&path, content).unwrap();
}

#[test]
fn hello는_계약과_구성을_준다() {
    let cli = Cli::new();
    let body = cli.json(&["hello", "--json"]).body();
    assert_eq!(body["contract"], 1);
    assert_eq!(body["hosts"], json!(["orca"]));
    assert_eq!(body["sessionHost"], "orca");
    assert_eq!(body["sessions"], json!({ "enabled": false, "configChanged": false }));
    let names: Vec<&str> =
        body["sources"].as_array().unwrap().iter().map(|source| source["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["calendar", "github", "slack", "gmail", "notion"]);
    assert!(body["surfaces"].as_array().unwrap().contains(&json!("vscode")));
}

#[test]
fn next는_호스트_이름을_표면으로_받는다() {
    let cli = Cli::new();
    for surface in ["panel", "orca", "claude-code", "vscode"] {
        assert_eq!(cli.run(&["next", "--surface", surface, "--json"]).status, 0, "{surface}");
    }
    assert_eq!(cli.run(&["next", "--surface", "band", "--json"]).status, 2);
}

#[test]
fn watch는_준비_뒤에_바뀐_때만_신호를_낸다() {
    let cli = Cli::new();
    let mut child = Command::new(BIN)
        .args(["watch", "--json", "--interval", "250", "--count", "1"])
        .env_clear()
        .envs(&cli.env)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let ready: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(ready["kind"], "ready");
    thread::sleep(Duration::from_millis(400));
    cli.display_on();
    let changed: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
    assert_eq!(changed["kind"], "changed");
    assert!(changed.get("items").is_none(), "내용은 담지 않는다");
    assert!(child.wait().unwrap().success());
}

fn mcp(cli: &Cli, requests: &[Value]) -> Vec<Value> {
    let input: String = requests.iter().map(|request| format!("{request}\n")).collect();
    let done = cli.pipe(&["mcp", "serve"], input.as_bytes());
    done.out.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

#[test]
fn mcp는_기본으로_읽기_도구만_열고_켠_동작만_더한다() {
    let cli = cli_with_issue();
    let init =
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } });
    let list = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
    let answers =
        mcp(&cli, &[init.clone(), json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), list.clone()]);
    assert_eq!(answers.len(), 2, "알림에는 답하지 않는다");
    assert_eq!(answers[0]["result"]["serverInfo"]["name"], "kairos");
    let names = |answer: &Value| -> Vec<String> {
        answer["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&answers[1]), ["kairos_next", "kairos_list"]);
    let call = json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "kairos_done", "arguments": { "ids": [1] } } });
    assert_eq!(mcp(&cli, std::slice::from_ref(&call))[0]["result"]["isError"], true);

    // accept는 claude-code 호스트도 켜야 나온다. 설정을 바꾸면 세션이 잠기므로 다시 켠다.
    append_user_config(&cli, "[mcp]\nallow_actions = [\"done\", \"accept\"]\n");
    assert_eq!(names(&mcp(&cli, std::slice::from_ref(&list))[0]), ["kairos_next", "kairos_list", "kairos_done"]);
    let done = mcp(&cli, &[call])[0].clone();
    assert_eq!(done["result"]["isError"], false, "{done}");
    let text: Value = serde_json::from_str(done["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["results"][0]["status"], "changed");
    let read = json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "kairos_next", "arguments": {} } });
    let next: Value =
        serde_json::from_str(mcp(&cli, &[read])[0]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(next["schema"], 1);
}
