//! 세션 호스트와 호출한 호스트. 실제 git, Orca, claude는 띄우지 않는다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;

use common::*;
use kairos_sessions::RUN_ID;

fn append_user_config(cli: &Cli, text: &str) {
    let path = cli.root.join("config.toml");
    let mut content = fs::read_to_string(&path).unwrap();
    content.push_str(text);
    fs::write(&path, content).unwrap();
}

/// 설정을 바꾼 뒤 세션을 켠다. 설정 지문은 실행 파일이 읽은 설정으로 남는다.
fn open_with(cli: &Cli, config: &str) {
    append_user_config(cli, config);
    seed_issue(&cli.db(), None);
    cli.display_on();
    assert_eq!(cli.run(&["sessions", "on"]).out, "세션 열림\n");
}

#[test]
fn git_호스트는_worktree를_만들고_띄울_명령을_돌려준다() {
    let mut cli = Cli::new();
    let clone = cli.root.join("src/atlas");
    fs::create_dir_all(clone.join(".git")).unwrap();
    let worktrees = cli.dir().join("worktrees");
    let (git, log) = (cli.root.join("fake-git"), cli.root.join("git.log"));
    // worktree add의 경로 인자(-b 이름 뒤)를 만든다.
    write_script(
        &git,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\nif [ \"$5\" = worktree ]; then mkdir -p \"${{10}}\"; fi\nexit 0\n",
            log = log.display()
        ),
    );
    cli.set("KAIROS_GIT", git.to_str().unwrap());
    open_with(
        &cli,
        &format!(
            "[sessions]\nhost = \"git\"\n[hosts.git]\nworktree_root = \"{}\"\n[hosts.git.repos]\n\"contoso/atlas\" = \"{}\"\n",
            worktrees.display(),
            clone.display()
        ),
    );
    let done = cli.run(&["accept", "1", "--json"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    let body = done.body();
    let run_id = body["run"]["id"].as_str().unwrap();
    assert!(RUN_ID.is_match(run_id));
    let cwd = body["launch"]["cwd"].as_str().unwrap();
    assert_eq!(cwd, worktrees.join(format!("t-{}", &run_id[..8])).to_str().unwrap());
    assert_eq!(body["launch"]["argv"][1], run_id);
    assert!(std::path::Path::new(cwd).is_dir());
    let calls = fs::read_to_string(&log).unwrap();
    assert!(calls.contains("-c core.hooksPath=/dev/null fetch --quiet --no-tags origin fe-stage"), "{calls}");
    assert!(calls.contains(&format!("worktree add --quiet -b kairos/t-{} ", &run_id[..8])), "{calls}");
    assert!(calls.trim_end().ends_with("origin/fe-stage"), "{calls}");
    // Orca를 부르지 않는다. 추적도 Orca ps를 보지 않는다.
    let runs = cli.json(&["runs", "--json"]);
    assert_eq!(runs.body()["orca"], true);
    let moved = cli.json(&["move", "1", "--json"]);
    assert_eq!(moved.body()["results"][0]["status"], "move_failed");
}

#[test]
fn git_호스트는_클론이_없으면_항목을_되돌린다() {
    let cli = Cli::new();
    open_with(&cli, "[sessions]\nhost = \"git\"\n");
    let done = cli.run(&["accept", "1", "--json"]);
    assert_eq!(done.status, 1);
    assert_eq!(done.body()["results"][0]["status"], "repo_unresolved");
    assert_eq!(done.body()["results"][0]["state"], "open");
}

#[test]
fn claude_code에서의_수락은_켠_호스트이고_kairos_세션_밖일_때만_받는다() {
    let mut cli = Cli::new();
    cli.fake_orca();
    open_with(&cli, "[hosts]\nenabled = [\"orca\", \"claude-code\"]\n");
    let plain = cli.run_with(&[("CLAUDECODE", "1")], &["accept", "1", "--json"]);
    assert_eq!(plain.body()["error"]["code"], "session_caller");
    let in_session = cli
        .run_with(&[("CLAUDECODE", "1"), ("KAIROS_SESSION", "1")], &["accept", "1", "--from", "claude-code", "--json"]);
    assert_eq!(in_session.body()["error"]["code"], "session_caller");
    let allowed = cli.run_with(&[("CLAUDECODE", "1")], &["accept", "1", "--from", "claude-code", "--json"]);
    assert_eq!(allowed.status, 0, "{}{}", allowed.out, allowed.err);
}

#[test]
fn claude_code를_켜지_않았으면_from이_있어도_거절한다() {
    let mut cli = cli_with_issue();
    cli.fake_orca();
    let done = cli.run_with(&[("CLAUDECODE", "1")], &["accept", "1", "--from", "claude-code", "--json"]);
    assert_eq!(done.body()["error"]["code"], "session_caller");
}

/// kairos-run이 띄워 끝난 세션. session_started_at은 kairos-run만 쓴다.
const ENDED: &str = "UPDATE runs SET state = 'ended', ended_at = '2026-03-02T01:30:00.000Z', \
                     session_started_at = '2026-03-02T01:00:00.000Z'";

/// git 세션 호스트로 수락한 뒤의 worktree에서 PR을 올린다. 가짜 git은 브랜치, 커밋 하나, push를 흉내 낸다.
fn accepted_fix_session(cli: &mut Cli) -> (std::path::PathBuf, std::path::PathBuf) {
    let clone = cli.root.join("src/atlas");
    fs::create_dir_all(clone.join(".git")).unwrap();
    let worktrees = cli.dir().join("worktrees");
    let (git, log) = (cli.root.join("fake-git"), cli.root.join("git.log"));
    write_script(
        &git,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\nshift 2; while [ \"$1\" = -c ]; do shift 2; done\n\
             [ \"$1\" = --no-replace-objects ] && shift\ncase \"$1\" in\n\
             worktree) mkdir -p \"$6\" ;;\n\
             config) if [ -f '{config}' ]; then cat '{config}'; else printf 'local\\000remote.origin.url\\nhttps://github.com/contoso/atlas.git\\000'; fi ;;\n\
             rev-parse) case \"$*\" in *refs/kairos/base/*) echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ;; *) echo 0123456789abcdef0123456789abcdef01234567 ;; esac ;;\n\
             ls-remote) if [ -f '{remote}' ]; then cat '{remote}'; else printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\trefs/heads/fe-stage\\n'; fi ;;\n\
             diff) printf 'src/button.ts\\0' ;;\n\
             log) printf '버튼 정렬을 고침\\037원인은 flex 방향\\n\\036' ;;\n\
             esac\nexit 0\n",
            log = log.display(),
            config = cli.root.join("git-config").display(),
            remote = cli.root.join("git-ls-remote").display()
        ),
    );
    cli.set("KAIROS_GIT", git.to_str().unwrap());
    open_with(
        cli,
        &format!(
            "[sessions]\nhost = \"git\"\n[hosts.git]\nworktree_root = \"{}\"\n[hosts.git.repos]\n\"contoso/atlas\" = \"{}\"\n",
            worktrees.display(),
            clone.display()
        ),
    );
    let done = cli.run(&["accept", "1", "--json"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    (git, log)
}

#[test]
fn pr은_세션이_커밋한_kairos_브랜치를_훅_없이_push하고_draft_pr을_만든다() {
    let mut cli = Cli::new();
    let (_, git_log) = accepted_fix_session(&mut cli);
    let (gh, gh_log) = (cli.root.join("fake-gh"), cli.root.join("gh.log"));
    write_script(
        &gh,
        &format!(
            "#!/bin/sh\nfor arg in \"$@\"; do printf '%s\\n' \"$arg\" >> '{log}'; done\n\
             if [ \"$2\" = create ]; then echo https://github.com/contoso/atlas/pull/7; fi\nexit 0\n",
            log = gh_log.display()
        ),
    );
    cli.set("KAIROS_GH", gh.to_str().unwrap());
    let running = cli.run(&["pr", "1", "--json"]);
    assert_eq!(running.body()["error"]["code"], "session_running", "일하는 세션은 올리지 않는다");
    cli.db().connection().execute_batch(ENDED).unwrap();
    let list = cli.json(&["list", "--json"]).body();
    let actions = list["items"][0]["actions"].as_array().unwrap().iter().map(|a| a["id"].clone()).collect::<Vec<_>>();
    assert!(actions.contains(&serde_json::json!("pr")), "{actions:?}");

    let done = cli.run(&["pr", "1", "--json"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    let pr = &done.body()["pr"];
    assert_eq!(pr["status"], "created");
    assert_eq!(pr["url"], "https://github.com/contoso/atlas/pull/7");
    let branch = pr["branch"].as_str().unwrap();
    assert!(branch.starts_with("kairos/t-") && branch.len() == "kairos/t-".len() + 8, "{branch}");
    let calls = fs::read_to_string(&git_log).unwrap();
    assert!(calls.contains("-c core.hooksPath=/dev/null --no-replace-objects log --reverse --no-merges"), "{calls}");
    assert!(calls.contains("--no-replace-objects config --list --show-scope -z"), "{calls}");
    assert!(
        calls.contains("fetch --quiet --no-tags --no-write-fetch-head origin +refs/heads/fe-stage:refs/kairos/base/t-"),
        "{calls}"
    );
    assert!(!calls.contains("FETCH_HEAD"), "{calls}");
    assert!(
        calls.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa..0123456789abcdef0123456789abcdef01234567"),
        "{calls}"
    );
    assert!(
        calls.contains("diff --name-only -z --no-renames aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 0123"),
        "{calls}"
    );
    assert!(
        calls.contains(&format!(
            "-c core.hooksPath=/dev/null --no-replace-objects push --quiet origin 0123456789abcdef0123456789abcdef01234567:refs/heads/{branch}"
        )),
        "{calls}"
    );
    let argv = fs::read_to_string(&gh_log).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    let create = argv.iter().position(|arg| *arg == "create").unwrap();
    assert_eq!(
        &argv[create..create + 9],
        ["create", "--draft", "--repo", "contoso/atlas", "--base", "fe-stage", "--head", branch, "--title"]
    );
    assert_eq!(argv[create + 9], "버튼 정렬을 고침");
}

/// 가짜 gh. PR을 만들면 주소를 찍는다.
fn fake_gh(cli: &mut Cli) {
    let gh = cli.root.join("fake-gh");
    write_script(
        &gh,
        "#!/bin/sh\nif [ \"$2\" = create ]; then echo https://github.com/contoso/atlas/pull/7; fi\nexit 0\n",
    );
    cli.set("KAIROS_GH", gh.to_str().unwrap());
}

#[test]
fn pr은_이어받은_세션이나_kairos_run이_띄우지_않은_세션은_올리지_않는다() {
    let mut cli = Cli::new();
    let (_, git_log) = accepted_fix_session(&mut cli);
    fake_gh(&mut cli);
    cli.db()
        .connection()
        .execute_batch("UPDATE runs SET state = 'ended', ended_at = '2026-03-02T01:30:00.000Z'")
        .unwrap();
    let never_started = cli.run(&["pr", "1", "--json"]);
    assert_eq!(never_started.body()["error"]["code"], "adopted_session");
    cli.db().connection().execute_batch(&format!("{ENDED}, adopted = 1")).unwrap();
    let adopted = cli.run(&["pr", "1", "--json"]);
    assert_eq!(adopted.body()["error"]["code"], "adopted_session");
    let calls = fs::read_to_string(&git_log).unwrap();
    assert!(!calls.contains("--no-write-fetch-head") && !calls.contains(" push "), "{calls}");
}

#[test]
fn pr은_저장소_설정이_명령이나_원격을_바꾸면_소유자_git을_돌리지_않는다() {
    let mut cli = Cli::new();
    let (_, git_log) = accepted_fix_session(&mut cli);
    fake_gh(&mut cli);
    cli.db().connection().execute_batch(ENDED).unwrap();
    let config = cli.root.join("git-config");
    for planted in [
        "local\0remote.origin.url\nhttps://github.com/contoso/atlas.git\0local\0core.sshcommand\ntouch /tmp/x\0",
        "local\0remote.origin.url\n/tmp/planted\0",
        "global\0user.name\nx\0",
    ] {
        fs::write(&config, planted).unwrap();
        let done = cli.run(&["pr", "1", "--json"]);
        assert_eq!(done.body()["error"]["code"], "unsafe_repo_config", "{planted:?}");
    }
    let calls = fs::read_to_string(&git_log).unwrap();
    assert!(!calls.contains("rev-parse --verify HEAD") && !calls.contains(" push "), "{calls}");
}

#[test]
fn pr은_받은_기준_ref가_원격이_알려_준_커밋과_다르면_push하지_않는다() {
    // 공용 refs는 같은 저장소의 다른 세션이 fetch와 다시 읽기 사이에 바꿀 수 있다. 가짜 git의 rev-parse는 aaa…를 낸다.
    let mut cli = Cli::new();
    let (_, git_log) = accepted_fix_session(&mut cli);
    fake_gh(&mut cli);
    cli.db().connection().execute_batch(ENDED).unwrap();
    let remote = cli.root.join("git-ls-remote");
    for listed in [
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/heads/fe-stage\n",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/heads/x/refs/heads/fe-stage\n",
        "",
    ] {
        fs::write(&remote, listed).unwrap();
        let done = cli.run(&["pr", "1", "--json"]);
        assert_eq!(done.body()["error"]["code"], "base_unavailable", "{listed:?}");
    }
    let calls = fs::read_to_string(&git_log).unwrap();
    assert!(calls.contains("ls-remote --refs origin refs/heads/fe-stage"), "{calls}");
    assert!(!calls.contains(" push "), "{calls}");
}

#[test]
fn pr은_세션_안에서_부르거나_세션이_없으면_올리지_않는다() {
    let mut cli = Cli::new();
    seed_issue(&cli.db(), None);
    let none = cli.run(&["pr", "1", "--json"]);
    assert_eq!(none.status, 1);
    assert_eq!(none.body()["error"]["code"], "no_session");
    cli.set("KAIROS_SESSION", "1");
    let inside = cli.run(&["pr", "1", "--json"]);
    assert_eq!(inside.body()["error"]["code"], "session_caller");
}
