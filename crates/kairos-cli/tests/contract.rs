//! 명령 틀의 약속. 인자 분기를 프로세스로 고정한다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;

use common::*;
use serde_json::json;

const NAMES: &str = "hello, list, next, watch, reveal, accept, move, pr, runs, later, done, keep, add, triage, focus, display, sessions, notify, doctor, pause, off, collect, plan-tick, page-info, ingest, setup, config, sources, mcp, connect, install, uninstall, demo, update, completions";

#[test]
fn 도움말은_명령을_묶어_설명하고_내부_명령은_all에만_나온다() {
    let cli = Cli::new();
    let help = cli.run(&["help"]);
    assert_eq!((help.status, help.err.as_str()), (0, ""));
    assert!(help.out.starts_with(&format!("kairos {}\n", env!("CARGO_PKG_VERSION"))), "{}", help.out);
    assert!(help.out.contains("사용법: kairos <명령> [옵션]"), "{}", help.out);
    assert!(help.out.contains("\n매일 쓰는 명령\n  next "), "{}", help.out);
    let internal = ["hello", "watch", "reveal", "plan-tick", "page-info", "ingest", "mcp"];
    let listed = |out: &str, name: &str| out.lines().any(|line| line.starts_with(&format!("  {name} ")));
    for name in NAMES.split(", ") {
        assert_eq!(listed(&help.out, name), !internal.contains(&name), "{name}\n{}", help.out);
    }
    for same in [&["--help"][..], &["-h"]] {
        let flag = cli.run(same);
        assert_eq!((flag.status, flag.out.as_str()), (0, help.out.as_str()));
    }
    let none = cli.run(&[]);
    assert_eq!((none.status, none.out.as_str()), (2, help.out.as_str()));
    let all = cli.run(&["help", "--all"]);
    assert_eq!(all.status, 0);
    for name in NAMES.split(", ") {
        assert!(listed(&all.out, name), "{name}\n{}", all.out);
    }
}

#[test]
fn 판_번호를_찍는다() {
    let cli = Cli::new();
    let expected = format!("kairos {}\n", env!("CARGO_PKG_VERSION"));
    for args in [&["--version"][..], &["-V"], &["version"]] {
        let done = cli.run(args);
        assert_eq!((done.status, done.out.as_str(), done.err.as_str()), (0, expected.as_str(), ""), "{args:?}");
    }
}

#[test]
fn 명령_하나의_도움말은_설명과_사용법_줄이다() {
    let cli = Cli::new();
    let later = "kairos later: 나중에 다시 본다\n사용법: kairos later <항목 ID>... [--json]\n";
    for args in [&["help", "later"][..], &["later", "--help"], &["later", "-h"], &["later", "--json", "--help"]] {
        let done = cli.run(args);
        assert_eq!((done.status, done.out.as_str(), done.err.as_str()), (0, later, ""), "{args:?}");
    }
    // usage()가 없는 명령은 표의 줄을 쓴다.
    let doctor = cli.run(&["doctor", "--help"]);
    assert_eq!(
        (doctor.status, doctor.out.as_str()),
        (0, "kairos doctor: 수집과 설정 상태를 점검한다\n사용법: kairos doctor\n")
    );
    // `--` 뒤는 값이다. 도움말로 보지 않는다.
    let value = cli.run(&["later", "--", "--help"]);
    assert_eq!((value.status, value.out.as_str()), (2, "항목 ID는 양의 정수입니다\n"));
    let unknown = cli.run(&["help", "nope"]);
    assert_eq!(unknown.status, 2);
    assert!(unknown.out.starts_with("알 수 없는 명령: nope\n"), "{}", unknown.out);
}

#[test]
fn 잘못_친_명령에는_가까운_이름을_권한다() {
    let cli = Cli::new();
    let done = cli.run(&["nxt"]);
    assert_eq!(done.status, 2);
    assert_eq!(done.out, "알 수 없는 명령: nxt\n혹시 이 명령인가요: kairos next\n명령 목록은 kairos help로 봅니다\n");
    let far = cli.run(&["send-it"]);
    assert_eq!(far.out, "알 수 없는 명령: send-it\n명령 목록은 kairos help로 봅니다\n");
}

#[test]
fn 모르는_명령은_정제해서_표준_출력에_찍는다() {
    let done = Cli::new().run(&["보내\u{1b}[31m기"]);
    assert_eq!(done.status, 2);
    assert!(done.out.starts_with("알 수 없는 명령: 보내기\n"), "{}", done.out);
    assert_eq!(done.err, "");
}

#[test]
fn object_prototype_이름도_모르는_명령이다() {
    let cli = Cli::new();
    for name in ["constructor", "toString", "__proto__", "valueOf", "hasOwnProperty"] {
        let done = cli.run(&[name]);
        assert_eq!(done.status, 2, "{name}");
        assert!(done.out.starts_with(&format!("알 수 없는 명령: {name}\n")), "{}", done.out);
    }
}

