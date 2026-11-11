//! 고치는 세션의 작업 공간. 세션이 고치지 못하는 git 경로를 실제 git이 만든 모양으로 확인한다.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use kairos_sessions::SessionWorkspace;
use regex::Regex;
use tempfile::TempDir;

/// 없는 홈. 전역 설정을 읽지 않는다. 전역 설정 시험은 임시 폴더를 홈으로 준다.
const HOME: &str = "/synthetic/home";

fn real(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

/// 값과 상관없이 막는 작업 공간 안의 경로. hook 폴더와 claude 설정이다.
const WORKSPACE_DENIED: [&str; 6] = [".husky", ".githooks", ".claude", ".mcp.json", "CLAUDE.md", "CLAUDE.local.md"];

fn workspace_denied(root: &Path) -> impl Iterator<Item = String> + '_ {
    WORKSPACE_DENIED.iter().map(move |name| text(&root.join(name)))
}

/// 하위 폴더에 새로 만드는 `.git`의 대소문자 무시 glob. `*/`로 시작해 작업 공간의 `.git` 자리는 맞지 않는다.
fn nested_git(root: &Path) -> [String; 2] {
    [format!("{}/*/**/.[gG][iI][tT]", text(root)), format!("{}/*/**/.[gG][iI][tT]/**", text(root))]
}

/// `.git` 포인터 자리의 대소문자 무시 glob. `.git`이 폴더가 아니면(연결된 worktree, git이 아닌 폴더) 붙는다.
fn pointer(root: &Path) -> [String; 2] {
    [format!("{}/.[gG][iI][tT]", text(root)), format!("{}/.[gG][iI][tT]/**", text(root))]
}

/// 하위 폴더의 claude 설정 glob.
fn nested_config(root: &Path) -> [String; 3] {
    let root = text(root);
    [
        format!("{root}/**/.[cC][lL][aA][uU][dD][eE]"),
        format!("{root}/**/.[cC][lL][aA][uU][dD][eE]/**"),
        format!("{root}/**/.[mM][cC][pP].[jJ]*[oO][nN]"),
    ]
}

/// 샌드박스 denyWrite 끝에 붙는 glob. 작업 공간 이름 여섯의 대소문자 무시 자리와 아래, 설정이 가리키는 작업 공간 안 대상(`inside`,
/// 이미 대소문자 무시로 쓴 상대 경로), `.git` 포인터, 하위 `.git`, 하위 CLAUDE.md, 하위 claude 설정이다.
fn globs_with(root: &Path, inside: &[&str]) -> Vec<String> {
    let names = [
        ".[hH][uU]*[yY]",
        ".[gG][iI][tT][hH][oO][oO]*",
        ".[cC][lL][aA][uU][dD][eE]",
        ".[mM][cC][pP].[jJ]*[oO][nN]",
        "[cC][lL][aA][uU][dD][eE].[mM][dD]",
        "[cC][lL][aA][uU][dD][eE].[lL][oO][cC][aA][lL].[mM][dD]",
    ];
    let base = text(root);
    let mut globs: Vec<String> =
        names.iter().chain(inside).flat_map(|name| [format!("{base}/{name}"), format!("{base}/{name}/**")]).collect();
    if !fs::symlink_metadata(root.join(".git")).is_ok_and(|metadata| metadata.is_dir()) {
        globs.extend(pointer(root));
    }
    globs.extend(nested_git(root));
    globs.push(format!("{base}/**/[cC][lL][aA][uU][dD][eE].[mM][dD]"));
    globs.push(format!("{base}/**/[cC][lL][aA][uU][dD][eE].[lL][oO][cC][aA][lL].[mM][dD]"));
    globs.extend(nested_config(root));
    globs
}

fn globs(root: &Path) -> Vec<String> {
    globs_with(root, &[])
}

fn is_glob(path: &str) -> bool {
    path.contains(['*', '[', '?'])
}

