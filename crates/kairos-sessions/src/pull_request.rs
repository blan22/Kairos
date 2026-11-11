//! 고치는 세션이 커밋한 일을 draft PR로 올린다. 사용자가 세션 카드의 'PR 올리기'를 눌렀을 때만 부른다.
//!
//! 세션은 샌드박스 안에서 로컬 커밋까지만 한다. push와 PR 생성은 세션 밖의 kairos가 사용자의 git, gh 로그인으로 한다.
//! 세션은 PR을 제안만 한다. 커밋 제목이 PR 제목, 커밋 목록이 PR 본문이 된다. 올리는 결정은 사용자가 한다.
//!
//! - 대상 저장소와 기준 브랜치는 프로필의 플레이북 대상에서 온다. 세션이 정한 값을 쓰지 않는다.
//! - 브랜치는 `kairos/`로 시작해야 한다. kairos가 만든 worktree 브랜치만 올린다.
//! - git은 인자 배열로 부르고 `core.hooksPath=/dev/null`로 저장소 훅을, `--no-replace-objects`로 replace ref를 끈다. 세션이 고친 코드가 샌드박스 밖에서 돌지 않게 한다.
//! - 공용 .git은 같은 클론의 다른 세션도 쓸 수 있다. 저장소 설정이 명령이나 원격 주소를 바꾸면 git을 더 부르지 않는다(`config_safe`).
//! - kairos-run이 이 run으로 띄운 세션만 올린다. 이어받은 세션은 다른 플레이북으로 떴을 수 있다.
//! - PR은 언제나 draft다. 이미 열린 PR이 있으면 push만 하고 그 주소를 돌려준다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use kairos_agenda::{ItemRepo, Playbook, ReadOnlyRunRepo, issue_url_matches};
use kairos_core::KairosResult;
use kairos_core::process::{ProcessEnd, ProcessRequest, ProcessRunner};
use kairos_core::profile::{self, PlaybookTarget};
use kairos_core::string_enum;
use kairos_ledger::ObjectRepo;
use kairos_store::Database;
use regex::Regex;
use serde::Serialize;

use crate::git_host::GIT_SELECTOR_PREFIX;
use crate::orca::{OrcaClient, OrcaWorktree};
use crate::run::{Run, RunRepo};

/// 합이 패널의 제한(115초) 안에 들게 잡는다. git 셋, push, gh 둘이다.
const GIT_TIMEOUT: Duration = Duration::from_secs(8);
const PUSH_TIMEOUT: Duration = Duration::from_secs(45);
const GH_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT: usize = 256 * 1024;
const TITLE_MAX_CHARS: usize = 200;
const BODY_MAX_CHARS: usize = 20_000;
/// 본문에 싣는 커밋 수. 넘으면 줄여 적는다.
const BODY_MAX_COMMITS: usize = 30;

static SHA: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{40}$").expect("상수 정규식"));
/// 저장소, worktree 설정에 있으면 올리지 않는 키. 세션이나 다른 세션이 공용 .git에 심은 설정이 소유자 자격 증명으로 도는 git에서
/// 명령을 고르거나(ssh, 자격 증명 도우미, upload-pack, 필터, 서명), 원격 주소를 바꾸는(insteadOf, 프록시) 자리다. 섹션째 막는 것과 키만 막는 것이 있다.
const UNSAFE_SECTIONS: [&str; 6] = ["credential", "url", "protocol", "filter", "gpg", "http"];
/// core.fsmonitor, core.hooksPath, push.followTags, push.recurseSubmodules는 `-c`로 덮으므로 여기 두지 않는다.
/// core.worktree는 status가 보는 폴더를 바꾼다.
const UNSAFE_KEYS: [&str; 6] =
    ["core.sshcommand", "core.gitproxy", "core.askpass", "core.worktree", "push.gpgsign", "include.path"];
/// remote.<이름>.<키> 가운데 실행할 명령이나 다른 전송을 고르거나, 고정한 refspec 밖을 함께 올리는 키.
const UNSAFE_REMOTE_KEYS: [&str; 5] = ["uploadpack", "receivepack", "proxy", "vcs", "followtags"];

