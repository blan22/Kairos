//! 세션 경계 시험(R21~R25). 거부 목록과 환경을 본다.
//! 번호는 .codex/runs/2026-11-04-stage2.md의 표와 같다. 실제 orca, claude를 부르지 않는다.
#![allow(clippy::unwrap_used)]

#[path = "../../kairos-sessions/tests/common/mod.rs"]
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use common::*;
use kairos_agenda::Playbook;
use kairos_core::model::TrustTier;
use kairos_ops::{Installer, UninstallOptions};
use kairos_sessions::{FIX_TOOLS, OrcaLiveAgent, SESSION_TOOLS, SessionProfile, SessionStatus, SessionWorkspace};
use serde_json::{Value, json};

const RUN_A: &str = RUN;

#[test]
fn r21_uninstall_문구는_사실과_맞고_플러그인을_내리는_방법을_알린다() {
    let (dir, _home) = temp_home();
    let options =
        UninstallOptions { launch_agents_dir: Some(dir.path().join("agents")), dry_run: true, skip_launchctl: false };
    // dry-run은 launchctl을 부르지 않는다. 불리면 시험이 깨지도록 orca 대역을 넘긴다.
    let text = Installer::new(FakeOrca::new()).uninstall(&options).unwrap().lines.join("\n");
    assert!(!text.contains("밴드 플러그인"));
    assert!(text.contains("orca plugin dev unload"));
}

fn agent_match(linked: Option<bool>) -> serde_json::Map<String, Value> {
    let mut agent = json!({ "liveness": "live", "paneKey": "tab-x:leaf-x", "terminal": "term_x" });
    if let Some(linked) = linked {
        agent["linked"] = json!(linked);
    }
    let body = json!({ "matches": [{ "workspace": { "id": "repo::/x" }, "agents": [agent] }] });
    body.as_object().unwrap().clone()
}

#[test]
fn r22_reference_find는_연결_증거가_있는_에이전트만_채택한다() {
    assert_eq!(OrcaLiveAgent::find(&agent_match(Some(false))), None);
    assert_eq!(OrcaLiveAgent::find(&agent_match(None)), None);
    assert_eq!(OrcaLiveAgent::find(&agent_match(Some(true))).unwrap().pane_key, "tab-x:leaf-x");
}

#[test]
fn r22_연결_없는_에이전트가_있어도_새_세션을_연다() {
    let db = test_db();
    seed_issue(&db);
    let agents = json!([{ "liveness": "live", "paneKey": "tab-x:leaf-x", "linked": false }]);
    let orca = FakeOrca::with(
        "reference find",
        ok(json!({ "matches": [{ "workspace": { "id": "repo::/x" }, "agents": agents }] })),
    );
    accept(&db, &orca);
    assert_eq!(orca.count("terminal create"), 1);
    assert!(!run(&db).adopted);
}

#[test]
fn r23_not_eligible_문구가_패널에_없는_링크를_가리키지_않는다() {
    let db = test_db();
    seed_issue_with(&db, 1, |object| object.trust_tier = Some(TrustTier::T3));
    let value = accept(&db, &FakeOrca::new());
    assert_eq!(value.result.status, SessionStatus::NotEligible);
    assert!(!value.result.message.contains("링크로"));
}

/// 고치는 세션(qa_issue)은 작업 공간이 있어야 뜬다. 합성 경로라 `.git`이 없고 그 자리를 통째로 막는다.
fn workspace() -> SessionWorkspace {
    SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap()
}

fn profile() -> SessionProfile {
    profile_at(Path::new("/synthetic/kairos-home"))
}

/// --settings 파일을 데이터 폴더 아래에 쓰므로 시험의 데이터 폴더로 만든다.
fn profile_at(kairos_home: &Path) -> SessionProfile {
    SessionProfile::new(Path::new("/synthetic/kairos"), kairos_home, Path::new("/synthetic/home"))
        .with_workspace(workspace())
}

/// 측정한 claude의 동작을 흉내 내는 대역. 받은 환경과 인자, --settings 파일의 내용을 적는다. 합치기는 시험이 한다.
fn write_fake_claude(root: &Path, dump: &Path) -> String {
    let claude = root.join("fake-claude");
    let script = format!(
        "#!/bin/sh\nenv > '{env}'\nfor arg in \"$@\"; do printf '%s\\n\\036\\n' \"$arg\"; done > '{args}'\n\
         prev=\nfor arg in \"$@\"; do if [ \"$prev\" = --settings ]; then cat \"$arg\" > '{settings}'; fi; prev=$arg; done\n",
        env = dump.join("env").display(),
        args = dump.join("args").display(),
        settings = dump.join("settings").display(),
    );
    fs::write(&claude, script).unwrap();
    fs::set_permissions(&claude, fs::Permissions::from_mode(0o700)).unwrap();
    claude.to_str().unwrap().to_owned()
}

