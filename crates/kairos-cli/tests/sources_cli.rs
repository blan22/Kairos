//! 소스 켜고 끄기와 처음 설정. 실제 gh, launchctl은 부르지 않는다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::*;
use serde_json::Value;

fn connections(cli: &Cli) -> String {
    fs::read_to_string(cli.root.join("connections.toml")).unwrap()
}

fn source<'a>(body: &'a Value, name: &str) -> &'a Value {
    body["sources"].as_array().unwrap().iter().find(|row| row["name"] == name).unwrap()
}

/// 설정 파일이 하나도 없는 데이터 폴더.
fn blank() -> Cli {
    let cli = Cli::new();
    for name in ["org.toml", "config.toml", "connections.toml"] {
        fs::remove_file(cli.root.join(name)).unwrap();
    }
    cli
}

fn write_user(cli: &Cli, content: &str) {
    let path = cli.root.join("config.toml");
    fs::write(&path, content).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn 목록은_소스마다_상태와_모자란_값을_보여_준다() {
    let cli = blank();
    let body = cli.json(&["sources"]).body();
    assert_eq!(body["sources"].as_array().unwrap().len(), 5);
    let slack = source(&body, "slack");
    assert_eq!(slack["state"], "needs_setup");
    assert_eq!(slack["missing"][0]["hint"], "kairos connect slack <사용자 ID>");
    let text = cli.run(&["sources"]);
    assert_eq!(text.status, 0);
    assert!(text.out.contains("◐ github: 설정 필요"), "{}", text.out);
    assert!(text.out.contains("필요: GitHub 조직"), "{}", text.out);

    let full = Cli::new().json(&["sources"]).body();
    assert!(full["sources"].as_array().unwrap().iter().all(|row| row["state"] == "ready"), "{full}");
}

#[test]
fn 끄고_켠_상태는_connections에_남고_수집_등록이_따른다() {
    let cli = Cli::new();
    let done = cli.json(&["sources", "off", "slack", "notion"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert_eq!(source(&done.body(), "slack")["state"], "off");
    assert!(connections(&cli).contains("disabled = [\"slack\", \"notion\"]"), "{}", connections(&cli));
    let show = cli.run(&["config", "show"]);
    assert!(show.out.contains("끈 소스: slack, notion"), "{}", show.out);
    // 끄기는 세션 자격을 바꾸지 않는다.
    assert!(!done.out.contains("잠깁니다"));

    let on = cli.json(&["sources", "on", "slack"]);
    assert_eq!(source(&on.body(), "slack")["state"], "ready");
    assert_eq!(source(&on.body(), "notion")["state"], "off");
}

#[test]
fn 모르는_소스와_세션_안의_호출은_거절한다() {
    let cli = Cli::new();
    let before = connections(&cli);
    let unknown = cli.run(&["sources", "off", "jira"]);
    assert_eq!(unknown.status, 2);
    assert!(unknown.out.contains("calendar, github, slack, gmail, notion"), "{}", unknown.out);
    let session = cli.run_with(&[("CLAUDECODE", "1")], &["sources", "off", "slack"]);
    assert_eq!(session.status, 1);
    assert_eq!(connections(&cli), before);
    assert_eq!(cli.run(&["sources", "off"]).status, 2);
}

#[test]
fn config에서_끈_소스는_켜지_않는다() {
    let cli = Cli::new();
    write_user(&cli, "schema = 1\n[sources]\ndisabled = [\"gmail\"]\n");
    let on = cli.json(&["sources", "on", "gmail"]);
    assert_eq!(on.status, 1);
    assert_eq!(on.body()["error"]["code"], "source_pinned_off");
    let list = cli.run(&["sources"]);
    assert!(list.out.contains("○ gmail: 끔(config.toml)"), "{}", list.out);
}

#[test]
fn setup은_조직_소스_계정을_차례로_묻는다() {
    let mut cli = blank();
    let gh = cli.root.join("gh-login");
    write_script(&gh, "#!/bin/sh\ncase \"$2\" in\nuser) printf 'Setup-Login\\n' ;;\n*) printf 'active\\n' ;;\nesac\n");
    cli.set("KAIROS_GH", gh.to_str().unwrap());
    let bin = cli.asset_bin(&[("contoso.toml", EXAMPLE_ORG)]);
    // 조직 1번과 지문 확인, Calendar 켬, GitHub 켬, Slack 끔, Gmail 켬, Notion 끔,
    // Google 주소는 한 번 틀린 뒤 바로잡음, gh 기록 예, 이름, 설치 안 함.
    let answers = "1\ny\n\n\nn\ny\nn\nnot-an-email\nsetup-user@contoso.example\n\n홍길동\nn\n";
    let done = cli.pipe_installed(&bin, &["setup"], answers.as_bytes());
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("조직 프로필을 들였습니다"), "{}", done.out);
    assert!(done.out.contains("저장하지 않음: 형식이 맞지 않습니다"), "{}", done.out);
    assert!(done.out.contains("나중에: kairos install --register"), "{}", done.out);
    // 끈 Slack의 사용자 ID는 묻지 않는다.
    assert!(!done.out.contains("Slack 사용자 ID("), "{}", done.out);
    assert!(cli.root.join("org.toml").exists());
    assert!(cli.root.join("config.toml").exists());
    let body = cli.json(&["sources"]).body();
    for (name, state) in
        [("calendar", "ready"), ("github", "ready"), ("slack", "off"), ("gmail", "ready"), ("notion", "off")]
    {
        assert_eq!(source(&body, name)["state"], state, "{name} {body}");
    }
    let show = cli.json(&["config", "show"]).body();
    assert_eq!(show["profile"]["identity"]["github"], "setup-login");
    assert_eq!(show["profile"]["identity"]["google"], "setup-user@contoso.example");
    assert_eq!(show["profile"]["identity"]["name"], "홍길동");
}

#[test]
fn setup을_다시_돌리면_엔터로_지금_값을_지킨다() {
    let cli = Cli::new();
    let before = connections(&cli);
    // 입력이 끝나면 모두 기본값이다. 설치는 기본이 아니오다.
    let done = cli.pipe(&["setup"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("org.toml이 있습니다"), "{}", done.out);
    assert!(done.out.contains("설치하지 않았습니다"), "{}", done.out);
    assert_eq!(connections(&cli), before);
}

#[test]
fn setup은_세션_안에서_돌지_않는다() {
    let cli = blank();
    let done = cli.run_with(&[("ORCA_TERMINAL_HANDLE", "x")], &["setup"]);
    assert_eq!(done.status, 1);
    assert!(!cli.root.join("config.toml").exists());
}

#[test]
fn setup은_확인하지_않은_조직_프로필을_들이지_않는다() {
    let cli = blank();
    let bin = cli.asset_bin(&[("contoso.toml", EXAMPLE_ORG)]);
    // 번호는 골랐지만 지문 확인에서 Enter(기본 아니오), 나머지는 입력 끝.
    let done = cli.pipe_installed(&bin, &["setup"], b"1\n\n");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("SHA256:"), "{}", done.out);
    assert!(!cli.root.join("org.toml").exists());
    // 입력이 바로 끝나도 들이지 않는다.
    let eof = cli.pipe_installed(&bin, &["setup"], b"");
    assert_eq!(eof.status, 0);
    assert!(!cli.root.join("org.toml").exists());
    // 지문이 다르면 들이지 않는다.
    let org = fs::canonicalize("../../examples/org.toml").unwrap();
    let wrong = cli.pipe(&["setup", "--org", org.to_str().unwrap(), "--sha256", &"0".repeat(64)], b"");
    assert!(wrong.out.contains("SHA256이 다릅니다"), "{}", wrong.out);
    assert!(!cli.root.join("org.toml").exists());
}
