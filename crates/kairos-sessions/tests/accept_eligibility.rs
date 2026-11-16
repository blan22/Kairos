//! 대상 판정과 외부 문자열 차단.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use common::*;
use kairos_agenda::{ItemState, Playbook};
use kairos_core::model::{EventType, NormalizedObject, TrustTier};
use kairos_core::ports::FixedClock;
use kairos_sessions::{RUN_ID, SessionLauncher, SessionStatus, launch_command, shell_quote};
use kairos_store::params;
use regex::Regex;

/// 시험 프로필(`Profile::fixture`)의 플레이북 대상.
const PLAYBOOK_REPOS: [&str; 2] = ["atlas", "atlas-specs"];
const PLAYBOOK_ORCA_REPO: &str = "0000c0de-0000-4000-8000-000000000001";

#[test]
fn 대상이_아니면_orca를_부르지_않고_url만_준다() {
    struct Case {
        name: &'static str,
        number: i64,
        url: String,
        trust: Option<TrustTier>,
        assigned: bool,
    }
    let case = |name, number, url, trust, assigned| Case { name, number, url, trust, assigned };
    let cases = [
        case("다른 저장소", 1, "https://github.com/contoso/other-repo/issues/1".into(), Some(TrustTier::T1), true),
        case("외부 조직", 2, "https://github.com/acme/widget/issues/2".into(), Some(TrustTier::T1), true),
        case("T2 출처", 3, issue_url(3), Some(TrustTier::T2), true),
        case("T3 출처", 4, issue_url(4), Some(TrustTier::T3), true),
        case("출처 없음", 5, issue_url(5), None, true),
        case(
            "다른 저장소의 내 이슈",
            6,
            "https://github.com/contoso/other-repo/issues/6".into(),
            Some(TrustTier::T1),
            false,
        ),
    ];
    for Case { name, number, url, trust, assigned } in cases {
        let db = test_db();
        let permalink = url.clone();
        seed_issue_with(&db, number, move |object| {
            object.permalink = Some(permalink);
            object.trust_tier = trust;
            object.assigned_to_me = Some(assigned);
            object.authored_by_me = Some(!assigned);
        });
        let orca = FakeOrca::new();
        let value = accept(&db, &orca);
        assert_eq!(value.result.status, SessionStatus::NotEligible, "{name}");
        assert_eq!(value.url, Some(url), "{name}");
        assert!(orca.calls().is_empty(), "{name}");
        assert_eq!(count(&db, "runs"), 0, "{name}");
        assert_eq!(item(&db, 1).state, ItemState::Open, "{name}");
    }
}

#[test]
fn specs_저장소의_이슈도_atlas_worktree로_간다() {
    let db = test_db();
    let url = issue_url_in(7, PLAYBOOK_REPOS[1]);
    let permalink = url.clone();
    seed_issue_with(&db, 7, move |object| object.permalink = Some(permalink));
    let orca = FakeOrca::new();
    accept(&db, &orca);
    assert_eq!(orca.args("worktree create")[2..4], ["--repo".to_owned(), format!("id:{PLAYBOOK_ORCA_REPO}")]);
    assert!(orca.args("worktree create").contains(&url));
}

#[test]
fn 플레이북은_항목이_열린_이유로_고르고_run에_남는다() {
    let review = |object: &mut NormalizedObject| {
        object.authored_by_me = Some(false);
        object.review_requested_me = Some(true);
    };
    let db = test_db();
    seed_pull_with(&db, 1, EventType::GhReviewRequested, review);
    let orca = FakeOrca::new();
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::Changed);
    assert_eq!(run(&db).playbook, Playbook::PrReview);
    assert!(orca.args("worktree create").contains(&pull_url(1)));

    let db = test_db();
    seed_issue_with(&db, 1, |object| {
        object.assigned_to_me = Some(false);
        object.authored_by_me = Some(true);
    });
    assert_eq!(accept(&db, &FakeOrca::new()).result.status, SessionStatus::Changed);
    assert_eq!(run(&db).playbook, Playbook::ReplyDraft);
}