/// glob을 뺀 고정 경로. 설정이 가리키는 곳은 이 목록의 끝에 온다.
fn fixed_paths(workspace: &SessionWorkspace) -> Vec<String> {
    workspace.deny_write().iter().filter(|path| !is_glob(path)).cloned().collect()
}

/// 고정 경로마다 그 자리와 아래를 막는 Edit 규칙이 있고, glob마다 같은 glob의 Edit 규칙이 있다.
/// 작업 공간의 `.git` 자리는 고정 규칙이 맡는다.
fn assert_edit_parity(workspace: &SessionWorkspace) {
    assert_edit_parity_with(workspace, &globs(workspace.root()));
}

fn assert_edit_parity_with(workspace: &SessionWorkspace, expected: &[String]) {
    let edits: Vec<String> = workspace.denied_edits().collect();
    let dot_git = text(&workspace.root().join(".git"));
    for path in fixed_paths(workspace) {
        // 작업 공간의 `.git` 자리는 대소문자를 가리지 않는 고정 규칙이 맡는다.
        let path = if path == dot_git { format!("{}/.[gG][iI][tT]", text(workspace.root())) } else { path };
        for rule in [format!("Edit(/{path})"), format!("Edit(/{path}/**)")] {
            assert!(edits.contains(&rule), "{rule}");
        }
    }
    let globs: Vec<&String> = workspace.deny_write().iter().filter(|path| is_glob(path)).collect();
    assert_eq!(globs, expected.iter().collect::<Vec<_>>());
    for glob in globs {
        assert!(edits.contains(&format!("Edit(/{glob})")), "{glob}");
    }
    let unique: std::collections::BTreeSet<&String> = edits.iter().collect();
    assert_eq!(unique.len(), edits.len());
}

/// 사용자 설정과 시스템 설정을 읽지 않는 git. 훅도 없다.
fn git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "synthetic")
        .env("GIT_AUTHOR_EMAIL", "synthetic@example.invalid")
        .env("GIT_COMMITTER_NAME", "synthetic")
        .env("GIT_COMMITTER_EMAIL", "synthetic@example.invalid")
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// 본 저장소와 연결된 worktree 둘. Orca가 만드는 모양과 같다(`git worktree add`).
fn repo() -> (TempDir, PathBuf, PathBuf) {
    let dir = TempDir::new().unwrap();
    let main = dir.path().join("main");
    fs::create_dir(&main).unwrap();
    git(&main, &["init", "-q", "-b", "main"]);
    fs::write(main.join("a.txt"), "a\n").unwrap();
    git(&main, &["add", "a.txt"]);
    git(&main, &["commit", "-q", "-m", "a"]);
    let worktree = dir.path().join("t-1");
    git(&main, &["worktree", "add", "-q", "-b", "fix", worktree.to_str().unwrap()]);
    git(&main, &["worktree", "add", "-q", "-b", "other", dir.path().join("t-2").to_str().unwrap()]);
    (dir, real(&main), real(&worktree))
}

#[test]
fn 연결된_worktree는_포인터와_공용_폴더의_설정_실행_경로와_다른_worktree를_막는다() {
    let (_dir, main, worktree) = repo();
    let workspace = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap();
    let common = main.join(".git");
    let admin = common.join("worktrees/t-1");
    let expected: Vec<String> = [
        worktree.join(".git"),
        common.join("hooks"),
        common.join("config"),
        common.join("config.worktree"),
        common.join("info"),
        common.join("modules"),
        common.join("objects/info"),
        common.join("shallow"),
        common.join("commondir"),
        admin.join("commondir"),
        admin.join("gitdir"),
        admin.join("config.worktree"),
        common.join("worktrees/t-2"),
    ]
    .iter()
    .map(|path| text(path))
    .chain(workspace_denied(&worktree))
    .collect();
    let globs = globs(&worktree);
    assert_eq!(workspace.deny_write(), [expected.clone(), globs.clone()].concat());
    assert_eq!(workspace.root(), worktree);
    let edits: Vec<String> = workspace.denied_edits().collect();
    let root = text(&worktree);
    let mut rules = vec![format!("Edit(/{root}/.[gG][iI][tT])"), format!("Edit(/{root}/.[gG][iI][tT]/**)")];
    rules.extend(expected[1..].iter().flat_map(|path| [format!("Edit(/{path})"), format!("Edit(/{path}/**)")]));
    // 포인터 glob은 고정 규칙과 같아 한 번만 낸다.
    let pointer = pointer(&worktree);
    rules.extend(globs.iter().filter(|glob| !pointer.contains(glob)).map(|glob| format!("Edit(/{glob})")));
    assert_eq!(edits, rules);
    assert_edit_parity(&workspace);
}