#[test]
fn 옵션_오류는_json을_주면_usage이고_없으면_표준_오류다() {
    let cli = Cli::new();
    let json = cli.run(&["list", "--bogus", "--json"]);
    assert_eq!(json.status, 2);
    assert_eq!(
        json.body(),
        json!({ "schema": 1, "ok": false, "error": { "code": "usage", "message": "사용법: kairos list [--would-show | --items | --json [--all]]" } })
    );
    let plain = cli.run(&["list", "--bogus"]);
    assert_eq!(plain.status, 1);
    assert_eq!(plain.out, "");
    // 첫 줄은 코드, 둘째 줄은 받는 옵션이다.
    let list_usage = "사용법: kairos list [--would-show | --items | --json [--all]]\n";
    assert_eq!(plain.err, format!("kairos list 실패: ERR_PARSE_ARGS_UNKNOWN_OPTION\n{list_usage}"));
    let positional = cli.run(&["list", "x"]);
    assert_eq!(positional.err, format!("kairos list 실패: ERR_PARSE_ARGS_UNEXPECTED_POSITIONAL\n{list_usage}"));
    let value = cli.run(&["next", "--surface"]);
    assert!(
        value.err.starts_with("kairos next 실패: ERR_PARSE_ARGS_INVALID_OPTION_VALUE\n사용법: kairos next "),
        "{}",
        value.err
    );
    let flag_value = cli.run(&["list", "--json=1"]);
    assert_eq!(flag_value.status, 1, "--json=1은 --json이 아니다");
    assert_eq!(flag_value.err, format!("kairos list 실패: ERR_PARSE_ARGS_INVALID_OPTION_VALUE\n{list_usage}"));
}

#[test]
fn json_감싸기가_없는_명령은_json을_줘도_표준_오류로_간다() {
    let cli = Cli::new();
    for args in [&["pause", "--bogus", "--json"][..], &["collect", "--json"]] {
        let done = cli.run(args);
        assert_eq!((done.status, done.out.as_str()), (1, ""), "{args:?}");
        assert!(done.err.contains("ERR_PARSE_ARGS_UNKNOWN_OPTION"), "{}", done.err);
    }
}

#[test]
fn 이중_대시_뒤의_json은_json_답을_켜지_않지만_오류_감싸기는_인자_전체를_본다() {
    let cli = Cli::new();
    let done = cli.run(&["later", "--", "--json"]);
    assert_eq!((done.status, done.out.as_str()), (2, "항목 ID는 양의 정수입니다\n"));
}

#[test]
fn 처리되지_않은_오류는_코드로_failed이고_json이_없으면_표준_오류다() {
    let mut cli = Cli::new();
    let file = cli.root.join("not-a-folder");
    fs::write(&file, "x").unwrap();
    cli.set("KAIROS_HOME", file.to_str().unwrap());
    let json = cli.run(&["next", "--json"]);
    let body = json.body();
    assert_eq!(json.status, 1);
    assert_eq!((&body["schema"], &body["ok"], &body["error"]["code"]), (&json!(1), &json!(false), &json!("failed")));
    assert!(!body["error"]["message"].as_str().unwrap().is_empty());
    let plain = cli.run(&["next"]);
    assert_eq!(plain.status, 1);
    assert!(plain.err.starts_with("kairos next 실패: "), "{}", plain.err);
    let doctor = cli.run(&["doctor", "--json"]);
    assert_eq!((doctor.status, doctor.out.as_str()), (1, ""));
    assert!(doctor.err.starts_with("kairos doctor 실패: "), "{}", doctor.err);
}

#[test]
fn 사용법_오류의_문구는_답_형식을_따른다() {
    let cli = Cli::new();
    let plain = cli.run(&["next", "--surface", "band"]);
    assert_eq!(
        (plain.status, plain.out.as_str()),
        (2, "사용법: kairos next [--seen] [--surface panel|orca|claude-code|vscode] [--ack] [--json]\n")
    );
    assert_eq!(cli.run(&["later"]).out, "항목 ID가 필요합니다\n");
    assert_eq!(cli.run(&["accept", "1", "2"]).out, "항목 ID는 하나만 받습니다\n");
    assert_eq!(cli.run(&["add", " "]).out, "사용법: kairos add <제목>\n");
    assert_eq!(cli.run(&["triage", "maybe"]).out, "사용법: kairos triage [keep|drop <항목 ID>...]\n");
    assert_eq!(cli.run(&["focus", "0"]).out, "사용법: kairos focus [분] | --end | --status\n");
    assert_eq!(cli.run(&["display", "maybe"]).out, "사용법: kairos display [on|off|auto]\n");
    assert_eq!(cli.run(&["sessions", "maybe"]).out, "사용법: kairos sessions [on|off]\n");
    assert_eq!(cli.run(&["notify", "maybe"]).out, "사용법: kairos notify [on|off|setup|test]\n");
}