#[test]
fn 담당_이슈는_글로_열려도_조사다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    assert_eq!(run(&db).playbook, Playbook::QaIssue);
}

#[test]
fn 플레이북이_없는_이유와_닫힌_객체는_대상이_아니다() {
    let db = test_db();
    seed_pull_with(&db, 1, EventType::GhAssigned, |object| object.assigned_to_me = Some(true));
    let orca = FakeOrca::new();
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::NotEligible, "PR 담당");
    assert!(orca.calls().is_empty());

    let db = test_db();
    seed_pull_with(&db, 1, EventType::GhChangesRequested, |_| {});
    assert_eq!(accept(&db, &FakeOrca::new()).result.status, SessionStatus::NotEligible, "내 PR 변경 요청");

    let db = test_db();
    seed_pull_with(&db, 1, EventType::GhComment, |_| {});
    assert_eq!(accept(&db, &FakeOrca::new()).result.status, SessionStatus::NotEligible, "내 PR의 글");

    let db = test_db();
    seed_pull_with(&db, 1, EventType::GhReviewRequested, |object| object.state = Some("closed".into()));
    assert_eq!(accept(&db, &FakeOrca::new()).result.status, SessionStatus::NotEligible, "닫힌 PR");

    let db = test_db();
    seed_pull_with(&db, 1, EventType::GhReviewRequested, |object| object.trust_tier = Some(TrustTier::T2));
    assert_eq!(accept(&db, &FakeOrca::new()).result.status, SessionStatus::NotEligible, "T2가 연 PR");
}

#[test]
fn url_위조는_어떤_값도_orca로_넘기지_않는다() {
    let url = issue_url(1);
    let forged = [
        format!("{url}; rm -rf ~"),
        format!("{url}\n--agent claude"),
        format!("{url}?x=1"),
        format!("{url}#--focus"),
        format!("{url}/../../evil/repo/issues/2"),
        "http://github.com/contoso/atlas/issues/1".to_owned(),
        "https://github.com.evil.example/contoso/atlas/issues/1".to_owned(),
        "https://github.com@evil.example/contoso/atlas/issues/1".to_owned(),
        "https://github.com/contoso/atlas/issues/1'$(touch x)'".to_owned(),
        "https://github.com/contoso/atlas/issues/abc".to_owned(),
        "--reference".to_owned(),
    ];
    for forged_url in forged {
        let (db, orca) = seeded();
        db.connection().execute("UPDATE objects SET permalink = ?", params![forged_url]).unwrap();
        let value = accept(&db, &orca);
        assert_eq!(value.result.status, SessionStatus::NotEligible, "{forged_url}");
        assert!(orca.calls().is_empty(), "{forged_url}");
        assert_eq!(count(&db, "runs"), 0, "{forged_url}");
    }
}

#[test]
fn 쉼표_따옴표_셸_문자가_든_제목은_어떤_orca_인자에도_없다() {
    let db = test_db();
    let title = "합성, \"큰따옴표\" 'x' $(touch /tmp/합성); `y` | z && w > q".to_owned();
    seed_issue_with(&db, 1, move |object| object.text_head = Some(title));
    let orca = FakeOrca::with("worktree ps", ps_with(&["done"]));
    accept(&db, &orca);
    sync(&db, &orca, 1000);
    move_to(&db, &orca, 1);
    let flat = orca.flat();
    for piece in ["합성", "큰따옴표", "$(", "`", "&&"] {
        assert!(!flat.iter().any(|argument| argument.contains(piece)), "{piece}");
    }
    let external: std::collections::BTreeSet<_> =
        flat.iter().filter(|argument| argument.starts_with("http:") || argument.starts_with("https:")).collect();
    assert_eq!(external.into_iter().collect::<Vec<_>>(), [&issue_url(1)]);
}

