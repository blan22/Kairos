use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use kairos_core::js;
use kairos_core::process::{ProcessEnd, ProcessRequest, ProcessRunner};
use serde_json::{Map, Value};

use super::replies::{OrcaTerminal, PsSnapshot, record, text};
use crate::launch::terminal_title;

/// 패널은 accept를 115초에 끊는다(plugins/orca-panel/src/config.ts). 호출 제한의 합이 그 안에 들어오게 둔다. 끊겨도 추적이 정리한다.
pub const ORCA_READ: Duration = Duration::from_secs(8);
pub const ORCA_TERMINAL: Duration = Duration::from_secs(15);
/// 체크아웃이 들어간다. 실측은 없다.
pub const ORCA_WORKTREE: Duration = Duration::from_secs(50);
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

/// 세션 안에서 불려도 새 worktree가 그 세션의 자식이 되지 않게 지운다(설계 7.2).
const CALLER_ENV: [&str; 5] =
    ["ORCA_TERMINAL_HANDLE", "ORCA_PANE_KEY", "ORCA_WORKSPACE_ID", "ORCA_WORKTREE_ID", "ORCA_CLI_CWD"];

/// 응답의 `result` 객체.
pub type OrcaResult = Map<String, Value>;

/// Lost 시간 초과, 깨진 응답. Orca가 그 명령을 실행했는지 모른다.
/// Refused Orca가 JSON으로 거절했다. 실행되지 않았다고 본다.
/// Missing orca 실행 파일이 없다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OrcaReply {
    Ok(OrcaResult),
    Refused { code: String },
    Lost,
    Missing,
}

impl OrcaReply {
    pub fn parse(stdout: &str) -> Self {
        let Ok(value) = js::parse_json(stdout) else {
            return Self::Lost;
        };
        let Value::Object(mut body) = value else {
            return Self::Lost;
        };
        match body.get("ok") {
            Some(Value::Bool(false)) => {
                let code = record(body.get("error")).and_then(|error| text(error.get("code")));
                Self::Refused { code: code.unwrap_or("unknown").to_owned() }
            }
            Some(Value::Bool(true)) => match body.remove("result") {
                Some(Value::Object(result)) => Self::Ok(result),
                _ => Self::Lost,
            },
            _ => Self::Lost,
        }
    }

    pub const fn result(&self) -> Option<&OrcaResult> {
        match self {
            Self::Ok(result) => Some(result),
            _ => None,
        }
    }
}

fn orca_env(base: impl IntoIterator<Item = (String, String)>) -> BTreeMap<String, String> {
    base.into_iter().filter(|(key, _)| !CALLER_ENV.contains(&key.as_str())).collect()
}

/// 터미널을 찾는 데 쓰는 run의 값.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LaunchedRun<'a> {
    pub id: &'a str,
    pub worktree_identity: Option<&'a str>,
    pub worktree_id: Option<&'a str>,
}

/// 셸을 거치지 않는다. 고정 cwd라서 호출한 곳의 worktree가 끼어들지 않는다.
///
/// 쓰지 않는 것: --focus, --pr, orca agent launch, worktree create --agent, orca terminal send.
#[derive(Clone)]
pub struct OrcaClient {
    runner: Arc<dyn ProcessRunner>,
    bin: Option<String>,
    cwd: PathBuf,
    env: BTreeMap<String, String>,
}