/// 하위 폴더에 심은 `.git`은 사용자가 그 폴더에서 git을 부를 때 샌드박스 밖에서 설정을 실행한다.
/// 본 checkout과 연결된 worktree 모두 Bash와 파일 도구에서 막는다.
#[test]
fn 하위_git은_본_checkout과_worktree_모두_샌드박스와_edit에서_막는다() {
    let (_dir, main, worktree) = repo();
    for root in [&worktree, &main] {
        let workspace = SessionWorkspace::discover(root, Path::new(HOME)).unwrap();
        let edits: Vec<String> = workspace.denied_edits().collect();
        for glob in nested_git(root) {
            assert!(workspace.deny_write().contains(&glob), "{glob}");
            assert!(edits.contains(&format!("Edit(/{glob})")), "{glob}");
        }
        assert_edit_parity(&workspace);
    }
}

/// Claude Code 샌드박스가 glob을 Seatbelt 정규식으로 바꾸는 방식을 줄인 것. `**/`는 폴더 0개 이상, `**`는 아무 글자,
/// `*`는 `/` 밖의 글자, 닫힌 `[..]`는 그대로 글자 묶음이다. 시험의 glob은 이 네 모양만 쓴다.
fn glob_regex(glob: &str) -> Regex {
    let mut pattern = String::from("^");
    let mut rest = glob;
    while let Some(char) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix("**/") {
            pattern.push_str("(.*/)?");
            rest = after;
        } else if let Some(after) = rest.strip_prefix("**") {
            pattern.push_str(".*");
            rest = after;
        } else if char == '*' {
            pattern.push_str("[^/]*");
            rest = &rest[1..];
        } else if char == '[' {
            let close = rest.find(']').unwrap();
            pattern.push_str(&rest[..=close]);
            rest = &rest[close + 1..];
        } else {
            pattern.push_str(&regex::escape(&char.to_string()));
            rest = &rest[char.len_utf8()..];
        }
    }
    pattern.push('$');
    Regex::new(&pattern).unwrap()
}

/// 하위 `.git` glob은 작업 공간의 `.git` 자리를 막지 않는다. 본 checkout의 커밋은 그 아래에 쓴다.
/// `<root>/*/`로 시작하므로 `<root>/.git`과 그 아래는 다시 `.git` 폴더를 품지 않는 한 맞을 수 없다.
#[test]
fn 하위_git_glob은_작업_공간의_git_자리를_덮지_않는다() {
    let (_dir, main, worktree) = repo();
    for root in [&worktree, &main] {
        let dot_git = text(&root.join(".git"));
        for glob in nested_git(root) {
            assert_ne!(glob, dot_git);
            assert!(glob.starts_with(&format!("{}/*/", text(root))), "{glob}");
            let regex = glob_regex(&glob);
            for path in [dot_git.clone(), format!("{dot_git}/config"), format!("{dot_git}/refs/heads/main")] {
                assert!(!regex.is_match(&path), "{glob} {path}");
            }
        }
    }
    // 본 checkout의 커밋이 쓴 파일은 어느 glob에도 맞지 않는다.
    let workspace = SessionWorkspace::discover(&main, Path::new(HOME)).unwrap();
    let regexes: Vec<(String, Regex)> =
        globs(&main).into_iter().map(|glob| (glob.clone(), glob_regex(&glob))).collect();
    let before = snapshot(&main.join(".git"));
    fs::write(main.join("a.txt"), "c\n").unwrap();
    git(&main, &["commit", "-q", "-am", "c"]);
    let after = snapshot(&main.join(".git"));
    let written: Vec<&PathBuf> =
        after.iter().filter(|(path, stamp)| before.get(*path) != Some(stamp)).map(|(path, _)| path).collect();
    assert!(written.iter().any(|path| path.ends_with("refs/heads/main")), "{written:?}");
    for path in written {
        let path = text(path);
        for (glob, regex) in &regexes {
            assert!(!regex.is_match(&path), "커밋이 막는 glob에 맞는 곳에 썼다: {glob} {path}");
        }
        assert!(!workspace.deny_write().iter().any(|deny| path.starts_with(deny.as_str())), "{path}");
    }
}