#[test]
fn command에는_따옴표로_묶은_상수_경로와_uuid뿐이다() {
    assert!(RUN_ID.is_match(RUN));
    let command = launch_command("/Users/synth/Library/Application Support/it's/bin/kairos-run", RUN).unwrap();
    assert_eq!(command, format!(r"'/Users/synth/Library/Application Support/it'\''s/bin/kairos-run' {RUN}"));
    assert_eq!(shell_quote("a'b"), r"'a'\''b'");
    assert!(launch_command("/s", &format!("{RUN}; rm -rf ~")).is_err());
    assert!(launch_command("/s", "not-a-uuid").is_err());
}

// 이하 원본에 없는 시험. --command는 셸이 해석하는 유일한 문자열이다.

#[test]
fn run_id_정규식은_한_줄의_소문자_uuid_v4만_받는다() {
    for bad in [
        "",
        "11111111-2222-1333-8444-555555555555",
        "11111111-2222-4333-7444-555555555555",
        "ABCDEF11-2222-4333-8444-555555555555",
        "11111111-2222-4333-8444-555555555555\n",
        "\n11111111-2222-4333-8444-555555555555",
        "11111111-2222-4333-8444-555555555555 ",
        "11111111-2222-4333-8444-55555555555５",
        "$(touch x)",
        "`touch x`",
    ] {
        assert!(!RUN_ID.is_match(bad), "{bad:?}");
        assert!(launch_command("/s", bad).is_err(), "{bad:?}");
    }
}

#[test]
fn 위조한_run_id는_터미널_명령에_닿기_전에_실패한다() {
    let (db, orca) = seeded();
    let (client, clock, ids) = (client(&orca), FixedClock(at(AT)), FixedId(format!("{RUN}; touch /tmp/x")));
    let launcher = SessionLauncher::new(&db, &client, &clock, &admit_all, RUN_BINARY).with_ids(&ids);
    let value = launcher.accept(1).unwrap();
    assert_eq!(value.result.status, SessionStatus::NotEligible);
    assert_eq!(value.result.state, Some(ItemState::Open));
    assert!(orca.calls().is_empty());
}

#[test]
fn 수락의_command는_설치본_경로와_run_id_말고_아무것도_없다() {
    let db = test_db();
    let title = "'; touch /tmp/합성; '".to_owned();
    seed_issue_with(&db, 1, move |object| object.text_head = Some(title));
    let orca = FakeOrca::new();
    accept(&db, &orca);
    let args = orca.args("terminal create");
    let command = &args[args.iter().position(|arg| arg == "--command").unwrap() + 1];
    assert_eq!(command, &format!("'{RUN_BINARY}' {RUN}"));
    let shape = Regex::new(&format!("^'[^']*' {}$", &RUN_ID.as_str()[1..RUN_ID.as_str().len() - 1])).unwrap();
    assert!(shape.is_match(command));
}

/// 경로에 어떤 글자가 있어도 셸이 경로와 run ID 둘로만 나눈다. 해석되면 cwd에 pwned가 생긴다.
#[test]
fn 실제_셸은_경로의_어떤_글자도_해석하지_않는다() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let dump = dir.path().join("argv");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$#\" \"$1\" > '{}'\n", dump.display());
    for name in [
        "plain",
        "with space",
        "it's",
        "$(touch pwned)",
        "`touch pwned`",
        "x'; touch pwned; '",
        "a\nb; touch pwned",
        "&& touch pwned",
        "한글 경로",
    ] {
        let binary = bin.join(name);
        fs::write(&binary, &script).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        let command = launch_command(binary.to_str().unwrap(), RUN).unwrap();
        let status = Command::new("/bin/sh").args(["-c", &command]).current_dir(dir.path()).status().unwrap();
        assert!(status.success(), "{name}");
        assert_eq!(fs::read_to_string(&dump).unwrap(), format!("1\n{RUN}\n"), "{name}");
        assert!(!dir.path().join("pwned").exists(), "{name}");
    }
}