/// 바뀌면 올리지 않는 경로. push만으로 원격 CI가 세션의 코드를 저장소 비밀과 함께 돌리거나 리뷰 규칙이 바뀐다.
const BLOCKED_PREFIXES: [&str; 5] = [".github/", ".circleci/", ".buildkite/", ".gitlab/", ".husky/"];
const BLOCKED_NAMES: [&str; 4] = ["CODEOWNERS", ".gitlab-ci.yml", "Jenkinsfile", ".travis.yml"];

/// 원격 브랜치 이름. 세션이 고른 HEAD 이름을 쓰지 않고 run ID로만 정한다(`worktree_name`).
/// 다른 세션이나 다른 사람의 브랜치에 올리지 않는다.
fn branch_of_run(run_id: &str) -> String {
    format!("kairos/{}", crate::launch::worktree_name(run_id))
}

/// origin이 대상 저장소의 github.com 주소인가. https, `git@github.com:`, `ssh://git@github.com/` 세 모양만 받는다.
/// 끝의 `.git`과 `/`는 있어도 된다. 대소문자는 가리지 않는다.
fn origin_matches(url: &str, repo: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    let path = ["https://github.com/", "git@github.com:", "ssh://git@github.com/"]
        .iter()
        .find_map(|prefix| url.strip_prefix(prefix));
    let Some(path) = path else { return false };
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    path == repo.to_ascii_lowercase()
}

/// `git config --list --show-scope -z`의 (범위, 키, 값). 키는 소문자다.
fn config_entries(out: &str) -> Vec<(&str, String, &str)> {
    let mut fields = out.split('\0');
    let mut found = Vec::new();
    while let (Some(scope), Some(entry)) = (fields.next(), fields.next()) {
        let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
        found.push((scope, key.to_ascii_lowercase(), value));
    }
    found
}

/// 저장소, worktree 범위의 설정이 소유자 git을 돌려 쓸 수 있는가. 전역, 시스템 설정은 소유자 것이라 믿는다.
/// origin과 pushurl은 범위와 상관없이 모두 대상 저장소여야 하고 origin이 하나는 있어야 한다.
fn config_safe(out: &str, repo: &str) -> bool {
    let entries = config_entries(out);
    let mut origin = false;
    for (scope, key, value) in &entries {
        if matches!(key.as_str(), "remote.origin.url" | "remote.origin.pushurl") {
            if !origin_matches(value, repo) {
                return false;
            }
            origin |= key == "remote.origin.url";
        }
        if !matches!(*scope, "local" | "worktree") {
            continue;
        }
        let section = key.split('.').next().unwrap_or_default();
        let remote_key = key.strip_prefix("remote.").and_then(|rest| rest.rsplit_once('.')).map(|(_, last)| last);
        if UNSAFE_SECTIONS.contains(&section)
            || UNSAFE_KEYS.contains(&key.as_str())
            || key.starts_with("includeif.")
            || remote_key.is_some_and(|last| UNSAFE_REMOTE_KEYS.contains(&last))
        {
            return false;
        }
    }
    origin
}

/// `git ls-remote` 출력에서 이름이 정확히 같은 ref의 SHA. ls-remote의 패턴은 끝부분만 맞추므로 이름을 다시 본다.
/// 같은 이름이 두 번 나오거나 SHA 모양이 아니면 모른다고 본다.
fn remote_sha(out: &str, name: &str) -> Option<String> {
    let mut found = out.lines().filter_map(|line| line.split_once('\t')).filter(|(_, line_ref)| *line_ref == name);
    let (sha, _) = found.next()?;
    (found.next().is_none() && SHA.is_match(sha)).then(|| sha.to_owned())
}

/// 대소문자는 가리지 않는다. 따옴표로 시작하는 경로(인용된 이름)는 막는 쪽으로 본다.
fn blocked_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.starts_with('"')
        || BLOCKED_PREFIXES.iter().any(|prefix| path.starts_with(prefix))
        || BLOCKED_NAMES.iter().any(|name| {
            let name = name.to_ascii_lowercase();
            path == name || path.ends_with(&format!("/{name}"))
        })
}

/// 세션이 쓴 여러 줄 글은 코드 블록 안에 싣는다. 링크, 이미지, HTML이 그려지지 않는다. 블록을 닫지 못하게 backtick을 바꾼다.
fn fenced(text: &str) -> String {
    format!("```text\n{}\n```\n\n", text.replace('`', "'"))
}

