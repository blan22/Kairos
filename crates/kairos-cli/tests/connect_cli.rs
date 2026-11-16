//! 계정 연동. 실제 gh와 키체인은 부르지 않는다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;

use common::*;

fn connections(cli: &Cli) -> String {
    fs::read_to_string(cli.root.join("connections.toml")).unwrap()
}

#[test]
fn github은_gh_로그인_사용자를_기록한다() {
    let mut cli = Cli::new();
    let gh = cli.root.join("gh-login");
    write_script(&gh, "#!/bin/sh\ncase \"$2\" in\nuser) printf 'New-Login\\n' ;;\n*) printf 'active\\n' ;;\nesac\n");
    cli.set("KAIROS_GH", gh.to_str().unwrap());
    let done = cli.run(&["connect", "github"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(connections(&cli).contains("github = \"New-Login\""));
    let show = cli.run(&["config", "show"]);
    assert!(show.out.contains("GitHub 로그인: new-login"), "{}", show.out);
}

#[test]
fn 잘못된_값은_저장하지_않는다() {
    let cli = Cli::new();
    let before = connections(&cli);
    let done = cli.run(&["connect", "slack", "not-an-id"]);
    assert_eq!(done.status, 1);
    assert!(done.out.contains("config_invalid_value"), "{}", done.out);
    assert_eq!(connections(&cli), before);
    let write = cli.run(&["connect", "mcp", "gmail", "my gmail", "mail.search=send_message"]);
    assert_eq!(write.status, 1);
    assert!(write.out.contains("config_write_tool"), "{}", write.out);
    assert_eq!(connections(&cli), before);
}

#[test]
fn 바인딩은_수집_계획의_서버와_도구를_바꾼다() {
    let cli = Cli::new();
    let done = cli.run(&["connect", "mcp", "gmail", "my gmail", "mail.search=find_mail"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    let status = cli.json(&["connect", "--json"]);
    assert_eq!(status.body()["bindings"]["gmail"]["server"], "my gmail");
    let reset = cli.run(&["connect", "mcp", "gmail", "--reset"]);
    assert_eq!(reset.status, 0);
    assert_eq!(cli.json(&["connect", "--json"]).body()["bindings"]["gmail"]["server"], "claude.ai Gmail");
}

#[test]
fn 세션_안에서는_연동을_바꾸지_않는다() {
    let cli = Cli::new();
    let done = cli.run_with(&[("CLAUDECODE", "1")], &["connect", "name", "홍길동"]);
    assert_eq!(done.status, 1);
    assert!(!connections(&cli).contains("홍길동"));
}

/// 세션은 GitHub 토큰을 쓰지 않는다. 예전 등록 명령은 사용법 오류이고 키체인을 부르지 않는다.
#[test]
fn 세션_토큰은_더_이상_받지_않는다() {
    let mut cli = Cli::new();
    let (security, log) = (cli.root.join("fake-security"), cli.root.join("security.log"));
    write_script(&security, &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\n", log = log.display()));
    cli.set("KAIROS_SECURITY", security.to_str().unwrap());
    let done = cli.pipe(&["connect", "session-token"], format!("github_pat_{}\n", "A".repeat(82)).as_bytes());
    assert_eq!(done.status, 2, "{}{}", done.out, done.err);
    assert!(!log.exists(), "키체인을 부르지 않는다");
    let status = cli.run(&["connect", "--json"]).body();
    assert!(status.get("sessionToken").is_none());
}