impl OrcaClient {
    /// `bin`이 None이면 모든 호출이 Missing이다. 환경은 지금 프로세스의 값에서 호출자 값을 뺀 것이다.
    pub fn new(runner: Arc<dyn ProcessRunner>, bin: Option<String>, cwd: impl Into<PathBuf>) -> Self {
        // ProcessRequest의 환경이 문자열이다. UTF-8이 아닌 값은 Orca에 넘어가지 않는다.
        let base =
            std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)));
        Self { runner, bin, cwd: cwd.into(), env: orca_env(base) }
    }

    /// 환경의 바탕을 바꾼다. 호출자 값은 여기서도 빠진다.
    #[must_use]
    pub fn with_env(mut self, base: BTreeMap<String, String>) -> Self {
        self.env = orca_env(base);
        self
    }

    fn call(&self, args: &[&str], timeout: Duration) -> OrcaReply {
        let Some(bin) = &self.bin else {
            return OrcaReply::Missing;
        };
        let request = ProcessRequest::new(bin.clone(), args.iter().copied(), timeout)
            .cwd(self.cwd.clone())
            .env(self.env.clone())
            .max_output(MAX_OUTPUT);
        let output = self.runner.run(&request);
        match output.end {
            ProcessEnd::Missing => OrcaReply::Missing,
            ProcessEnd::TimedOut | ProcessEnd::Cancelled => OrcaReply::Lost,
            _ => OrcaReply::parse(&output.stdout),
        }
    }

    pub(crate) fn reference_find(&self, url: &str) -> OrcaReply {
        self.call(&["reference", "find", url, "--json"], ORCA_READ)
    }

    /// 대상 저장소와 기준 브랜치는 그 URL의 플레이북 대상(프로필)에서 온다. 대상이 없거나 Orca 저장소를 모르면 부르지 않는다.
    pub(crate) fn worktree_create(&self, name: &str, url: &str) -> OrcaReply {
        let Some(target) = kairos_core::profile::current().playbooks.target_for_url(url) else {
            return OrcaReply::Refused { code: "no_playbook_target".to_owned() };
        };
        let Some(orca_repo) = target.orca_repo.as_deref() else {
            return OrcaReply::Refused { code: "orca_repo_unresolved".to_owned() };
        };
        let repo = format!("id:{orca_repo}");
        self.call(
            &[
                "worktree",
                "create",
                "--repo",
                &repo,
                "--name",
                name,
                "--no-parent",
                "--setup",
                "skip",
                "--base-branch",
                &target.base_branch,
                "--reference",
                url,
                "--json",
            ],
            ORCA_WORKTREE,
        )
    }

    pub(crate) fn worktree_show(&self, selector: &str) -> OrcaReply {
        self.call(&["worktree", "show", "--worktree", selector, "--json"], ORCA_READ)
    }

    pub(crate) fn worktree_ps(&self) -> OrcaReply {
        self.call(&["worktree", "ps", "--json"], ORCA_READ)
    }

    /// terminal create는 재시도 ID가 없어 어떤 경우에도 다시 부르지 않는다. 응답을 잃으면 제목과 ps로 찾는다.
    /// 수락은 사용자가 세션 열기를 누른 순간이라 --focus로 세션 창을 보여 준다. 자동 경로는 이 호출에 닿지 않는다.
    pub(crate) fn terminal_create(&self, selector: &str, title: &str, command: &str) -> OrcaReply {
        let args =
            ["terminal", "create", "--worktree", selector, "--title", title, "--command", command, "--focus", "--json"];
        self.call(&args, ORCA_TERMINAL)
    }

    pub(crate) fn terminal_list(&self, selector: &str) -> OrcaReply {
        self.call(&["terminal", "list", "--worktree", selector, "--json"], ORCA_READ)
    }

    fn terminal_layouts(&self, selector: &str) -> OrcaReply {
        self.call(&["terminal", "list", "--worktree", selector, "--include-visual-layouts", "--json"], ORCA_READ)
    }

    pub(crate) fn terminal_switch(&self, handle: &str) -> OrcaReply {
        self.call(&["terminal", "switch", "--terminal", handle, "--json"], ORCA_READ)
    }

    /// 제목과 ps로 터미널을 찾는다. 새 worktree의 에이전트는 이 세션뿐이다.
    pub(crate) fn find_launched(&self, run: LaunchedRun<'_>, known: Option<&PsSnapshot>) -> Option<OrcaTerminal> {
        let selector = run.worktree_identity?;
        let listed = self.terminal_layouts(selector);
        let title = terminal_title(run.id);
        if let Some(found) = listed.result().and_then(|result| OrcaTerminal::find_by_title(result, &title)) {
            return Some(found);
        }
        let fetched;
        let ps = match known {
            Some(ps) => Some(ps),
            None => {
                fetched = self.worktree_ps().result().map(PsSnapshot::read);
                fetched.as_ref()
            }
        };
        let pane_key = ps?.panes_of.get(run.worktree_id?)?.first()?;
        Some(OrcaTerminal { handle: None, pane_key: Some(pane_key.clone()) })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn 호출자_값만_환경에서_뺀다() {
        let base = CALLER_ENV.iter().map(|key| ((*key).to_owned(), "합성".to_owned()));
        let env = orca_env(base.chain([("PATH".to_owned(), "/usr/bin".to_owned())]));
        assert_eq!(env, BTreeMap::from([("PATH".to_owned(), "/usr/bin".to_owned())]));
    }

    #[test]
    fn 응답_해석은_네_가지다() {
        assert_eq!(OrcaReply::parse(r#"{"ok":true,"result":{"a":1}}"#).result().unwrap()["a"], json!(1));
        assert_eq!(
            OrcaReply::parse(r#"{"ok":false,"error":{"code":"selector_not_found"}}"#),
            OrcaReply::Refused { code: "selector_not_found".into() }
        );
        for body in [r#"{"ok":false}"#, r#"{"ok":false,"error":{"code":""}}"#, r#"{"ok":false,"error":"x"}"#] {
            assert_eq!(OrcaReply::parse(body), OrcaReply::Refused { code: "unknown".into() }, "{body}");
        }
        for body in
            ["", "{", "null", "[]", "1", r#"{"ok":true}"#, r#"{"ok":true,"result":[]}"#, r#"{"ok":1,"result":{}}"#]
        {
            assert_eq!(OrcaReply::parse(body), OrcaReply::Lost, "{body}");
        }
    }

    /// JS 문자열은 짝 없는 서로게이트를 그대로 쥐지만 Rust는 U+FFFD로 읽는다. 어느 쪽이든 응답을 잃지 않는다.
    #[test]
    fn 짝_없는_서로게이트가_있어도_응답을_받는다() {
        assert_eq!(
            OrcaReply::parse(r#"{"ok":true,"result":{"t":"x\ud83d"}}"#).result().unwrap()["t"],
            json!("x\u{FFFD}")
        );
    }
}