/// 세션이 쓴 글을 사용자 이름으로 게시하기 전에 멘션, 이슈 참조, 링크를 무력화한다.
/// `@팀` 알림, `Closes #1`이나 URL로 다른 이슈 닫기, 외부 링크 게시를 막는다.
fn neutral(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(char) = chars.next() {
        out.push(char);
        let next = chars.peek().copied();
        let breaks = match char {
            '@' => true,
            '#' => next.is_some_and(|next| next.is_ascii_digit()),
            ':' => next == Some('/'),
            _ => false,
        };
        if breaks {
            out.push('\u{200b}');
        }
    }
    out
}

string_enum! {
    /// created     새 draft PR을 만들었다.
    /// updated     이미 열린 PR이 있어 push만 했다.
    /// not_found   항목이 없다.
    /// no_session  이 항목에 세션이 없거나 worktree를 모른다.
    /// not_fix_session  고치는 세션(qa_issue)이 아니다. 읽는 세션은 커밋하지 않는다.
    /// adopted_session  이미 열린 세션을 이어받은 기록이다. kairos-run이 고치는 세션 프로필로 띄운 세션만 올린다.
    /// unsafe_repo_config  저장소 설정이 명령이나 원격 주소를 바꾼다. 소유자 자격 증명으로 git을 돌리지 않는다.
    /// session_running  세션이 아직 열려 있다. 끝난 세션만 올린다.
    /// blocked_paths    CI, 리뷰 설정 경로를 바꿨다.
    /// not_eligible     플레이북 대상 저장소가 아니다.
    /// no_worktree      worktree 폴더를 찾지 못했다.
    /// base_unavailable 기준 브랜치를 원격에서 받지 못했다.
    /// no_commits       기준 브랜치 뒤로 커밋이 없다.
    /// push_failed, create_failed  그 단계에서 실패했다.
    pub enum PrStatus {
        Created => "created",
        Updated => "updated",
        NotFound => "not_found",
        NoSession => "no_session",
        NotFixSession => "not_fix_session",
        AdoptedSession => "adopted_session",
        UnsafeRepoConfig => "unsafe_repo_config",
        SessionRunning => "session_running",
        BlockedPaths => "blocked_paths",
        NotEligible => "not_eligible",
        NoWorktree => "no_worktree",
        BaseUnavailable => "base_unavailable",
        NoCommits => "no_commits",
        PushFailed => "push_failed",
        CreateFailed => "create_failed",
    }
}

impl PrStatus {
    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Created | Self::Updated)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PrOutcome {
    pub id: i64,
    pub status: PrStatus,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// 커밋하지 않은 변경이 worktree에 남았다. PR에는 들지 않는다.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub uncommitted: bool,
}

impl PrOutcome {
    fn of(id: i64, status: PrStatus, message: &str) -> Self {
        Self { id, status, message: message.to_owned(), url: None, branch: None, uncommitted: false }
    }
}

/// PR 제목과 본문. 커밋에서 만든다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrDraft {
    pub title: String,
    pub body: String,
}

impl PrDraft {
    /// `subjects`는 오래된 것부터다. 커밋이 하나면 그 본문을 PR 본문 앞에 싣는다.
    pub fn from_commits(commits: &[(String, String)], issue_url: &str) -> Option<Self> {
        let (first_subject, first_body) = commits.first()?;
        let title = neutral(&clip(first_subject.trim(), TITLE_MAX_CHARS));
        if title.is_empty() {
            return None;
        }
        let mut text = String::new();
        if commits.len() == 1 {
            text.push_str(first_body.trim());
        } else {
            for (subject, _) in commits.iter().take(BODY_MAX_COMMITS) {
                text.push_str(&format!("- {}\n", subject.trim()));
            }
            if commits.len() > BODY_MAX_COMMITS {
                text.push_str(&format!("- 외 {}개\n", commits.len() - BODY_MAX_COMMITS));
            }
        }
        let text = clip(text.trim(), BODY_MAX_CHARS);
        let mut body = if text.is_empty() { String::new() } else { fenced(&neutral(&text)) };
        body.push_str(&format!("이슈: {issue_url}\n\nkairos 세션이 제안하고 사용자가 올린 draft PR입니다."));
        Some(Self { title, body })
    }
}

