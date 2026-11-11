//! 고치는 세션(qa_issue)의 작업 공간과 그 git 폴더. 세션이 고칠 수 없는 git 경로를 띄우기 전에 정한다.
//!
//! 세션 Bash의 샌드박스는 작업 공간과 임시 폴더에 쓰고, 작업 공간이 연결된 worktree이면 본 저장소의 공용 .git에도 쓴다.
//! 그래야 `git commit`이 objects, refs, logs, worktree 관리 폴더의 index와 HEAD를 고친다. 공용 .git의 hooks와 config는 claude가 막는다
//! (https://code.claude.com/docs/en/sandboxing, Git worktrees). 그래서 allowWrite는 두지 않는다.
//! 공용 .git에는 그 둘 말고도 사용자가 나중에 샌드박스 밖에서 돌리는 git이 읽어 실행으로 이어지는 곳이 있다. 여기서 이름으로 한 번 더 막는다.
//! 작업 공간의 `.git` 파일은 관리 폴더를 가리키는 포인터라 고치면 git이 다른 설정을 읽는다. 이것도 막는다.
//! `core.hooksPath`가 작업 공간 안(husky의 `.husky/_` 등)을 가리키면 그 hook 폴더는 기본 쓰기 영역 안이다.
//! 그래서 git 설정 파일을 include까지 읽어 그 값과 include된 파일을 막고, `.husky`, `.githooks`는 값과 상관없이 막는다.
//! claude 설정(`.claude`, `.mcp.json`, `CLAUDE.md`, `CLAUDE.local.md`), 어느 폴더의 `CLAUDE.md`, `.claude`, `.mcp.json`, 하위 폴더에 새로 만드는 `.git`,
//! 설정이 가리키는 작업 공간 안 대상(hook 폴더, include 파일), 연결된 worktree의 `.git` 포인터도 glob으로 막는다. 하위 `.git` glob은 본 checkout과 연결된 worktree 모두에 붙고, `*/`로 시작해 작업 공간의 `.git` 자리는 맞지 않는다.
//! APFS는 기본으로 대소문자를 가리지 않으므로 glob은 글자마다 두 경우를 받는다(`any_case`). s와 k는 유니코드 접기(ſ, 켈빈 기호) 때문에 `*`로 넓힌다. claude 샌드박스는 glob을 Seatbelt 정규식으로
//! 바꿀 때 닫힌 `[..]`를 글자 묶음 그대로 넘긴다.
//! `.git` 이름 없이 git 폴더 모양만 둔 bare 저장소는 이름으로 막지 못해 남는다. 사용자가 `safe.bareRepository=explicit`를 두면 git이 열지 않는다.
//! git을 부르지 않고 파일만 읽는다. 세션이 뜨기 전이라 작업 공간에는 Orca와 git이 쓴 것만 있다.

use std::collections::VecDeque;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};

use kairos_core::{KairosError, KairosResult};

/// 공용 .git에서 git이 설정이나 실행 파일로 읽는 곳. objects/info는 alternates로 다른 객체 저장소를 붙이는 곳이다.
/// shallow는 얕은 클론의 경계 커밋 목록이라 log와 diff가 보는 역사를 바꾼다. commondir은 git이 어느 .git에서든 읽어
/// 공용 폴더를 다른 곳(심은 설정과 hook)으로 돌린다. 사용자가 본 checkout에서 부르는 git도 따른다. 커밋은 이 중 어디에도 쓰지 않는다.
const COMMON_DENIED: [&str; 8] =
    ["hooks", "config", "config.worktree", "info", "modules", "objects/info", "shallow", "commondir"];