/// APFS는 대소문자를 가리지 않아 `.GIT`, `claude.md`도 같은 파일을 연다. glob은 글자마다 두 경우를 모두 받는다.
#[test]
fn glob은_대소문자를_가리지_않는다() {
    let (_dir, main, _worktree) = repo();
    let workspace = SessionWorkspace::discover(&main, Path::new(HOME)).unwrap();
    let root = text(&main);
    for glob in [
        format!("{root}/[cC][lL][aA][uU][dD][eE].[mM][dD]"),
        format!("{root}/**/[cC][lL][aA][uU][dD][eE].[mM][dD]"),
        format!("{root}/*/**/.[gG][iI][tT]"),
    ] {
        assert!(workspace.deny_write().contains(&glob), "{glob}");
    }
    // 대소문자를 바꾼 이름과 하위 폴더의 이름이 어느 glob엔가 맞는다.
    let regexes: Vec<Regex> = globs(&main).iter().map(|glob| glob_regex(glob)).collect();
    for path in [
        "CLAUDE.md",
        "claude.MD",
        "CLAUDE.local.md",
        "packages/web/Claude.Local.Md",
        ".Claude/settings.json",
        ".MCP.JSON",
        ".Husky/pre-commit",
        ".GitHooks/pre-push",
        "packages/web/.Claude/settings.json",
        "packages/web/.claude",
        "a/b/.MCP.json",
        "vendor/.git",
        "vendor/.GIT/config",
        "a/b/.Git/hooks/pre-commit",
    ] {
        let full = format!("{root}/{path}");
        assert!(regexes.iter().any(|regex| regex.is_match(&full)), "{full}");
    }
    // 고칠 수 있어야 하는 파일은 어느 glob에도 맞지 않는다.
    for path in [
        "src/claude.rs",
        "docs/CLAUDE.md.bak",
        "a.txt",
        ".gitignore",
        ".github/workflows/ci.yml",
        "packages/.claude-plugin/plugin.json",
        "docs/mcp.json",
    ] {
        let full = format!("{root}/{path}");
        assert!(!regexes.iter().any(|regex| regex.is_match(&full)), "{full}");
    }
}

/// claude 설정과 hook 폴더는 샌드박스 denyWrite와 Edit 거부 양쪽에 있다.
#[test]
fn claude_설정과_hook_폴더는_bash와_파일_도구_모두_막는다() {
    let (_dir, main, worktree) = repo();
    for root in [&worktree, &main] {
        let workspace = SessionWorkspace::discover(root, Path::new(HOME)).unwrap();
        let edits: Vec<String> = workspace.denied_edits().collect();
        for path in workspace_denied(root) {
            assert!(workspace.deny_write().contains(&path), "{path}");
            assert!(edits.contains(&format!("Edit(/{path})")), "{path}");
            assert!(edits.contains(&format!("Edit(/{path}/**)")), "{path}");
        }
        for glob in globs(root) {
            assert!(workspace.deny_write().contains(&glob), "{glob}");
            assert!(edits.contains(&format!("Edit(/{glob})")), "{glob}");
        }
        assert_edit_parity(&workspace);
    }
}