/// 시작 환경 위에 settings.json의 env가 오고 그 위에 --settings가 온다. 측정한 claude의 순서다.
/// --settings의 값은 파일 경로이고, 대역 claude가 띄워졌을 때 그 파일에서 읽은 내용을 쓴다.
fn merged_env(home: &Path, dump: &Path) -> BTreeMap<String, String> {
    let text = fs::read_to_string(dump.join("env")).unwrap();
    let mut env: BTreeMap<String, String> = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    let user: Value = serde_json::from_str(&fs::read_to_string(home.join(".claude/settings.json")).unwrap()).unwrap();
    let args = fs::read_to_string(dump.join("args")).unwrap();
    let args: Vec<&str> = args.split("\n\u{1e}\n").collect();
    let flag = args.iter().position(|arg| *arg == "--settings").unwrap();
    assert!(serde_json::from_str::<Value>(args[flag + 1]).is_err(), "--settings 인자는 JSON이 아니라 경로다");
    let given: Value = serde_json::from_str(&fs::read_to_string(dump.join("settings")).unwrap()).unwrap();
    for layer in [&user["env"], &given["env"]] {
        for (key, value) in layer.as_object().unwrap() {
            env.insert(key.clone(), value.as_str().unwrap().to_owned());
        }
    }
    env
}

/// 시작 환경에 토큰들을 두고 `user_env`를 ~/.claude/settings.json의 env로 둔 채 세션 명령을 띄워 합친 환경을 얻는다.
/// --settings 파일은 kairos-run의 spawner처럼 띄우기 직전에 쓰고 끝나면 지운다.
fn session_env(user_env: &Value) -> BTreeMap<String, String> {
    let (dir, _home) = temp_home();
    let root = dir.path();
    fs::create_dir(root.join(".claude")).unwrap();
    fs::write(root.join(".claude/settings.json"), json!({ "env": user_env }).to_string()).unwrap();
    let claude = write_fake_claude(root, root);
    let kairos_home = root.join("kairos-home");
    fs::create_dir(&kairos_home).unwrap();
    let command =
        profile_at(&kairos_home).command(&claude, RUN_A, Playbook::QaIssue, &issue_url(1), session_token()).unwrap();
    assert!(!command.args.iter().any(|arg| arg.contains(SYNTHETIC_TOKEN)), "인자는 다른 사용자도 ps로 읽는다");
    let written = command.write_settings().unwrap();
    let status = Command::new(&command.file)
        .args(&command.args)
        .env_clear()
        .envs([
            ("PATH", "/usr/bin:/bin"),
            ("HOME", root.to_str().unwrap()),
            ("GITHUB_PERSONAL_ACCESS_TOKEN", "synthetic-start-token"),
            ("GH_TOKEN", "synthetic-gh"),
            ("GITHUB_TOKEN", "synthetic-github"),
            ("FIGMA_API_TOKEN", "synthetic-figma"),
            ("ANTHROPIC_API_KEY", "synthetic-anthropic"),
            ("ORCA_AGENT_HOOK_TOKEN", "synthetic-hook"),
        ])
        .envs(command.child_env())
        .status()
        .unwrap();
    assert!(status.success());
    drop(written);
    assert!(!command.settings_path().unwrap().exists());
    merged_env(root, root)
}

#[test]
fn r24_settings_json의_env로_되살아나는_토큰이_세션_환경에서_비어_있다() {
    let env = session_env(&json!({ "GITHUB_PERSONAL_ACCESS_TOKEN": "synthetic-settings-token" }));
    for key in ["GITHUB_PERSONAL_ACCESS_TOKEN", "GITHUB_TOKEN", "FIGMA_API_TOKEN", "ANTHROPIC_API_KEY"] {
        assert_eq!(env.get(key).map_or("", String::as_str), "", "{key}");
    }
    // 시작 환경의 GH_TOKEN은 소유자가 둔 읽기 전용 토큰으로 바뀐다.
    assert_eq!(env.get("GH_TOKEN").map(String::as_str), Some(SYNTHETIC_TOKEN));
    let hook = env.get("ORCA_AGENT_HOOK_TOKEN").map(String::as_str);
    assert_eq!(hook, Some("synthetic-hook"), "Orca 상태 보고 훅이 쓰는 값은 지우지 않는다");
}