/// worktree 관리 폴더에서 공용 폴더, 작업 공간, worktree 설정을 가리키는 파일. 커밋은 쓰지 않는다.
const ADMIN_DENIED: [&str; 3] = ["commondir", "gitdir", "config.worktree"];
/// `.git`이 폴더인 본 작업 공간. 위 두 목록과 다른 worktree의 관리 폴더를 함께 막는다.
const MAIN_DENIED: [&str; 1] = ["worktrees"];
/// 작업 공간 안에서 Bash와 Edit, Write 모두 고치지 못하는 경로. 값과 상관없이 막는다.
/// hook 폴더는 사용자가 나중에 샌드박스 밖에서 커밋할 때 돈다. claude 설정, MCP 설정, CLAUDE.md는
/// 이 세션과 사용자가 나중에 여는 일반 권한 세션이 읽어 지시나 실행으로 이어진다.
const WORKSPACE_DENIED: [&str; 6] = [".husky", ".githooks", ".claude", ".mcp.json", "CLAUDE.md", "CLAUDE.local.md"];
/// Edit, Write가 고치지 못하는 작업 공간의 `.git` 자리. 본 checkout이면 폴더라 샌드박스는 막지 않는다(커밋이 쓴다).
const EDIT_DENIED: [&str; 2] = [".git", ".git/**"];
/// 하위 폴더에 새로 만드는 `.git`. 심어 둔 `.git`의 설정(core.fsmonitor 등)은 사용자가 그 폴더에서 git을 부를 때
/// 샌드박스 밖에서 돈다. 본 checkout과 연결된 worktree 모두 샌드박스와 Edit 거부에 붙는다.
/// `*/`로 시작해 작업 공간의 `.git` 자리는 맞지 않으므로 본 checkout의 커밋은 막지 않는다.
const NESTED_GIT: [&str; 2] = ["*/**/.git", "*/**/.git/**"];
/// 하위 폴더에서도 claude가 읽는 지시 파일.
const NESTED_MEMORY: [&str; 2] = ["**/CLAUDE.md", "**/CLAUDE.local.md"];
/// 하위 폴더의 claude 설정. 사용자가 나중에 그 폴더에서 claude를 열면 프로젝트 설정(훅, MCP 서버)으로 읽힌다.
const NESTED_CONFIG: [&str; 3] = ["**/.claude", "**/.claude/**", "**/.mcp.json"];
/// 저장소 밖의 git 설정. 상대 `core.hooksPath`는 작업 공간 기준이라 전역, 시스템 설정의 값도 작업 공간 안을 가리킬 수 있다.
const SYSTEM_CONFIGS: [&str; 4] = [
    "/etc/gitconfig",
    "/opt/homebrew/etc/gitconfig",
    "/usr/local/etc/gitconfig",
    "/Library/Developer/CommandLineTools/usr/share/git-core/gitconfig",
];
const HOME_CONFIGS: [&str; 2] = [".gitconfig", ".config/git/config"];
/// 샌드박스 경로에서 glob으로 읽히는 글자. 이런 글자가 든 경로는 막는 범위를 장담할 수 없어 받지 않는다.
const GLOB: [char; 3] = ['*', '?', '['];
const POINTER_MAX_BYTES: u64 = 4096;
const CONFIG_MAX_BYTES: u64 = 1 << 20;
/// git의 include 깊이 한도(10)보다 작게 둔다. 넘으면 다 읽었다고 보일 수 없어 받지 않는다.
const INCLUDE_DEPTH: usize = 5;

/// 글자마다 대소문자를 모두 받는 glob. APFS는 기본으로 대소문자를 가리지 않아 `.GIT`, `claude.md`도 git과 claude가 같은 이름으로 연다.
/// APFS는 유니코드로 접어 `ſ`(U+017F)를 s로, `K`(U+212A)를 k로 연다. Seatbelt 정규식은 바이트로 비교해 `[sSſ]`로는 못 막으므로
/// s와 k 자리는 `*`(슬래시가 아닌 아무 글자들)로 넓힌다. 이어진 `*`는 globstar가 되지 않게 하나로 줄인다.
fn any_case(pattern: &str) -> String {
    let mut out = String::new();
    let mut widened = false;
    for char in pattern.chars() {
        let (lower, upper) = (char.to_ascii_lowercase(), char.to_ascii_uppercase());
        if matches!(lower, 's' | 'k') {
            if !widened {
                out.push('*');
            }
            widened = true;
            continue;
        }
        widened = false;
        if lower == upper {
            out.push(char);
        } else {
            out.push_str(&format!("[{lower}{upper}]"));
        }
    }
    out
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// 작은 텍스트 파일 한 줄. 링크는 따라가지 않는다.
fn read_line(path: &Path) -> Option<String> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > POINTER_MAX_BYTES {
        return None;
    }
    let mut content = String::new();
    fs::File::open(path).ok()?.take(POINTER_MAX_BYTES).read_to_string(&mut content).ok()?;
    let line = content.trim_end_matches(['\n', '\r']);
    (!line.is_empty() && !line.contains(['\n', '\r'])).then(|| line.to_owned())
}