fn clip(text: &str, max: usize) -> String {
    text.chars().filter(|char| !char.is_control() || *char == '\n').take(max).collect()
}

/// git과 gh를 부르는 쪽. 경로와 환경은 조립 지점이 정한다.
pub struct PullRequestPublisher<'a> {
    database: &'a Database,
    orca: &'a OrcaClient,
    runner: Arc<dyn ProcessRunner>,
    git_bin: String,
    gh_bin: String,
    env: BTreeMap<String, String>,
    data_folder: PathBuf,
}

impl<'a> PullRequestPublisher<'a> {
    /// `path`와 `home`은 사용자의 git 자격 증명 도우미와 gh 로그인을 찾는 데 쓴다.
    pub fn new(
        database: &'a Database,
        orca: &'a OrcaClient,
        runner: Arc<dyn ProcessRunner>,
        bins: (&str, &str),
        path: &str,
        home: &str,
        data_folder: &Path,
    ) -> Self {
        // 상대 항목은 worktree 기준으로 풀려 세션이 심은 실행 파일을 고를 수 있다. 절대 경로만 남긴다.
        let path = path.split(':').filter(|entry| entry.starts_with('/')).collect::<Vec<_>>().join(":");
        let env = BTreeMap::from([
            ("PATH".to_owned(), path),
            ("HOME".to_owned(), home.to_owned()),
            ("LANG".to_owned(), "en_US.UTF-8".to_owned()),
            ("GIT_TERMINAL_PROMPT".to_owned(), "0".to_owned()),
            ("GH_HOST".to_owned(), "github.com".to_owned()),
            ("GH_PROMPT_DISABLED".to_owned(), "1".to_owned()),
            ("GH_NO_UPDATE_NOTIFIER".to_owned(), "1".to_owned()),
            ("NO_COLOR".to_owned(), "1".to_owned()),
        ]);
        Self {
            database,
            orca,
            runner,
            git_bin: bins.0.to_owned(),
            gh_bin: bins.1.to_owned(),
            env,
            data_folder: data_folder.to_path_buf(),
        }
    }

    fn git(&self, worktree: &Path, args: &[&str], timeout: Duration) -> (ProcessEnd, String) {
        // 세션은 저장소 설정을 쓰지 못한다(SessionWorkspace의 denyWrite). 그래도 설정이 고르는 실행 경로는 한 번 더 끈다.
        // 남은 키는 `config_safe`가 먼저 본다. replace ref는 log, diff가 다른 커밋을 보게 하므로 끈다.
        let mut argv = vec![
            "-C",
            worktree.to_str().unwrap_or_default(),
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.pager=cat",
            "-c",
            "core.quotePath=false",
            "-c",
            "push.followTags=false",
            "-c",
            "push.recurseSubmodules=no",
            "-c",
            "core.hooksPath=/dev/null",
            "--no-replace-objects",
        ];
        argv.extend_from_slice(args);
        let request =
            ProcessRequest::new(self.git_bin.clone(), argv, timeout).env(self.env.clone()).max_output(MAX_OUTPUT);
        let output = self.runner.run(&request);
        (output.end, output.stdout)
    }

    fn gh(&self, worktree: &Path, args: &[&str]) -> (ProcessEnd, String) {
        let request = ProcessRequest::new(self.gh_bin.clone(), args.to_vec(), GH_TIMEOUT)
            .env(self.env.clone())
            .cwd(worktree)
            .max_output(MAX_OUTPUT);
        let output = self.runner.run(&request);
        (output.end, output.stdout)
    }

    /// 항목의 마지막 run. 같은 이슈의 다른 항목이 연 세션도 찾는다(`move`와 같다).
    fn run_of(&self, item_id: i64, object_key: &str) -> KairosResult<Option<Run>> {
        let connection = self.database.connection();
        let runs = RunRepo::new(connection);
        match ReadOnlyRunRepo::new(connection).active_on_object(object_key)? {
            Some(sibling) => runs.get(&sibling.id),
            None => runs.latest(item_id),
        }
    }

    fn worktree_path(&self, selector: &str) -> Option<PathBuf> {
        let path = match selector.strip_prefix(GIT_SELECTOR_PREFIX) {
            Some(path) => PathBuf::from(path),
            None => PathBuf::from(self.orca.worktree_show(selector).result().and_then(OrcaWorktree::read)?.path?),
        };
        (path.is_absolute() && path.is_dir() && !path.starts_with(&self.data_folder)).then_some(path)
    }

