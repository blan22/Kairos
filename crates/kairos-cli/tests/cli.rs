#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::SourceResult;
use kairos_core::model::{Outcome, SourceName};

/// 원본의 seeded. 기준선 이슈 하나와 그 뒤의 댓글 하나가 있다.
fn seeded() -> Cli {
    let cli = Cli::new();
    let db = cli.db();
    let moment = now();
    let first = moment.to_iso();
    push_objects(&db, SourceName::Github, &first, vec![issue(1)]);
    let after = moment.plus_ms(1000).to_iso();
    let mut event = comment("1", &after);
    event.text_head = Some("\u{1b}[31m합성 댓글\u{1b}[0m".into());
    push_events(&db, SourceName::Github, &after, vec![event]);
    drop(db);
    cli
}

#[test]
fn 명령_없음과_모르는_명령은_2로_끝난다() {
    let cli = Cli::new();
    assert_eq!(cli.run(&[]).status, 2);
    let unknown = cli.run(&["send-it"]);
    assert_eq!(unknown.status, 2);
    assert!(unknown.out.contains("알 수 없는 명령"));
}

#[test]
fn 데이터_폴더가_git_작업_트리_안이면_help만_돈다() {
    let cli = Cli::new();
    let repo = cli.root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let user = cli.root.join("user");
    std::fs::create_dir_all(&user).unwrap();
    let data = repo.join("data");
    let env = [("KAIROS_HOME", data.to_str().unwrap()), ("HOME", user.to_str().unwrap())];
    for args in [&["list", "--json"][..], &["config", "init", "--json"], &["doctor"]] {
        let refused = cli.run_with(&env, args);
        assert_eq!(refused.status, 1, "{args:?}");
        if args.contains(&"--json") {
            assert_eq!(refused.body()["error"]["code"], "home_in_git_worktree");
        } else {
            assert!(refused.err.contains("git 작업 트리"), "{}", refused.err);
        }
    }
    assert!(!data.exists(), "거절하기 전에 데이터 폴더를 만들면 안 된다");
    assert_eq!(cli.run_with(&env, &["help"]).status, 0);
}

#[test]
fn help는_명령을_나열한다() {
    let help = Cli::new().run(&["help"]);
    assert_eq!(help.status, 0);
    for name in ["list", "doctor", "pause", "off"] {
        assert!(help.out.contains(name), "{name}");
    }
}

#[test]
fn doctor는_수집_전의_notion을_기록_없음으로_드러낸다() {
    let doctor = Cli::new().run(&["doctor"]);
    assert!(doctor.out.contains("Notion mail: 수집 기록 없음"), "{}", doctor.out);
    assert!(doctor.out.contains("권한 700"));
    assert!(doctor.out.contains("스키마 v8, wal, 권한 600"));
    assert!(doctor.out.contains("근거 없는 종료 0건"));
    assert_eq!(doctor.err, "");
}

#[test]
fn collect_reread_notion은_다시_읽을_건수를_알린다() {
    let reread = Cli::new().run(&["collect", "--reread-notion"]);
    assert_eq!(reread.status, 0);
    assert!(reread.out.contains("Notion 메일 0건을 다시 읽습니다"));
}

#[test]
fn list_첫_줄은_건강_상태이고_수집_기록_없음을_숨기지_않는다() {
    let list = Cli::new().run(&["list"]);
    assert_eq!(list.status, 0);
    assert!(list.out.lines().next().unwrap().contains("Notion 수집 기록 없음"), "{}", list.out);
    assert!(list.out.contains("저장된 항목 없음"));
}

#[test]
fn list는_기준선_건수를_보여_준다() {
    assert!(seeded().run(&["list"]).out.contains("issue 1건 (기준선 1건)"));
}