/// `.git` 파일의 `gitdir: <경로>`와 관리 폴더의 `commondir`을 따라 (관리 폴더, 공용 폴더)를 찾는다.
/// 관리 폴더가 `<공용 폴더>/worktrees/<이름>`이 아니면 git이 만든 모양이 아니므로 None이다.
fn linked(root: &Path, pointer: &Path) -> Option<(PathBuf, PathBuf)> {
    let gitdir = read_line(pointer)?.strip_prefix("gitdir: ")?.to_owned();
    let admin = fs::canonicalize(root.join(gitdir)).ok()?;
    let common = fs::canonicalize(admin.join(read_line(&admin.join("commondir"))?)).ok()?;
    (admin.parent()? == common.join("worktrees") && admin.is_dir()).then_some((admin, common))
}

/// 설정 파일 내용. 없으면 None이고, 크거나 읽지 못하면 오류다. 전역 설정은 링크인 경우가 많아 따라간다.
fn read_config(path: &Path) -> Result<Option<String>, ()> {
    match fs::metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
        Ok(metadata) if !metadata.is_file() => Ok(None),
        Ok(metadata) if metadata.len() > CONFIG_MAX_BYTES => Err(()),
        Ok(_) => fs::read_to_string(path).map(Some).map_err(|_| ()),
    }
}

/// 따옴표, 역슬래시, 줄 잇기, 주석을 git처럼 풀어 값 하나를 읽는다. 줄 잇기는 `lines`에서 다음 줄을 가져온다.
fn config_value<'a>(first: &str, lines: &mut impl Iterator<Item = &'a str>) -> Option<String> {
    let mut value = String::new();
    let mut space = String::new();
    let mut quoted = false;
    let mut chars: Vec<char> = first.chars().collect();
    let mut index = 0;
    while let Some(&char) = chars.get(index) {
        index += 1;
        match char {
            '\\' => {
                let Some(&next) = chars.get(index) else {
                    chars = lines.next()?.chars().collect();
                    index = 0;
                    continue;
                };
                index += 1;
                value.push_str(&space);
                space.clear();
                value.push(match next {
                    'n' => '\n',
                    't' => '\t',
                    'b' => '\u{8}',
                    '\\' | '"' => next,
                    _ => return None,
                });
            }
            '"' => quoted = !quoted,
            '#' | ';' if !quoted => break,
            ' ' | '\t' if !quoted => {
                if !value.is_empty() {
                    space.push(char);
                }
            }
            _ => {
                value.push_str(&space);
                space.clear();
                value.push(char);
            }
        }
    }
    (!quoted).then_some(value)
}

/// git 설정 한 파일의 (섹션, 키, 값). 섹션은 하위 이름을 뺀 소문자, 키는 소문자다. git이 받지 않을 모양이면 None이다.
fn config_entries(content: &str) -> Option<Vec<(String, String, String)>> {
    let mut found = Vec::new();
    let mut section = String::new();
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let mut rest = line.trim_start();
        if let Some(header) = rest.strip_prefix('[') {
            let mut quoted = false;
            let end = header.char_indices().find_map(|(at, char)| {
                if char == '"' {
                    quoted = !quoted;
                }
                (char == ']' && !quoted).then_some(at)
            })?;
            let name = header[..end].split([' ', '\t', '"', '.']).next().unwrap_or_default();
            section = name.to_ascii_lowercase();
            rest = header[end + 1..].trim_start();
        }
        if rest.is_empty() || rest.starts_with(['#', ';']) {
            continue;
        }
        let key_len = rest.find(|char: char| !(char.is_ascii_alphanumeric() || char == '-')).unwrap_or(rest.len());
        if key_len == 0 {
            return None;
        }
        let key = rest[..key_len].to_ascii_lowercase();
        let after = rest[key_len..].trim_start();
        let value = if after.is_empty() || after.starts_with(['#', ';']) {
            String::new()
        } else {
            config_value(after.strip_prefix('=')?, &mut lines)?
        };
        found.push((section.clone(), key, value));
    }
    Some(found)
}