/// 공용 폴더 아래 모든 파일의 (수정 시각, 크기).
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (SystemTime, u64)> {
    let mut found = BTreeMap::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                stack.push(path);
            } else {
                found.insert(path, (metadata.modified().unwrap(), metadata.len()));
            }
        }
    }
    found
}

/// 로컬 커밋이 쓰는 곳은 막는 경로와 겹치지 않는다. 겹치면 고치는 세션의 커밋이 샌드박스에서 실패한다.
#[test]
fn worktree의_커밋은_막는_경로에_쓰지_않는다() {
    let (_dir, main, worktree) = repo();
    let workspace = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap();
    let before = snapshot(&main.join(".git"));
    let pointer = fs::read_to_string(worktree.join(".git")).unwrap();
    fs::write(worktree.join("a.txt"), "b\n").unwrap();
    git(&worktree, &["commit", "-q", "-am", "b"]);
    let after = snapshot(&main.join(".git"));
    let written: Vec<&PathBuf> =
        after.iter().filter(|(path, stamp)| before.get(*path) != Some(stamp)).map(|(path, _)| path).collect();
    assert!(written.iter().any(|path| path.ends_with("refs/heads/fix")), "{written:?}");
    for path in &written {
        let denied = workspace.deny_write().iter().any(|deny| path.starts_with(deny));
        assert!(!denied, "커밋이 막는 경로에 썼다: {}", path.display());
    }
    assert_eq!(fs::read_to_string(worktree.join(".git")).unwrap(), pointer);
}

#[test]
fn 본_작업_공간은_git_폴더_안의_설정_실행_경로를_막는다() {
    let (_dir, main, _worktree) = repo();
    let workspace = SessionWorkspace::discover(&main, Path::new(HOME)).unwrap();
    let dot_git = main.join(".git");
    let expected: Vec<String> =
        ["hooks", "config", "config.worktree", "info", "modules", "objects/info", "shallow", "commondir", "worktrees"]
            .iter()
            .map(|name| text(&dot_git.join(name)))
            .chain(workspace_denied(&main))
            .chain(globs(&main))
            .collect();
    assert_eq!(workspace.deny_write(), expected);
    assert_edit_parity(&workspace);
}

#[test]
fn git이_아니거나_포인터를_읽지_못하면_git_자리를_통째로_막는다() {
    let dir = TempDir::new().unwrap();
    let root = real(dir.path());
    let workspace = SessionWorkspace::discover(&root, Path::new(HOME)).unwrap();
    let expected: Vec<String> =
        [text(&root.join(".git"))].into_iter().chain(workspace_denied(&root)).chain(globs(&root)).collect();
    assert_eq!(workspace.deny_write(), expected);
    assert_edit_parity(&workspace);
    // git이 만든 모양이 아닌 포인터는 따라가지 않는다.
    let elsewhere = root.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    fs::write(elsewhere.join("commondir"), "..").unwrap();
    fs::write(root.join(".git"), format!("gitdir: {}\n", elsewhere.display())).unwrap();
    assert_eq!(SessionWorkspace::discover(&root, Path::new(HOME)).unwrap().deny_write(), workspace.deny_write());
}

#[test]
fn 상대_경로와_glob_글자가_든_작업_공간은_받지_않는다() {
    for bad in ["relative/workspace", "/synthetic/work*space", "/synthetic/work?space", "/synthetic/[w]orkspace"] {
        let error = SessionWorkspace::discover(Path::new(bad), Path::new(HOME)).unwrap_err();
        assert_eq!(error.code(), "session_workspace_unsafe", "{bad}");
    }
}