#[test]
fn list_would_show는_표시됐을_항목을_정제해서_보여_준다() {
    let list = seeded().run(&["list", "--would-show"]);
    assert_eq!(list.status, 0);
    assert!(list.out.contains("표시됐을 항목 1건"), "{}", list.out);
    let line = list.out.lines().find(|line| line.starts_with("P2 GitHub p2.owned_comment ")).unwrap();
    assert!(line.ends_with(" 합성 댓글"), "{line}");
    assert!(!list.out.contains('\u{1b}'));
}

#[test]
fn 첫_실행의_would_show는_0건이다() {
    let cli = Cli::new();
    let db = cli.db();
    let mut event = comment("old", T0);
    event.mentions_me = Some(true);
    event.mention_count = Some(1);
    let run = SourceResult { objects: vec![issue(1)], events: vec![event], ..run_result(SourceName::Github, T0) };
    ingest(&db, run, T0);
    drop(db);
    assert!(cli.run(&["list", "--would-show"]).out.contains("표시됐을 항목 0건"));
}

#[test]
fn pause는_표시_상태만_바꾸고_state_json에_반영한다() {
    let cli = Cli::new();
    assert_eq!(cli.run(&["pause"]).status, 0);
    assert_eq!(cli.control().paused_until(), Some("indefinite"));
    assert_eq!(cli.state()["paused"], true);

    assert_eq!(cli.run(&["pause", "--resume"]).status, 0);
    assert_eq!(cli.control().paused_until(), None);
    assert_eq!(cli.state()["paused"], false);

    assert_eq!(cli.run(&["pause", "30"]).status, 0);
    assert_eq!(cli.state()["paused"], true);
    assert_eq!(cli.run(&["pause", "abc"]).status, 2);
}

#[test]
fn off는_쉬는_날을_기록하고_해제한다() {
    let cli = Cli::new();
    assert_eq!(cli.run(&["off", "2026-03-02"]).status, 0);
    assert_eq!(cli.run(&["off", "2026-03-02"]).status, 0);
    assert_eq!(cli.control().off_dates(), ["2026-03-02"]);
    assert_eq!(cli.run(&["off", "2026-03-02", "--clear"]).status, 0);
    assert!(cli.control().off_dates().is_empty());
    assert_eq!(cli.run(&["off", "tomorrow"]).status, 2);
}

#[test]
fn off_기본값은_오늘이다() {
    let cli = Cli::new();
    cli.run(&["off"]);
    assert_eq!(cli.control().off_dates().len(), 1);
}

#[test]
fn 잘못된_옵션은_실패_코드로_끝나고_입력을_그대로_되풀이하지_않는다() {
    let failed = Cli::new().run(&["list", "--unknown\u{1b}[31m"]);
    assert_eq!(failed.status, 1);
    assert!(!failed.err.contains('\u{1b}'));
}

#[test]
fn collect_skip_slack_until은_지금_이후와_형식_오류를_거절한다() {
    let cli = Cli::new();
    assert_eq!(cli.run(&["collect", "--skip-slack-until", "not-a-time"]).status, 2);
    assert_eq!(cli.run(&["collect", "--skip-slack-until", "4000000000"]).status, 2);
    let past = cli.run(&["collect", "--skip-slack-until", "1772413200.000100"]);
    assert_eq!(past.status, 0);
    assert!(past.out.contains("옮긴 커서가 없습니다"));
}

#[test]
fn list_첫_줄에_덮지_못한_구간이_나온다() {
    let cli = Cli::new();
    let db = cli.db();
    let moment = now().to_iso();
    let run = SourceResult {
        query: "mentions".into(),
        outcome: Outcome::Ok,
        error_code: Some("record_quarantined".into()),
        window_from: Some(moment.clone()),
        window_to: Some(moment.clone()),
        ..run_result(SourceName::Slack, &moment)
    };
    ingest(&db, run, &moment);
    drop(db);
    let list = cli.run(&["list"]);
    let first = list.out.lines().next().unwrap();
    assert!(first.contains("Slack mentions: ") && first.contains(" 메시지 격리"), "{first}");
}