/// `..`와 `.`를 글자로 푼다. 없는 경로도 막아야 하므로 파일 시스템을 보지 않는다.
fn normal(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// 설정의 경로 값. `~/`는 홈, 상대 경로는 `base` 기준이다. `~user`, `%(prefix)`는 풀 수 없어 None이다.
fn config_path(value: &str, base: &Path, home: &Path) -> Option<PathBuf> {
    let path = match value.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if value == "~" => home.to_owned(),
        None if value.starts_with('~') || value.starts_with("%(") => return None,
        None => base.join(value),
    };
    Some(normal(&path))
}

/// 설정 파일을 include까지 따라 읽어 `core.hooksPath`의 폴더와 include된 파일을 모은다. 링크면 실제 경로도 더한다.
/// 풀 수 없는 값, 너무 깊은 include, 읽지 못하는 파일이 있으면 None이다. `includeIf`는 조건과 상관없이 따라간다.
fn config_targets(files: Vec<PathBuf>, root: &Path, home: &Path) -> Option<Vec<PathBuf>> {
    let mut queue: VecDeque<(PathBuf, usize)> = files.into_iter().map(|file| (file, 0)).collect();
    let mut found = Vec::new();
    while let Some((file, depth)) = queue.pop_front() {
        let Some(content) = read_config(&file).ok()? else { continue };
        for (section, key, value) in config_entries(&content)? {
            let target = match (section.as_str(), key.as_str()) {
                ("core", "hookspath") if !value.is_empty() => config_path(&value, root, home)?,
                ("include" | "includeif", "path") if !value.is_empty() => {
                    let path = config_path(&value, file.parent()?, home)?;
                    if depth + 1 > INCLUDE_DEPTH {
                        return None;
                    }
                    queue.push_back((path.clone(), depth + 1));
                    path
                }
                _ => continue,
            };
            let real = fs::canonicalize(&target).ok().filter(|real| *real != target);
            found.extend(std::iter::once(target).chain(real));
        }
    }
    Some(found)
}

/// 세션의 작업 공간. kairos-run의 현재 폴더이고 claude가 그대로 물려받는다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionWorkspace {
    root: PathBuf,
    /// glob이 없는 막을 경로. Edit 거부도 이 경로마다 붙는다.
    deny_paths: Vec<String>,
    /// 샌드박스 denyWrite. `deny_paths` 뒤에 `globs`가 붙는다.
    deny_write: Vec<String>,
    /// 대소문자를 가리지 않는 작업 공간 이름, 작업 공간 안 설정 대상, `.git` 포인터, 하위 `.git`, 하위 CLAUDE.md와 claude 설정 glob.
    /// Edit 거부에도 들어간다. 포인터 glob은 고정 규칙과 같아 한 번만 낸다.
    globs: Vec<String>,
    /// 작업 공간의 git 자리 전체. `.git` 폴더, 또는 연결된 worktree의 포인터와 공용 폴더다. 읽는 세션이 통째로 쓰지 못한다.
    git_dirs: Vec<String>,
    /// 공용 폴더의 모든 worktree 관리 폴더(나중에 생긴 것 포함)에서 git을 다른 폴더로 돌리는 파일의 glob. 연결된 worktree에만 있다.
    admin_globs: Vec<String>,
}