#[test]
fn pause의_분은_js_number를_따른다() {
    let cli = Cli::new();
    assert_eq!(cli.run(&["pause", "1e3"]).out, "1000분 동안 표시를 멈춥니다. 수집은 계속합니다.\n");
    assert_eq!(cli.run(&["pause", "0x10"]).out, "16분 동안 표시를 멈춥니다. 수집은 계속합니다.\n");
    for bad in ["0", "1.5", "abc", "", "Infinity"] {
        assert_eq!(cli.run(&["pause", bad]).status, 2, "{bad}");
    }
    let huge = cli.run(&["pause", "1e30"]);
    assert_eq!((huge.status, huge.err.as_str()), (1, "kairos pause 실패: unexpected\n"));
}

#[test]
fn 세션_밖_문구와_빈_환경값() {
    let cli = Cli::new();
    assert_eq!(cli.run(&["sessions"]).out, "세션 잠김. 차단 시험 뒤 kairos sessions on\n");
    let inside = cli.run_with(&[("CLAUDECODE", "1")], &["sessions", "on"]);
    assert_eq!((inside.status, inside.out.as_str()), (1, "세션 안에서는 켤 수 없습니다\n"));
    let refused = cli.run_with(&[("CLAUDECODE", "1")], &["accept", "1"]);
    assert_eq!((refused.status, refused.out.as_str()), (1, "세션 안에서는 수락할 수 없습니다. 패널에서 누르세요\n"));
    assert_eq!(cli.run_with(&[("CLAUDECODE", "")], &["sessions", "on"]).out, "세션 열림\n");
}

#[test]
fn collect_resume은_정지_표식을_지운다() {
    let cli = Cli::new();
    assert_eq!(cli.run(&["collect", "--resume"]).out, "수집 중단 상태가 아닙니다.\n");
    fs::write(cli.home().file("halt.json"), "{\"reason\":\"model_called\"}").unwrap();
    assert_eq!(cli.run(&["collect", "--resume"]).out, "수집 중단을 해제했습니다.\n");
    assert!(!cli.home().file("halt.json").exists());
}

#[test]
fn plan_tick은_stdout_json_한_줄을_낸다() {
    let cli = Cli::new();
    let done = cli.run(&["plan-tick"]);
    let body = done.body();
    assert_eq!(done.status, 0);
    assert_eq!(body["version"], 1);
    assert!(!body["calls"].as_array().unwrap().is_empty(), "첫 틱은 모든 질의가 차례다");
    let contract = cli.run_with(&[("KAIROS_CONTRACT_TEST", "1")], &["plan-tick"]).body();
    assert_eq!(contract["calls"], body["calls"]);
}

#[test]
fn page_info는_json만_받고_쪽_값만_낸다() {
    let cli = Cli::new();
    let bad = cli.pipe(&["page-info"], b"{");
    assert_eq!((bad.status, bad.err.as_str()), (2, "kairos page-info 실패: stdin이 JSON이 아닙니다\n"));
    for input in ["null", "[]", "1", "{\"content\":[]}", "{\"content\":\"본문\"}"] {
        let done = cli.pipe(&["page-info"], input.as_bytes());
        assert_eq!((done.status, done.out.as_str()), (0, "{}\n"), "{input}");
    }
}

#[test]
fn ingest는_입력을_끝까지_읽고_판정한다() {
    let cli = Cli::new();
    let refuse = |input: &str, message: &str| {
        let done = cli.pipe(&["ingest"], input.as_bytes());
        assert_eq!((done.status, done.err.as_str()), (2, format!("kairos ingest 실패: {message}\n").as_str()));
    };
    refuse("", "stdin이 JSON이 아닙니다");
    refuse("{", "stdin이 JSON이 아닙니다");
    refuse("null", "입력 형식이 맞지 않습니다");
    refuse("{\"now\":\"x\",\"source\":\"gmail\",\"responses\":[]}", "입력 형식이 맞지 않습니다");
    refuse("{\"now\":\"2026-03-02T01:00:00.000Z\",\"source\":\"github\",\"responses\":[]}", "MCP 소스가 아닙니다");
    let done = cli.pipe(&["ingest"], b"{\"now\":\"2026-03-02T01:00:00.000Z\",\"source\":\"gmail\",\"responses\":[]}");
    assert_eq!(done.status, 0, "{}", done.err);
    assert_eq!(done.body(), json!({ "source": "gmail", "results": [] }));
}

#[test]
fn ingest의_짝_없는_서로게이트와_깨진_바이트도_json이면_받는다() {
    let cli = Cli::new();
    let done = cli.pipe(&["page-info"], b"{\"content\":\"a\\ud83d\"}");
    assert_eq!(done.status, 0);
    let broken = cli.pipe(&["page-info"], b"{\"content\":\"\xff\"}");
    assert_eq!(broken.status, 0, "깨진 바이트는 U+FFFD가 되고 JSON은 그대로다");
}

#[test]
fn 표준_출력이_닫혀도_패닉하지_않는다() {
    use std::process::{Command, Stdio};
    let cli = Cli::new();
    let mut child = Command::new(BIN)
        .arg("help")
        .env_clear()
        .envs(&cli.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty(), "{}", String::from_utf8_lossy(&output.stderr));
}