    /// 기준 브랜치 뒤의 커밋(오래된 것부터). 제목과 본문이다.
    fn commits(&self, worktree: &Path, base: &str, sha: &str) -> Option<Vec<(String, String)>> {
        let range = format!("{base}..{sha}");
        let (end, out) =
            self.git(worktree, &["log", "--reverse", "--no-merges", "--format=%s%x1f%b%x1e", &range], GIT_TIMEOUT);
        if end != ProcessEnd::Exited(0) {
            return None;
        }
        Some(
            out.split('\u{1e}')
                .map(|record| record.trim_start_matches('\n'))
                .filter(|record| !record.is_empty())
                .map(|record| {
                    let (subject, body) = record.split_once('\u{1f}').unwrap_or((record, ""));
                    (subject.to_owned(), body.to_owned())
                })
                .collect(),
        )
    }

    fn existing_pr(&self, worktree: &Path, repo: &str, branch: &str) -> Option<String> {
        // 내가 연 PR만 이어 쓴다.
        let (end, out) = self.gh(
            worktree,
            &["pr", "list", "--repo", repo, "--head", branch, "--author", "@me", "--json", "url", "--jq", ".[0].url"],
        );
        (end == ProcessEnd::Exited(0)).then(|| pr_url(&out, repo)).flatten()
    }