/// husky가 저장소 설정에 두는 모양. 실제 git이 쓴 설정을 읽는다.
#[test]
fn 저장소_설정의_hooks_path가_가리키는_작업_공간_안_폴더를_막는다() {
    let (_dir, main, worktree) = repo();
    git(&main, &["config", "core.hooksPath", "tools/my hooks"]);
    let workspace = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap();
    let hooks = text(&worktree.join("tools/my hooks"));
    assert_eq!(fixed_paths(&workspace).last(), Some(&hooks));
    let edits: Vec<String> = workspace.denied_edits().collect();
    // glob 규칙 앞, 고정 경로 규칙의 끝이다. 포인터 glob 둘은 고정 규칙과 같아 Edit 규칙에서 빠진다.
    let globs = globs_with(&worktree, &[HOOKS_GLOB]);
    let end = edits.len() - (globs.len() - 2);
    assert_eq!(edits[end - 2..end], [format!("Edit(/{hooks})"), format!("Edit(/{hooks}/**)")]);
    assert_edit_parity_with(&workspace, &globs);
    let main_workspace = SessionWorkspace::discover(&main, Path::new(HOME)).unwrap();
    assert_eq!(fixed_paths(&main_workspace).last(), Some(&text(&main.join("tools/my hooks"))));
    assert_edit_parity_with(&main_workspace, &globs_with(&main, &[HOOKS_GLOB]));
}

/// `tools/my hooks`의 대소문자 무시 glob. s와 k 자리는 `*` 하나로 넓힌다.
const HOOKS_GLOB: &str = "[tT][oO][oO][lL]*/[mM][yY] [hH][oO][oO]*";

/// APFS에서 `Tools/My Hooks`도 같은 hook 폴더다. 작업 공간 안을 가리키는 hooksPath는 대소문자 변형도 Bash와 Edit 모두 막는다.
#[test]
fn 작업_공간_안의_hooks_path는_대소문자_변형도_막는다() {
    let (_dir, main, worktree) = repo();
    git(&main, &["config", "core.hooksPath", "tools/my hooks"]);
    for root in [&worktree, &main] {
        let workspace = SessionWorkspace::discover(root, Path::new(HOME)).unwrap();
        let edits: Vec<String> = workspace.denied_edits().collect();
        let base = text(root);
        for glob in [format!("{base}/{HOOKS_GLOB}"), format!("{base}/{HOOKS_GLOB}/**")] {
            assert!(workspace.deny_write().contains(&glob), "{glob}");
            assert!(edits.contains(&format!("Edit(/{glob})")), "{glob}");
        }
        let regexes: Vec<Regex> =
            workspace.deny_write().iter().filter(|path| is_glob(path)).map(|glob| glob_regex(glob)).collect();
        for path in ["Tools/My Hooks", "TOOLS/my hooks/pre-commit", "tools/MY HOOKS/pre-push"] {
            let full = format!("{base}/{path}");
            assert!(regexes.iter().any(|regex| regex.is_match(&full)), "{full}");
        }
        assert!(!regexes.iter().any(|regex| regex.is_match(&format!("{base}/tools/other/a.sh"))));
    }
}

/// 연결된 worktree의 `.git`은 포인터 파일이라 커밋이 쓰지 않는다. `.GIT` 같은 대소문자 변형도 샌드박스에서 막는다.
/// 본 checkout은 `.git` 폴더에 커밋이 쓰므로 포인터 glob을 두지 않는다.
#[test]
fn 연결된_worktree만_git_포인터의_대소문자_변형을_샌드박스에서_막는다() {
    let (_dir, main, worktree) = repo();
    let linked = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap();
    let base = text(&worktree);
    for glob in pointer(&worktree) {
        assert!(linked.deny_write().contains(&glob), "{glob}");
    }
    let regexes: Vec<Regex> =
        linked.deny_write().iter().filter(|path| is_glob(path)).map(|glob| glob_regex(glob)).collect();
    for path in [".git", ".GIT", ".Git/config"] {
        let full = format!("{base}/{path}");
        assert!(regexes.iter().any(|regex| regex.is_match(&full)), "{full}");
    }
    let checkout = SessionWorkspace::discover(&main, Path::new(HOME)).unwrap();
    for glob in pointer(&main) {
        assert!(!checkout.deny_write().contains(&glob), "{glob}");
    }
}