impl SessionWorkspace {
    /// 작업 공간의 `.git`을 읽어 막을 경로를 정한다. 경로에 glob 글자가 있으면 거부한다. 오류 코드에 경로를 넣지 않는다.
    /// `.git`이 연결된 worktree의 포인터이면 포인터, 공용 폴더의 설정과 실행 경로, 자기 관리 폴더의 포인터, 다른 worktree의 관리 폴더를 막는다.
    /// `.git`이 폴더이면 그 안의 같은 경로를 막는다. 없거나 읽지 못하면 `.git` 자리를 통째로 막는다. 이때 커밋은 실패하고 세션은 멈춰 알린다.
    /// 이어서 작업 공간의 hook 폴더와, 저장소, 전역(`user_home`), 시스템 설정의 `core.hooksPath`와 include된 파일을 막는다.
    /// 설정을 다 읽었다고 보일 수 없으면 거부한다.
    pub fn discover(root: &Path, user_home: &Path) -> KairosResult<Self> {
        let unsafe_path = |path: &Path| path.to_str().is_none_or(|value| value.contains(GLOB));
        if !root.is_absolute() || unsafe_path(root) {
            return Err(KairosError::new("session_workspace_unsafe"));
        }
        let dot_git = root.join(".git");
        let is_dir = fs::symlink_metadata(&dot_git).is_ok_and(|metadata| metadata.is_dir());
        let mut deny: Vec<PathBuf> = Vec::new();
        let mut git_dirs: Vec<PathBuf> = vec![dot_git.clone()];
        let mut admin_globs: Vec<String> = Vec::new();
        let mut configs: Vec<PathBuf> = SYSTEM_CONFIGS.iter().map(PathBuf::from).collect();
        configs.extend(HOME_CONFIGS.iter().map(|name| user_home.join(name)));
        if is_dir {
            deny.extend(COMMON_DENIED.iter().chain(&MAIN_DENIED).map(|name| dot_git.join(name)));
            configs.extend([dot_git.join("config"), dot_git.join("config.worktree")]);
        } else {
            deny.push(dot_git.clone());
            if let Some((admin, common)) = linked(root, &dot_git) {
                deny.extend(COMMON_DENIED.iter().map(|name| common.join(name)));
                deny.extend(ADMIN_DENIED.iter().map(|name| admin.join(name)));
                let mut siblings: Vec<PathBuf> = fs::read_dir(common.join("worktrees"))
                    .map(|entries| entries.filter_map(|entry| Some(entry.ok()?.path())).collect())
                    .unwrap_or_default();
                siblings.sort();
                deny.extend(siblings.into_iter().filter(|path| path.file_name() != admin.file_name()));
                // 형제 폴더 막기는 띄울 때 본 목록이다. 뒤에 생긴 run의 관리 폴더도 공용 폴더를 돌리지 못하게 이름으로 막는다.
                // 공용 폴더 경로는 아래에서 glob 글자를 거부한 뒤에만 쓰인다.
                admin_globs.extend(
                    ADMIN_DENIED
                        .iter()
                        .map(|name| text(&common.join(any_case("worktrees")).join("*").join(any_case(name)))),
                );
                git_dirs.push(common.clone());
                configs.extend([common.join("config"), admin.join("config.worktree")]);
            }
        }
        deny.extend(WORKSPACE_DENIED.iter().map(|name| root.join(name)));
        let mut targets: Vec<PathBuf> = Vec::new();
        for target in
            config_targets(configs, root, user_home).ok_or_else(|| KairosError::new("session_workspace_unsafe"))?
        {
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        if deny.iter().chain(&targets).chain(&git_dirs).any(|path| unsafe_path(path)) {
            return Err(KairosError::new("session_workspace_unsafe"));
        }
        deny.extend(targets.iter().filter(|target| !deny.contains(target)).cloned().collect::<Vec<_>>());
        let deny_paths: Vec<String> = deny.iter().map(|path| text(path)).collect();
        // glob은 검사가 끝난 뒤에 고정 글자로만 붙인다. 바깥 값은 위에서 glob 글자를 거부했다.
        // 작업 공간 안을 가리키는 설정 대상(hook 폴더, include 파일)은 대소문자 변형도 막는다.
        let inside: Vec<String> =
            targets.iter().filter_map(|target| Some(text(target.strip_prefix(root).ok()?))).collect();
        // `.git`이 폴더가 아니면(연결된 worktree의 포인터 파일, 없거나 읽지 못한 자리) 커밋이 쓰지 않는다. 그 대소문자 변형도 샌드박스에서 막는다.
        let pointer = if is_dir { Vec::new() } else { vec![".git".to_owned()] };
        let mut globs: Vec<String> = Vec::new();
        for glob in WORKSPACE_DENIED
            .iter()
            .map(|name| (*name).to_owned())
            .chain(inside)
            .chain(pointer)
            .flat_map(|name| [any_case(&name), format!("{}/**", any_case(&name))])
            .chain(NESTED_GIT.iter().chain(&NESTED_MEMORY).chain(&NESTED_CONFIG).map(|pattern| any_case(pattern)))
            .map(|pattern| text(&root.join(pattern)))
        {
            // hooksPath가 `.husky`처럼 고정 이름과 같으면 같은 glob이 다시 나온다. 한 번만 둔다.
            if !globs.contains(&glob) {
                globs.push(glob);
            }
        }
        Ok(Self {
            root: root.to_owned(),
            deny_write: deny_paths.iter().cloned().chain(globs.clone()).collect(),
            deny_paths,
            globs,
            // APFS가 `.GIT`도 같은 폴더로 여므로 작업 공간의 `.git` 자리는 대소문자 무시 glob으로도 막는다.
            git_dirs: git_dirs
                .iter()
                .map(|path| text(path))
                .chain([text(&root.join(any_case(".git"))), text(&root.join(any_case(".git")).join("**"))])
                .collect(),
            admin_globs,
        })
    }

    /// 읽는 세션이 쓰지 못하는 git 자리. 읽는 세션은 커밋하지 않으므로 공용 .git을 통째로 막는다.
    /// 다른 run의 관리 폴더, 공유 ref, replace ref, grafts를 고쳐 `kairos pr`이 소유자 자격 증명으로 도는 git을 돌리지 못하게 한다.
    pub fn git_dirs(&self) -> &[String] {
        &self.git_dirs
    }

    /// 고치는 세션이 더 막는 관리 폴더 glob. 띄운 뒤 생긴 다른 run의 관리 폴더에도 걸린다. 자기 관리 폴더의 같은 파일은 이미 막혀 있다.
    pub fn admin_globs(&self) -> &[String] {
        &self.admin_globs
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 샌드박스 denyWrite에 더할 경로. 공용 폴더 쪽은 연결된 worktree에서 샌드박스가 여는 공용 .git 안의 막을 곳이고,
    /// 작업 공간 쪽은 `.git` 포인터와 hook 폴더, 설정이 가리키는 곳은 hook 폴더와 include된 설정 파일이다.
    /// 끝에 대소문자를 가리지 않는 작업 공간 이름, 작업 공간 안 설정 대상, `.git` 포인터, 하위 `.git`, 하위 CLAUDE.md와 claude 설정 glob이 붙는다.
    pub fn deny_write(&self) -> &[String] {
        &self.deny_write
    }

    /// Edit, Write의 거부 규칙. `//`로 시작하는 절대 경로다. 샌드박스 denyWrite의 경로마다 그 자리와 아래를 모두 막아
    /// Bash와 파일 도구가 같은 곳을 막는다. 파일 도구는 작업 공간 밖 쓰기 차단에만 기대지 않는다.
    pub fn denied_edits(&self) -> impl Iterator<Item = String> + '_ {
        // 작업 공간의 `.git` 자리는 고정 규칙이 이미 막으니 같은 규칙을 두 번 내지 않는다. 연결된 worktree의 포인터 glob도 같다.
        let dot_git = text(&self.root.join(".git"));
        let fixed: Vec<String> = EDIT_DENIED.iter().map(|relative| text(&self.root.join(any_case(relative)))).collect();
        let paths = self
            .deny_paths
            .iter()
            .filter(move |path| **path != dot_git)
            .flat_map(|path| [format!("Edit(/{path})"), format!("Edit(/{path}/**)")]);
        let globs: Vec<String> = self
            .globs
            .iter()
            .filter(|pattern| !fixed.contains(pattern))
            .map(|pattern| format!("Edit(/{pattern})"))
            .collect();
        fixed.into_iter().map(|pattern| format!("Edit(/{pattern})")).chain(paths).chain(globs)
    }
}

#[cfg(test)]
mod tests {
    use super::any_case;

    #[test]
    fn 글자만_두_경우의_묶음이_되고_나머지는_그대로다() {
        assert_eq!(any_case(".git"), ".[gG][iI][tT]");
        assert_eq!(any_case("CLAUDE.md"), "[cC][lL][aA][uU][dD][eE].[mM][dD]");
        assert_eq!(any_case("*/**/.git/**"), "*/**/.[gG][iI][tT]/**");
        assert_eq!(any_case("-_0/"), "-_0/");
        assert_eq!(any_case(".husky"), ".[hH][uU]*[yY]");
        assert_eq!(any_case(".githooks"), ".[gG][iI][tT][hH][oO][oO]*");
        assert_eq!(any_case(".mcp.json"), ".[mM][cC][pP].[jJ]*[oO][nN]");
    }
}
