//! 계정 연동. 실제 gh와 키체인은 부르지 않는다.
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

#[test]
fn 세션_토큰은_stdin으로_받아_키체인에_넣고_파일을_지운다() {
    let mut cli = Cli::new();
    let (security, log) = (cli.root.join("fake-security"), cli.root.join("security.log"));
    write_script(
        &security,
        &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\ncat >> '{log}'\n", log = log.display()),
    );
    cli.set("KAIROS_SECURITY", security.to_str().unwrap());
    cli.write_session_token();
    let token = format!("github_pat_{}", "A".repeat(82));
    let classic = cli.pipe(&["connect", "session-token"], b"ghp_abc\n");
    assert_eq!(classic.status, 1);
    let done = cli.pipe(&["connect", "session-token"], format!("{token}\n").as_bytes());
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    let calls = fs::read_to_string(&log).unwrap();
    // 토큰은 인자가 아니라 stdin으로 갔다.
    assert!(calls.starts_with("-i\nadd-generic-password -U -s kairos -a session-gh-token"), "{calls}");
    assert!(!calls.lines().next().unwrap().contains("github_pat_"));
    assert!(!cli.root.join("session-gh-token").exists());
}