    pub fn publish(&self, id: i64) -> KairosResult<PrOutcome> {
        let Some(item) = ItemRepo::new(self.database.connection()).get(id)? else {
            return Ok(PrOutcome::of(id, PrStatus::NotFound, "항목 없음"));
        };
        let Some(run) = self.run_of(id, &item.object_key)? else {
            return Ok(PrOutcome::of(id, PrStatus::NoSession, "이 항목에 세션이 없습니다"));
        };
        if run.playbook != Playbook::QaIssue {
            return Ok(PrOutcome::of(id, PrStatus::NotFixSession, "코드를 고치는 세션이 아니라 올릴 커밋이 없습니다"));
        }
        // 일하는 세션은 확인과 push 사이에 브랜치를 바꿀 수 있다. 끝난 세션만 올린다.
        if run.state.is_active() {
            return Ok(PrOutcome::of(
                id,
                PrStatus::SessionRunning,
                "세션이 아직 열려 있습니다. 세션을 끝낸 뒤 올리세요",
            ));
        }
        // 이어받은 세션은 다른 플레이북(읽는 세션)으로 떴을 수 있다. 그 worktree는 고치는 세션의 막기를 받지 않았다.
        // kairos-run이 이 run으로 띄운 세션(session_started_at)만 올린다.
        if run.adopted || run.session_started_at.is_none() {
            return Ok(PrOutcome::of(
                id,
                PrStatus::AdoptedSession,
                "kairos가 고치는 세션으로 띄운 기록이 아니라 올리지 않습니다. 직접 확인한 뒤 올리세요",
            ));
        }
        if !self.gh_bin.starts_with('/') {
            return Ok(PrOutcome::of(id, PrStatus::CreateFailed, "gh를 찾지 못했습니다"));
        }
        let object = ObjectRepo::new(self.database.connection()).get(&item.object_key)?;
        let url = object.and_then(|object| object.permalink).filter(|url| issue_url_matches(url)).unwrap_or_default();
        let profile = profile::current();
        let Some(target): Option<&PlaybookTarget> = profile.playbooks.target_for_url(&url) else {
            return Ok(PrOutcome::of(id, PrStatus::NotEligible, "플레이북 대상 저장소가 아닙니다"));
        };
        let Some(selector) = run.worktree_identity.as_deref() else {
            return Ok(PrOutcome::of(id, PrStatus::NoSession, "세션의 worktree를 모릅니다"));
        };
        let Some(worktree) = self.worktree_path(selector) else {
            return Ok(PrOutcome::of(id, PrStatus::NoWorktree, "세션의 worktree 폴더를 찾지 못했습니다"));
        };
        // 공용 .git은 같은 저장소의 다른 세션도 쓸 수 있다. 설정이 고르는 명령과 원격 주소를 먼저 본다.
        let (end, config) = self.git(&worktree, &["config", "--list", "--show-scope", "-z"], GIT_TIMEOUT);
        if end != ProcessEnd::Exited(0) || !config_safe(&config, &target.workspace_repo) {
            let message = "저장소 설정이 원격 주소나 git이 부르는 명령을 바꿉니다. 직접 확인한 뒤 올리세요";
            return Ok(PrOutcome::of(id, PrStatus::UnsafeRepoConfig, message));
        }
        let branch = branch_of_run(&run.id);
        // 확인한 커밋을 그대로 올린다. 뒤에서 HEAD를 다시 읽지 않는다.
        let (end, sha) = self.git(&worktree, &["rev-parse", "--verify", "HEAD^{commit}"], GIT_TIMEOUT);
        let sha = sha.trim().to_owned();
        if end != ProcessEnd::Exited(0) || !SHA.is_match(&sha) {
            return Ok(PrOutcome::of(id, PrStatus::NoCommits, "커밋을 읽지 못했습니다"));
        }
        let with_branch = |mut outcome: PrOutcome| {
            outcome.branch = Some(branch.clone());
            outcome
        };
        // 기준은 원격에서 지금 받은 커밋이다. 로컬의 원격 추적 ref는 세션이 옮길 수 있어 믿지 않는다.
        let remote_base = target.base_branch.strip_prefix("origin/").unwrap_or(&target.base_branch);
        // FETCH_HEAD는 worktree 관리 폴더에 써서 세션이 심은 링크를 따라갈 수 있다. 쓰지 않고 kairos가 정한 ref로 받는다.
        // ref 갱신은 잠금 파일과 rename으로 해서 링크를 따라가지 않는다.
        let base_ref = format!("refs/kairos/base/{}", crate::launch::worktree_name(&run.id));
        // 받은 ref는 공용 refs에 있어 같은 저장소의 다른 세션이 fetch와 다시 읽기 사이에 바꿀 수 있다.
        // 원격이 알려 준 SHA를 먼저 받아 두고, 다시 읽은 값이 그와 같을 때만 쓴다.
        let remote_head = format!("refs/heads/{remote_base}");
        let (listed, heads) = self.git(&worktree, &["ls-remote", "--refs", "origin", &remote_head], PUSH_TIMEOUT);
        let expected = remote_sha(&heads, &remote_head);
        let refspec_base = format!("+refs/heads/{remote_base}:{base_ref}");
        let (fetched, _) = self.git(
            &worktree,
            &["fetch", "--quiet", "--no-tags", "--no-write-fetch-head", "origin", &refspec_base],
            PUSH_TIMEOUT,
        );
        let verify = format!("{base_ref}^{{commit}}");
        let (end, base) = self.git(&worktree, &["rev-parse", "--verify", &verify], GIT_TIMEOUT);
        let base = base.trim().to_owned();
        let agreed = listed == ProcessEnd::Exited(0) && expected.as_deref() == Some(base.as_str());
        if fetched != ProcessEnd::Exited(0) || end != ProcessEnd::Exited(0) || !SHA.is_match(&base) || !agreed {
            let message =
                "기준 브랜치를 원격에서 받지 못했거나 받은 값이 원격과 다릅니다. 네트워크와 git 자격 증명을 확인하세요";
            return Ok(with_branch(PrOutcome::of(id, PrStatus::BaseUnavailable, message)));
        }
        let commits = self.commits(&worktree, &base, &sha).unwrap_or_default();
        let Some(draft) = PrDraft::from_commits(&commits, &url) else {
            return Ok(with_branch(PrOutcome::of(id, PrStatus::NoCommits, "올릴 커밋이 없습니다")));
        };
        // 두 트리를 바로 비교한다. merge-base와 비교하면 세션이 오래된 커밋 위에 쌓아 비교 기준을 옮길 수 있다.
        let (end, changed) =
            self.git(&worktree, &["diff", "--name-only", "-z", "--no-renames", &base, &sha], GIT_TIMEOUT);
        if end != ProcessEnd::Exited(0) || changed.split('\0').filter(|path| !path.is_empty()).any(blocked_path) {
            let message = "CI나 리뷰 설정(.github 등)을 바꾼 커밋이라 올리지 않습니다. 직접 확인한 뒤 올리세요";
            return Ok(with_branch(PrOutcome::of(id, PrStatus::BlockedPaths, message)));
        }
        let (_, status) = self.git(&worktree, &["status", "--porcelain", "--untracked-files=no"], GIT_TIMEOUT);
        let uncommitted = !status.trim().is_empty();
        let refspec = format!("{sha}:refs/heads/{branch}");
        let (pushed, _) = self.git(&worktree, &["push", "--quiet", "origin", &refspec], PUSH_TIMEOUT);
        if pushed != ProcessEnd::Exited(0) {
            let message = "push하지 못했습니다. git 자격 증명과 원격 권한을 확인하세요";
            return Ok(with_branch(PrOutcome::of(id, PrStatus::PushFailed, message)));
        }
        let repo = target.workspace_repo.as_str();
        let done = |status, message: &str, url: String| {
            let mut outcome = with_branch(PrOutcome::of(id, status, message));
            outcome.url = Some(url);
            outcome.uncommitted = uncommitted;
            outcome
        };
        if let Some(existing) = self.existing_pr(&worktree, repo, &branch) {
            return Ok(done(PrStatus::Updated, "열린 PR에 push했습니다", existing));
        }
        let base = target.base_branch.strip_prefix("origin/").unwrap_or(&target.base_branch);
        let (created, out) = self.gh(
            &worktree,
            &[
                "pr",
                "create",
                "--draft",
                "--repo",
                repo,
                "--base",
                base,
                "--head",
                &branch,
                "--title",
                &draft.title,
                "--body",
                &draft.body,
            ],
        );
        match (created, pr_url(&out, repo)) {
            (ProcessEnd::Exited(0), Some(url)) => Ok(done(PrStatus::Created, "draft PR을 만들었습니다", url)),
            _ => Ok(with_branch(PrOutcome::of(
                id,
                PrStatus::CreateFailed,
                "PR을 만들지 못했습니다. gh 로그인을 확인하세요",
            ))),
        }
    }
}

