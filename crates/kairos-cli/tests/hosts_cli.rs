//! 세션 호스트와 호출한 호스트. 실제 git, Orca, claude는 띄우지 않는다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
// 세션은 macOS에서만 연다(`Os::supports_sessions`). 다른 OS의 거절은 session_e2e가 본다.
#![cfg(target_os = "macos")]
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
    let worktrees = cli.outside().join("worktrees");
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
    let worktrees = cli.outside().join("worktrees");
    let (git, log) = (cli.root.join("fake-git"), cli.root.join("git.log"));
    write_script(
        &git,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\nshift 2; while [ \"$1\" = -c ]; do shift 2; done\n\
             [ \"$1\" = --no-replace-objects ] && shift\ncase \"$1\" in --git-dir=*) shift ;; esac\ncase \"$1\" in\n\
             worktree) mkdir -p \"$6\" ;;\n\
             config) if [ -f '{config}' ]; then cat '{config}'; else printf 'local\\000remote.origin.url\\nhttps://github.com/contoso/atlas.git\\000'; fi ;;\n\
             rev-parse) case \"$*\" in *refs/kairos/base/*) echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ;; \\\n\
             *refs/kairos/head/check*) if [ -f '{moved}' ]; then echo 1111111111111111111111111111111111111111; else echo 0123456789abcdef0123456789abcdef01234567; fi ;; \\\n\
             *) echo 0123456789abcdef0123456789abcdef01234567 ;; esac ;;\n\
             ls-remote) if [ -f '{remote}' ]; then cat '{remote}'; else printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\\trefs/heads/fe-stage\\n'; fi ;;\n\
             diff) printf 'src/button.ts\\0' ;;\n\
             log) printf '버튼 정렬을 고침\\037원인은 flex 방향\\n\\036' ;;\n\
             esac\nexit 0\n",
            log = log.display(),
            config = cli.root.join("git-config").display(),
            remote = cli.root.join("git-ls-remote").display(),
            moved = cli.root.join("git-moved").display()
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
    assert!(
        calls.contains("-c core.hooksPath=/dev/null --no-replace-objects --git-dir=. log --reverse --no-merges"),
        "{calls}"
    );
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
    // 비교와 push는 데이터 폴더의 검사용 저장소에서 한다. 세션이 쓸 수 있는 공용 .git의 객체를 다시 읽지 않는다.
    let check = cli.root.join("pr-check").join(&branch["kairos/".len()..]);
    let check = check.to_str().unwrap();
    assert!(calls.contains(&format!("init --bare --quiet {check}\n")), "{calls}");
    let fetched =
        calls.lines().find(|line| line.starts_with(&format!("-C {check} ")) && line.contains(" fetch ")).unwrap();
    assert!(
        fetched.ends_with(&format!(
            "-c transfer.fsckObjects=true fetch --quiet --no-tags --no-write-fetch-head {} +refs/kairos/base/{}:refs/kairos/base/check +HEAD:refs/kairos/head/check",
            worktree_of(&calls),
            &branch["kairos/".len()..]
        )),
        "{fetched}"
    );
    for step in ["log --reverse", "diff --name-only", "push --quiet"] {
        let line = calls.lines().find(|line| line.contains(step)).unwrap();
        assert!(line.starts_with(&format!("-C {check} ")), "{line}");
    }
    assert!(
        calls.contains(&format!(
            "-c core.hooksPath=/dev/null --no-replace-objects --git-dir=. push --quiet https://github.com/contoso/atlas.git 0123456789abcdef0123456789abcdef01234567:refs/heads/{branch}"
        )),
        "{calls}"
    );
    assert!(!std::path::Path::new(check).exists(), "검사용 저장소는 끝나면 지운다");
    let argv = fs::read_to_string(&gh_log).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    let create = argv.iter().position(|arg| *arg == "create").unwrap();
    assert_eq!(
        &argv[create..create + 9],
        ["create", "--draft", "--repo", "contoso/atlas", "--base", "fe-stage", "--head", branch, "--title"]
    );
    assert_eq!(argv[create + 9], "버튼 정렬을 고침");
}

/// 확인한 뒤 HEAD가 바뀌면(세션이 남긴 프로세스) 검사용 저장소로 받은 커밋이 확인한 SHA와 달라진다. 올리지 않는다.
#[test]
fn pr은_검사용_저장소로_받은_커밋이_확인한_것과_다르면_올리지_않는다() {
    let mut cli = Cli::new();
    let (_, git_log) = accepted_fix_session(&mut cli);
    fake_gh(&mut cli);
    cli.db().connection().execute_batch(ENDED).unwrap();
    fs::write(cli.root.join("git-moved"), "").unwrap();
    let done = cli.run(&["pr", "1", "--json"]);
    assert_eq!(done.body()["error"]["code"], "push_failed", "{}", done.out);
    let calls = fs::read_to_string(&git_log).unwrap();
    assert!(!calls.contains(" push --quiet "), "{calls}");
    assert!(!calls.contains(" diff --name-only "), "{calls}");
}

/// 물음에 넣을 답. 화면의 확인 코드를 그대로 넣거나, 정한 글을 넣거나, 입력을 닫는다.
enum Answer {
    Code,
    Text(&'static str),
    Closed,
}

/// git 호스트로 수락한 고치는 세션을 kairos-run으로 띄우고, 가짜 claude가 0으로 끝난 뒤 물음에 `answer`로 답한다.
/// gh는 글 읽기(graphql)와 PR 만들기를 흉내 낸다. 돌려주는 값은 (kairos-run 출력, git 기록)이다.
fn run_fix_session(answer: Answer) -> (String, String) {
    let mut cli = Cli::new();
    let (_, git_log) = accepted_fix_session(&mut cli);
    let worktree = fs::read_dir(cli.outside().join("worktrees")).unwrap().next().unwrap().unwrap().path();
    fs::create_dir_all(worktree.join(".git")).unwrap();
    let issue = serde_json::json!({ "data": { "repository": { "issueOrPullRequest": {
        "__typename": "Issue", "id": "I_synth", "number": 1, "title": "합성 이슈", "body": "합성 본문",
        "authorAssociation": "MEMBER", "author": { "__typename": "User", "login": "synthetic-lead" },
        "comments": { "totalCount": 0, "pageInfo": { "hasNextPage": false, "endCursor": null }, "nodes": [] },
    } } } });
    let gh = cli.root.join("fake-gh");
    write_script(
        &gh,
        &format!(
            "#!/bin/sh
case \"$1 $2\" in\n'auth token') printf 'synthetic-token\\n' ;;\n'api graphql') cat > /dev/null; printf '%s\\n' '{issue}' ;;\n\
             'pr create') echo https://github.com/contoso/atlas/pull/7 ;;\n'pr list') ;;\n*) exit 1 ;;\nesac\n"
        ),
    );
    cli.set("KAIROS_GH", gh.to_str().unwrap());
    let claude = cli.root.join("fake-claude");
    write_script(&claude, "#!/bin/sh\nexit 0\n");
    let bin = cli.install_binaries();
    let run_id = cli.db().connection().query_row("SELECT id FROM runs", [], |row| row.get::<_, String>(0)).unwrap();
    let mut child = std::process::Command::new(bin.join("kairos-run"))
        .arg(&run_id)
        .current_dir(&worktree)
        .env_clear()
        .envs(&cli.env)
        .env("KAIROS_CLAUDE", &claude)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // 물음이 나올 때까지 읽는다. 확인 코드는 물음 안에 있다.
    let mut stdout = child.stdout.take().unwrap();
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    while !String::from_utf8_lossy(&seen).contains("다른 입력은 취소입니다:") {
        assert_eq!(std::io::Read::read(&mut stdout, &mut byte).unwrap(), 1, "{}", String::from_utf8_lossy(&seen));
        seen.push(byte[0]);
    }
    let prompt = String::from_utf8_lossy(&seen).into_owned();
    let code = prompt.rsplit("열려면 ").next().unwrap()[..4].to_owned();
    match answer {
        Answer::Code => {
            std::io::Write::write_all(child.stdin.as_mut().unwrap(), format!("{code}\n").as_bytes()).unwrap()
        }
        Answer::Text(text) => std::io::Write::write_all(child.stdin.as_mut().unwrap(), text.as_bytes()).unwrap(),
        Answer::Closed => {}
    }
    drop(child.stdin.take());
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut rest).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{}", String::from_utf8_lossy(&output.stderr));
    let shown = format!("{prompt}{rest}{}", String::from_utf8_lossy(&output.stderr));
    (shown, fs::read_to_string(&git_log).unwrap())
}

/// 정상으로 끝난 고치는 세션은 검사용 저장소에서 구한 커밋과 파일을 보여 주고 확인 코드를 묻는다.
/// 코드를 그대로 넣을 때만 push하고 draft PR을 연다. y는 남은 프로세스도 넣을 수 있어 받지 않는다.
#[test]
fn kairos_run은_세션이_끝나면_커밋을_보여_주고_확인_코드를_받아야_올린다() {
    let (shown, calls) = run_fix_session(Answer::Code);
    assert!(shown.contains("올릴 커밋 0123456789abcdef0123456789abcdef01234567\n  - 버튼 정렬을 고침\n"), "{shown}");
    assert!(shown.contains("바뀐 파일 1개\n  src/button.ts\n"), "{shown}");
    assert!(shown.contains("draft PR을 만들었습니다 https://github.com/contoso/atlas/pull/7"), "{shown}");
    assert!(calls.contains(" push --quiet https://github.com/contoso/atlas.git "), "{calls}");

    for answer in [Answer::Text("y\n"), Answer::Text("0000x\n"), Answer::Closed] {
        let (shown, calls) = run_fix_session(answer);
        assert!(shown.contains("올리지 않았습니다. 패널의 PR 올리기로 올릴 수 있습니다"), "{shown}");
        assert!(!calls.contains(" push --quiet "), "답하지 않거나 아니오면 올리지 않는다: {calls}");
    }
}

/// 가짜 git 기록에서 PR 올리기가 본 worktree 폴더. config를 읽은 줄의 `-C` 값이다.
fn worktree_of(calls: &str) -> String {
    let line = calls.lines().find(|line| line.contains(" config --list ")).unwrap();
    line.strip_prefix("-C ").unwrap().split(" -c ").next().unwrap().to_owned()
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