/// 하위 폴더의 `.claude`, `.mcp.json`은 사용자가 그 폴더에서 claude를 열면 프로젝트 설정으로 읽힌다.
#[test]
fn 하위_폴더의_claude_설정은_본_checkout과_worktree_모두_막는다() {
    let (_dir, main, worktree) = repo();
    for root in [&worktree, &main] {
        let workspace = SessionWorkspace::discover(root, Path::new(HOME)).unwrap();
        let edits: Vec<String> = workspace.denied_edits().collect();
        for glob in nested_config(root) {
            assert!(workspace.deny_write().contains(&glob), "{glob}");
            assert!(edits.contains(&format!("Edit(/{glob})")), "{glob}");
        }
    }
}

/// include된 파일이 작업 공간 안에 있으면 세션이 hooksPath를 새로 정할 수 있다. 그 파일과 그 안의 값을 막는다.
#[test]
fn include된_설정_파일과_그_안의_hooks_path를_막는다() {
    let (_dir, main, worktree) = repo();
    let included = worktree.join("repo.gitconfig");
    fs::write(&included, "[core]\n\thooksPath = \"../hooks\" ; 주석\n").unwrap();
    git(&main, &["config", "includeIf.onbranch:fix.path", included.to_str().unwrap()]);
    let workspace = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap();
    let fixed = fixed_paths(&workspace);
    let tail = &fixed[fixed.len() - 2..];
    let outside = worktree.parent().unwrap().join("hooks");
    assert_eq!(tail, [text(&included), text(&outside)]);
    // 작업 공간 안의 include 파일만 대소문자 무시 glob이 붙는다. 밖의 hook 폴더는 고정 경로로만 막는다.
    let globs = globs_with(&worktree, &["[rR][eE][pP][oO].[gG][iI][tT][cC][oO][nN][fF][iI][gG]"]);
    assert_edit_parity_with(&workspace, &globs);
}

/// 전역 설정의 상대 hooksPath는 작업 공간 기준이다. `~/`는 주어진 홈으로 푼다.
#[test]
fn 전역_설정의_hooks_path도_막는다() {
    let (dir, _main, worktree) = repo();
    let home = real(dir.path()).join("home");
    fs::create_dir_all(home.join(".config/git")).unwrap();
    fs::write(home.join(".gitconfig"), "[Core]\nHOOKSPATH = .hooks\n").unwrap();
    fs::write(home.join(".config/git/config"), "[core] hooksPath = ~/global-hooks\n").unwrap();
    let workspace = SessionWorkspace::discover(&worktree, &home).unwrap();
    let fixed = fixed_paths(&workspace);
    let tail = &fixed[fixed.len() - 2..];
    assert_eq!(tail, [text(&worktree.join(".hooks")), text(&home.join("global-hooks"))]);
    assert_edit_parity_with(&workspace, &globs_with(&worktree, &[".[hH][oO][oO]*"]));
}

/// 다 읽었다고 보일 수 없는 설정이면 고치는 세션을 띄우지 않는다.
#[test]
fn 풀_수_없는_설정이면_받지_않는다() {
    for config in [
        "[core]\n\thooksPath = ~other/hooks\n",
        "[core]\n\thooksPath = %(prefix)/hooks\n",
        "[core]\n\thooksPath = \"unterminated\n",
        "[core]\n\thooksPath = hooks/*\n",
        "[core\n",
    ] {
        let (_dir, main, worktree) = repo();
        let mut content = fs::read_to_string(main.join(".git/config")).unwrap();
        content.push_str(config);
        fs::write(main.join(".git/config"), content).unwrap();
        let error = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap_err();
        assert_eq!(error.code(), "session_workspace_unsafe", "{config}");
    }
}