/// gh가 찍은 PR 주소. 대상 저장소의 PR 주소 모양만 받는다.
fn pr_url(out: &str, repo: &str) -> Option<String> {
    let prefix = format!("https://github.com/{repo}/pull/");
    out.lines().map(str::trim).rev().find_map(|line| {
        let number = line.strip_prefix(&prefix)?;
        (!number.is_empty() && number.len() <= 10 && number.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| line.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ls_remote에서는_이름이_정확히_같은_ref_하나만_받는다() {
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        let name = "refs/heads/main";
        assert_eq!(remote_sha(&format!("{a}\t{name}\n"), name), Some(a.clone()));
        // 끝부분만 같은 ref는 버린다.
        assert_eq!(remote_sha(&format!("{b}\trefs/heads/x/{name}\n{a}\t{name}\n"), name), Some(a.clone()));
        assert_eq!(remote_sha(&format!("{a}\t{name}\n{b}\t{name}\n"), name), None);
        assert_eq!(remote_sha(&format!("{b}\trefs/heads/x/{name}\n"), name), None);
        assert_eq!(remote_sha(&format!("HEAD\t{name}\n"), name), None);
        assert_eq!(remote_sha("", name), None);
    }

    fn commit(subject: &str, body: &str) -> (String, String) {
        (subject.to_owned(), body.to_owned())
    }

    #[test]
    fn 커밋_하나면_제목과_본문을_그대로_쓴다() {
        let draft = PrDraft::from_commits(&[commit("버튼 정렬을 고침", "원인은 flex 방향\n")], "https://x/1").unwrap();
        assert_eq!(draft.title, "버튼 정렬을 고침");
        assert!(draft.body.starts_with("```text\n원인은 flex 방향\n```\n\n이슈: https://x/1"));
    }

    #[test]
    fn 커밋이_여럿이면_첫_제목과_목록을_쓰고_없으면_초안이_없다() {
        let draft = PrDraft::from_commits(&[commit("첫째", ""), commit("둘째", "")], "u").unwrap();
        assert_eq!(draft.title, "첫째");
        assert!(draft.body.contains("- 첫째\n- 둘째\n```"));
        let fence = PrDraft::from_commits(&[commit("t", "```\n[x](https://evil)")], "u").unwrap();
        assert_eq!(fence.body.matches("```").count(), 2, "세션 글이 블록을 닫지 못한다");
        assert_eq!(PrDraft::from_commits(&[], "u"), None);
        assert_eq!(PrDraft::from_commits(&[commit("  ", "")], "u"), None);
    }

    #[test]
    fn pr_주소는_대상_저장소의_모양만_받는다() {
        assert_eq!(
            pr_url("Creating...\nhttps://github.com/acme/web/pull/12\n", "acme/web").as_deref(),
            Some("https://github.com/acme/web/pull/12")
        );
        assert_eq!(pr_url("https://github.com/evil/web/pull/12", "acme/web"), None);
        assert_eq!(pr_url("https://github.com/acme/web/pull/12x", "acme/web"), None);
    }

    fn scoped(entries: &[(&str, &str, &str)]) -> String {
        entries.iter().map(|(scope, key, value)| format!("{scope}\0{key}\n{value}\0")).collect()
    }

    #[test]
    fn origin은_대상_저장소의_github_주소만_받는다() {
        for url in [
            "https://github.com/acme/web.git",
            "https://github.com/Acme/Web",
            "git@github.com:acme/web.git",
            "ssh://git@github.com/acme/web/",
        ] {
            assert!(origin_matches(url, "acme/web"), "{url}");
        }
        for url in ["https://github.com/evil/web.git", "git@evil.example:acme/web.git", "/tmp/acme/web", "ext::sh"] {
            assert!(!origin_matches(url, "acme/web"), "{url}");
        }
    }

    #[test]
    fn 저장소_설정이_명령이나_원격을_바꾸면_올리지_않는다() {
        let origin = ("local", "remote.origin.url", "https://github.com/acme/web.git");
        assert!(config_safe(&scoped(&[("global", "credential.helper", "osxkeychain"), origin]), "acme/web"));
        assert!(!config_safe(&scoped(&[("global", "user.name", "x")]), "acme/web"), "origin이 없다");
        for unsafe_entry in [
            ("local", "core.sshCommand", "touch /tmp/x"),
            ("local", "credential.helper", "!touch /tmp/x"),
            ("worktree", "remote.origin.uploadpack", "touch /tmp/x"),
            ("local", "url.https://evil.example/.insteadOf", "https://github.com/"),
            ("local", "protocol.ext.allow", "always"),
            ("local", "filter.x.clean", "touch /tmp/x"),
            ("local", "includeIf.onbranch:main.path", "/tmp/x"),
            ("local", "remote.origin.pushurl", "git@evil.example:acme/web.git"),
            ("local", "remote.origin.followTags", "true"),
            ("worktree", "core.worktree", "/tmp/elsewhere"),
        ] {
            assert!(!config_safe(&scoped(&[origin, unsafe_entry]), "acme/web"), "{unsafe_entry:?}");
        }
        assert!(!config_safe(&scoped(&[("local", "remote.origin.url", "/tmp/planted")]), "acme/web"));
    }

    #[test]
    fn 원격_브랜치는_run_id로만_정한다() {
        assert_eq!(branch_of_run("1a2b3c4d-0000-4000-8000-000000000000"), "kairos/t-1a2b3c4d");
    }

    #[test]
    fn ci와_리뷰_설정_경로는_막는다() {
        assert!(blocked_path(".github/workflows/ci.yml"));
        assert!(blocked_path("docs/CODEOWNERS"));
        assert!(blocked_path(".gitlab-ci.yml"));
        assert!(blocked_path(".GitHub/workflows/ci.yml"));
        assert!(blocked_path("\".github/workflows/\\355.yml\""));
        assert!(!blocked_path("src/github/client.ts"));
    }

    #[test]
    fn 멘션과_이슈_참조와_링크는_무력화한다() {
        let text = neutral("@acme/team Closes #12 https://evil.example a#b");
        assert!(!text.contains("@a") && !text.contains("#1") && !text.contains("://"));
        assert!(text.contains("a#b"));
        let draft = PrDraft::from_commits(&[commit("@x 고침", "Fixes #3")], "https://github.com/o/r/issues/1").unwrap();
        assert!(!draft.title.contains("@x"));
        assert!(draft.body.ends_with(
            "이슈: https://github.com/o/r/issues/1\n\nkairos 세션이 제안하고 사용자가 올린 draft PR입니다."
        ));
    }
}