/// 사용자 settings.json의 env에 소유자의 넓은 권한 토큰이 있어도 세션은 읽기 전용 토큰으로 끝난다.
/// 프로세스 환경만으로는 막지 못한다. settings.json의 env가 시작 뒤에 그 위로 다시 들어온다.
#[test]
fn r24_settings_json의_env에_있는_gh_token도_세션에서는_읽기_전용_토큰이다() {
    let owner = "github_pat_SYNTHETICOWNERFULLSCOPE0123456789";
    let env = session_env(&json!({ "GH_TOKEN": owner, "GITHUB_TOKEN": owner }));
    assert_eq!(env.get("GH_TOKEN").map(String::as_str), Some(SYNTHETIC_TOKEN));
    assert_eq!(env.get("GITHUB_TOKEN").map_or("", String::as_str), "");
    assert!(!env.values().any(|value| value == owner));
}

/// 고치는 세션은 `--setting-sources user`로 프로젝트, 로컬 설정의 훅을 빼고 사용자 설정은 남긴다. Orca 상태 보고 훅이 거기 있다.
/// 읽는 세션은 `--setting-sources`를 넘기지 않는다.
#[test]
fn r24_고치는_세션만_사용자_설정만_읽고_읽는_세션은_설정_출처를_바꾸지_않는다() {
    let fix = profile().command("/c", RUN_A, Playbook::QaIssue, &issue_url(1), session_token()).unwrap();
    let index = fix.args.iter().position(|arg| arg == "--setting-sources").unwrap();
    assert_eq!(fix.args[index + 1], "user");
    assert_eq!(fix.args.iter().filter(|arg| *arg == "--setting-sources").count(), 1);
    let read = profile().command("/c", RUN_A, Playbook::PrReview, &issue_url(1), session_token()).unwrap();
    assert!(!read.args.iter().any(|arg| arg == "--setting-sources"));
}

/// claude의 판정이 아니라 접두어 비교다. 실제 판정은 S4로만 확인한다. 읽는 세션(pr_review)의 목록이다.
fn denied(profile: &SessionProfile, command: &str) -> bool {
    denied_in(profile, Playbook::PrReview, command)
}

fn denied_in(profile: &SessionProfile, playbook: Playbook, command: &str) -> bool {
    profile.denied_tools(playbook).iter().any(|rule| {
        let prefix = rule.strip_prefix("Bash(").and_then(|rest| rest.strip_suffix(":*)"));
        prefix.is_some_and(|prefix| command == prefix || command.starts_with(&format!("{prefix} ")))
    })
}

#[test]
fn r25_github_쓰기와_플래그_삽입_명령이_거부_목록에_걸린다() {
    let writes = [
        "gh issue create",
        "gh issue delete 1",
        "gh issue reopen 1",
        "gh issue transfer 1 x",
        "gh issue lock 1",
        "gh issue pin 1",
        "gh issue develop 1",
        "gh issue comment 1",
        "gh issue edit 1",
        "gh issue close 1",
        "gh pr create",
        "gh pr edit 1",
        "gh pr close 1",
        "gh pr reopen 1",
        "gh pr ready 1",
        "gh pr update-branch 1",
        "gh pr lock 1",
        "gh pr merge 1",
        "gh pr comment 1",
        "gh pr review 1",
        "gh gist create x",
        "gh release create v1",
        "gh workflow run x",
        "gh run rerun 1",
        "gh label create x",
        "gh repo edit",
        "gh secret set X",
        "gh variable set X",
        "gh project item-add 1",
        "gh ssh-key add x",
        "gh alias set a b",
        "gh auth setup-git",
        "gh auth token",
        "gh api x",
        "gh pr --repo o/r comment 1 -b x",
        "gh issue --repo o/r comment 1 -b x",
        "gh pr -R o/r comment 1",
        "gh issue -R o/r comment 1",
        "gh --repo o/r api x",
        "gh -R o/r api x",
        "git push",
        "git -C . push",
        "git -c a=b push",
        "git commit -m x",
        "orca terminal send",
        "claude -p x",
        "curl https://api.synthetic.example",
        "wget x",
        "sh -c x",
        "bash -c x",
        "zsh -c x",
        "env -u X y",
        "kairos accept 1",
        "kairos-run x",
        // 감사에서 거부 목록을 돌아간 형태. 강제 경계는 샌드박스이고 이 목록은 겹이다.
        "git --no-pager push",
        "git -P commit -m x",
        "git credential fill",
        "git config credential.helper x",
        "/usr/bin/curl -sS x",
        "/usr/bin/env -u X y",
        "perl -e x",
        "nc x 1",
        "sqlite3 x",
        "/Applications/Orca.app/Contents/Resources/bin/orca terminal send",
        "/synthetic/home/.local/bin/kairos sessions on",
    ];
    let profile = profile();
    for command in writes {
        assert!(denied(&profile, command), "{command}");
        // 고치는 세션은 로컬 커밋만 더 열린다.
        if !command.starts_with("git commit") {
            assert!(denied_in(&profile, Playbook::QaIssue, command), "qa {command}");
        }
    }
    assert!(!denied_in(&profile, Playbook::QaIssue, "git commit -m x"));
    assert!(!denied_in(&profile, Playbook::QaIssue, "/usr/bin/git commit -m x"));
    for command in ["git commit-tree x", "git update-ref refs/heads/x HEAD", "git push", "git remote add x y"] {
        assert!(denied_in(&profile, Playbook::QaIssue, command), "qa {command}");
    }
}