/// husky처럼 hooksPath가 고정 이름(`.husky`)이면 같은 glob이 다시 나오지 않는다.
#[test]
fn 고정_이름과_같은_hooks_path는_규칙을_두_번_내지_않는다() {
    let (_dir, main, worktree) = repo();
    git(&main, &["config", "core.hooksPath", ".husky"]);
    for root in [&worktree, &main] {
        let workspace = SessionWorkspace::discover(root, Path::new(HOME)).unwrap();
        let unique: std::collections::BTreeSet<&String> = workspace.deny_write().iter().collect();
        assert_eq!(unique.len(), workspace.deny_write().len());
        let edits: Vec<String> = workspace.denied_edits().collect();
        let unique: std::collections::BTreeSet<&String> = edits.iter().collect();
        assert_eq!(unique.len(), edits.len());
        assert!(workspace.deny_write().contains(&format!("{}/.[hH][uU]*[yY]", text(root))));
    }
}

/// 읽는 세션은 커밋하지 않는다. 연결된 worktree면 포인터와 공용 `.git`을 통째로, 본 checkout이면 `.git` 폴더를 통째로 막는다.
/// 같은 클론을 쓰는 고치는 세션의 관리 폴더, ref, replace ref, grafts가 모두 그 아래다.
#[test]
fn 읽는_세션이_막는_git_자리는_공용_폴더_전체다() {
    let (_dir, main, worktree) = repo();
    let linked = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap();
    let any_git = |root: &Path| [format!("{}/.[gG][iI][tT]", text(root)), format!("{}/.[gG][iI][tT]/**", text(root))];
    let mut expected = vec![text(&worktree.join(".git")), text(&main.join(".git"))];
    expected.extend(any_git(&worktree));
    assert_eq!(linked.git_dirs(), expected);
    let checkout = SessionWorkspace::discover(&main, Path::new(HOME)).unwrap();
    let mut expected = vec![text(&main.join(".git"))];
    expected.extend(any_git(&main));
    assert_eq!(checkout.git_dirs(), expected);
    assert!(checkout.admin_globs().is_empty());
    for path in ["worktrees/t-2/commondir", "refs/heads/fix", "refs/replace/x", "info/grafts", "packed-refs"] {
        let path = text(&main.join(".git").join(path));
        assert!(linked.git_dirs().iter().any(|dir| path.starts_with(dir.as_str())), "{path}");
    }
}

/// 형제 관리 폴더 막기는 띄울 때 본 목록이다. 뒤에 생긴 run의 관리 폴더에서 git을 돌리는 파일은 glob이 막는다.
#[test]
fn 고치는_세션은_뒤에_생긴_관리_폴더의_포인터도_막는다() {
    let (dir, main, worktree) = repo();
    let workspace = SessionWorkspace::discover(&worktree, Path::new(HOME)).unwrap();
    git(&main, &["worktree", "add", "-q", "-b", "later", dir.path().join("t-3").to_str().unwrap()]);
    let later = main.join(".git/worktrees/t-3");
    assert!(!workspace.deny_write().iter().any(|deny| text(&later).starts_with(deny.as_str())));
    let globs: Vec<Regex> = workspace.admin_globs().iter().map(|glob| glob_regex(glob)).collect();
    assert_eq!(globs.len(), 3);
    for name in ["commondir", "gitdir", "config.worktree", "COMMONDIR"] {
        let path = text(&later.join(name));
        assert!(globs.iter().any(|glob| glob.is_match(&path)), "{path}");
    }
    for name in ["HEAD", "index", "logs/HEAD"] {
        let path = text(&later.join(name));
        assert!(!globs.iter().any(|glob| glob.is_match(&path)), "{path}");
    }
    // APFS는 `WORKTREES`도 같은 폴더로 연다.
    let upper = text(&main.join(".git/WORKTREES/t-3/gitdir"));
    assert!(globs.iter().any(|glob| glob.is_match(&upper)), "{upper}");
}
