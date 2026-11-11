//! 설정 명령과 설정 지문의 세션 잠금. 실행 파일은 데이터 폴더의 설정 파일에서 프로필을 읽는다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::*;

fn append_user_config(cli: &Cli, text: &str) {
    let path = cli.root.join("config.toml");
    let mut content = fs::read_to_string(&path).unwrap();
    content.push_str(text);
    fs::write(&path, content).unwrap();
}

#[test]
fn validate와_show는_층과_출처를_보여_준다() {
    let cli = Cli::new();
    let valid = cli.run(&["config", "validate"]);
    assert_eq!(valid.status, 0, "{}", valid.err);
    assert!(valid.out.starts_with("설정 정상: org.toml, config.toml, connections.toml"), "{}", valid.out);
    let show = cli.run(&["config", "show", "--origin"]);
    assert!(show.out.contains("GitHub 로그인: kairos-dev  (connections.toml)"), "{}", show.out);
    assert!(show.out.contains("GitHub 조직: contoso  (org.toml)"), "{}", show.out);
    assert!(show.out.contains("시간대: Asia/Seoul  (기본값)"), "{}", show.out);
    let json = cli.json(&["config", "show", "--json"]);
    let body = json.body();
    assert_eq!(body["profile"]["playbooks"][0]["baseBranch"], "origin/fe-stage");
    assert_eq!(body["fingerprint"].as_str().unwrap().len(), 64);
}

#[test]
fn 깨진_설정이면_다른_명령은_멈추고_json으로_답한다() {
    let cli = Cli::new();
    append_user_config(&cli, "[schedule]\ntimezone = \"Mars/Base\"\n");
    let next = cli.run(&["next", "--json"]);
    assert_eq!(next.status, 1);
    let body = next.body();
    assert_eq!(
        (&body["ok"], &body["error"]["code"]),
        (&serde_json::json!(false), &serde_json::json!("config_unknown_timezone"))
    );
    let plain = cli.run(&["list"]);
    assert_eq!(plain.status, 1);
    assert!(plain.err.starts_with("kairos 설정 오류: config.toml schedule.timezone"), "{}", plain.err);
    let validate = cli.run(&["config", "validate"]);
    assert_eq!(validate.status, 1);
}

#[test]
fn 남이_쓸_수_있는_설정은_읽지_않는다() {
    let cli = Cli::new();
    fs::set_permissions(cli.root.join("org.toml"), fs::Permissions::from_mode(0o666)).unwrap();
    let done = cli.run(&["list"]);
    assert_eq!(done.status, 1);
    assert!(done.err.contains("config_unsafe"), "{}", done.err);
}

#[test]
fn 세션_자격_설정이_바뀌면_세션이_다시_잠긴다() {
    let cli = Cli::new();
    cli.display_on();
    let on = cli.run(&["sessions", "on"]);
    assert_eq!(on.out, "세션 열림\n");
    // 근무 시간은 세션 자격과 무관하다.
    append_user_config(&cli, "[schedule]\nwork = [\"09:00\", \"18:00\"]\n");
    assert_eq!(cli.run(&["sessions"]).out, "세션 열림\n");
    // 신뢰 범위가 넓어지면 잠긴다.
    append_user_config(
        &cli,
        "[[playbooks]]\nowner = \"contoso\"\nrepos = [\"other\"]\nworkspace_repo = \"contoso/other\"\nbase_branch = \"origin/main\"\n",
    );
    let locked = cli.json(&["sessions", "--json"]);
    let body = locked.body();
    assert_eq!(body["sessions"], serde_json::json!({ "enabled": false, "configChanged": true }));
    assert!(cli.run(&["doctor"]).out.contains("세션: 잠김. 설정이 바뀌어"));
    assert_eq!(cli.run(&["sessions", "on"]).out, "세션 열림\n");
}

#[test]
fn init은_조직_프로필을_검증하고_sha256을_맞춘다() {
    let cli = Cli::new();
    for name in ["org.toml", "config.toml", "connections.toml"] {
        fs::remove_file(cli.root.join(name)).unwrap();
    }
    let org = cli.root.join("incoming.toml");
    fs::write(&org, "schema = 1\n[github]\norgs = [\"acme\"]\n").unwrap();
    let wrong = cli.run(&["config", "init", "--org", org.to_str().unwrap(), "--sha256", &"0".repeat(64)]);
    assert_eq!(wrong.status, 1);
    assert!(!cli.root.join("org.toml").exists());
    let done = cli.run(&["config", "init", "--org", org.to_str().unwrap()]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(cli.root.join("org.toml").exists() && cli.root.join("config.toml").exists());
    fs::write(&org, "schema = 1\n[github]\norgs = [\"bad org\"]\n").unwrap();
    let bad = cli.run(&["config", "init", "--org", org.to_str().unwrap(), "--force"]);
    assert_eq!(bad.status, 1);
    assert!(fs::read_to_string(cli.root.join("org.toml")).unwrap().contains("acme"));
}