#[test]
fn r25_대조_조사에_쓰는_읽기는_막지_않는다() {
    let reads = [
        "gh issue view 1",
        "gh issue list",
        "gh pr view 1",
        "gh pr diff 1",
        "gh pr checks 1",
        "gh pr list",
        "gh search issues x",
        "git log",
        "git status",
        "git diff",
        "git show HEAD",
        "git blame x",
        "ls",
        "rg x",
    ];
    let profile = profile();
    for command in reads {
        assert!(!denied(&profile, command), "{command}");
    }
}

#[test]
fn r25_수정과_외부_도구는_읽는_세션에_없고_거부_목록에도_있다() {
    let mut tools = SESSION_TOOLS.to_vec();
    tools.sort_unstable();
    assert_eq!(tools, ["Bash", "Glob", "Grep", "Read"]);
    let profile = profile();
    let command = profile.command("/c", RUN_A, Playbook::PrReview, &issue_url(1), session_token()).unwrap();
    let index = command.args.iter().position(|arg| arg == "--tools").unwrap();
    assert_eq!(command.args[index + 1], SESSION_TOOLS.join(","));
    let denied = profile.denied_tools(Playbook::PrReview);
    for tool in [
        "Edit",
        "Write",
        "NotebookEdit",
        "WebFetch",
        "WebSearch",
        "Agent",
        "ArtifactData",
        "ArtifactComments",
        "CronDelete",
        "PushNotification",
        "DesignSync",
        "Read(~/.claude/settings.json)",
    ] {
        assert!(denied.iter().any(|rule| rule == tool), "{tool}");
    }
    let unique: BTreeSet<&String> = denied.iter().collect();
    assert_eq!(unique.len(), denied.len());
}

#[test]
fn r25_설치본_경로로_부른_kairos도_거부_목록에_있다() {
    let profile = profile();
    let bin = "/synthetic/kairos/bin/kairos";
    assert!(denied(&profile, &format!("{bin} accept 1")));
    assert!(denied(&profile, &format!("\"{bin}\" accept 1")));
    assert!(denied(&profile, &format!("'{bin}' accept 1")));
}

#[test]
fn 프롬프트의_외부_값은_여전히_url뿐이다() {
    assert!(SessionProfile::prompt(Playbook::QaIssue, &issue_url(1)).unwrap().contains(&issue_url(1)));
    assert_eq!(pane_row()["handle"], "term_now");
}

#[test]
fn r25_고치는_세션은_edit_write만_더_열리고_웹과_외부_도구는_그대로_막힌다() {
    let profile = profile();
    let command = profile.command("/c", RUN_A, Playbook::QaIssue, &issue_url(1), session_token()).unwrap();
    let index = command.args.iter().position(|arg| arg == "--tools").unwrap();
    assert_eq!(command.args[index + 1], FIX_TOOLS.join(","));
    let denied = profile.denied_tools(Playbook::QaIssue);
    for tool in ["Edit", "Write"] {
        assert!(!denied.iter().any(|rule| rule == tool), "{tool}");
    }
    for tool in
        ["NotebookEdit", "WebFetch", "WebSearch", "Agent", "Artifact", "Edit(//synthetic/workspace/.[gG][iI][tT]/**)"]
    {
        assert!(denied.iter().any(|rule| rule == tool), "{tool}");
    }
    let unique: BTreeSet<&String> = denied.iter().collect();
    assert_eq!(unique.len(), denied.len());
}
