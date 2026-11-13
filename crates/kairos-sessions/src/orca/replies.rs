//! Orca 응답 읽기. 타입 판정은 원본의 `typeof`, `Array.isArray` 분기와 같다.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::client::OrcaResult;

/// JS의 `\w`는 ASCII다.
static HANDLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_:.-]*$").expect("상수 정규식"));
/// Orca 저장소 ID. 설정 검사(`kairos-config`의 `orca_repo`)와 같은 모양이다.
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").expect("상수 정규식")
});

pub(super) fn record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

fn list(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

pub(super) fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|text| !text.is_empty())
}

/// `/[\u0000-\u001f\u007f-\u009f]/`.
fn has_control(value: &str) -> bool {
    value.chars().any(|c| matches!(c, '\u{0}'..='\u{1f}' | '\u{7f}'..='\u{9f}'))
}

fn handle_of(value: Option<&Value>) -> Option<String> {
    text(value).filter(|handle| HANDLE.is_match(handle)).map(str::to_owned)
}

fn pane_of(tab_id: Option<&Value>, leaf_id: Option<&Value>) -> Option<String> {
    Some(format!("{}:{}", text(tab_id)?, text(leaf_id)?))
}

fn get<'a>(row: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    row?.get(key)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrcaWorktree {
    pub id: String,
    /// --worktree에 넘기는 값.
    pub selector: String,
    pub path: Option<String>,
    pub head: Option<String>,
}

impl OrcaWorktree {
    pub fn read(result: &OrcaResult) -> Option<Self> {
        let worktree = record(result.get("worktree"))?;
        let id = text(worktree.get("id")).filter(|id| !has_control(id))?;
        let key = text(get(record(worktree.get("identity")), "key")).filter(|key| !has_control(key));
        Some(Self {
            id: id.to_owned(),
            selector: key.map_or_else(|| format!("id:{id}"), |key| format!("identity:{key}")),
            path: text(worktree.get("path")).map(str::to_owned),
            head: text(worktree.get("head")).map(str::to_owned),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrcaTerminal {
    pub handle: Option<String>,
    pub pane_key: Option<String>,
}

fn layout_tabs<'a>(node: Option<&'a Value>, tabs: &mut Vec<&'a Map<String, Value>>) {
    let Some(row) = record(node) else {
        return;
    };
    if row.get("type").and_then(Value::as_str) == Some("split") {
        layout_tabs(row.get("first"), tabs);
        layout_tabs(row.get("second"), tabs);
        return;
    }
    tabs.extend(list(row.get("tabs")).iter().filter_map(Value::as_object));
}

fn first_pane(node: Option<&Value>) -> Option<&Map<String, Value>> {
    let row = record(node)?;
    if row.get("type").and_then(Value::as_str) == Some("pane-split") {
        first_pane(row.get("first")).or_else(|| first_pane(row.get("second")))
    } else {
        Some(row)
    }
}

/// `a ?? b`. 키가 없거나 null이면 뒤의 값을 쓴다.
fn present<'a>(first: Option<&'a Value>, second: Option<&'a Value>) -> Option<&'a Value> {
    first.filter(|value| !value.is_null()).or(second)
}

impl OrcaTerminal {
    pub fn read_created(result: &OrcaResult) -> Option<Self> {
        let terminal = record(result.get("terminal"));
        let handle = handle_of(get(terminal, "handle"))?;
        Some(Self { handle: Some(handle), pane_key: text(get(terminal, "paneKey")).map(str::to_owned) })
    }

    /// terminal create --title은 탭 제목이고 visualLayouts에 나온다.
    /// terminals[].title은 실행 중인 프로그램이 바꾸므로 대조에 쓰지 않는다.
    pub fn find_by_title(result: &OrcaResult, title: &str) -> Option<Self> {
        for layout in list(result.get("visualLayouts")) {
            let mut tabs = Vec::new();
            layout_tabs(get(layout.as_object(), "root"), &mut tabs);
            for tab in tabs {
                if tab.get("title").and_then(Value::as_str) != Some(title) {
                    continue;
                }
                let pane = first_pane(tab.get("panes"));
                if let Some(handle) = handle_of(get(pane, "handle")) {
                    let tab_id = present(get(pane, "tabId"), tab.get("tabId"));
                    return Some(Self { handle: Some(handle), pane_key: pane_of(tab_id, get(pane, "leafId")) });
                }
            }
        }
        None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrcaLiveAgent {
    pub worktree_id: String,
    pub pane_key: String,
    pub handle: Option<String>,
}

impl OrcaLiveAgent {
    /// reference find의 살아 있는 에이전트. 기록으로 연결된 것만 채택한다.
    /// Orca 도움말이 '작업공간에 있다는 것만으로는 소유가 아니다'라고 적고 있다. 무관한 에이전트를 추적하면 시작이 세션을 열지 않게 된다.
    /// paneKey가 없으면 추적할 수 없어 뺀다. 실제 응답의 agents 모양은 아직 실측하지 못했다.
    pub fn find(result: &OrcaResult) -> Option<Self> {
        for found in list(result.get("matches")) {
            let row = found.as_object();
            let worktree_id = text(get(record(get(row, "workspace")), "id")).filter(|id| !has_control(id));
            let Some(worktree_id) = worktree_id else {
                continue;
            };
            for agent in list(get(row, "agents")) {
                let agent = agent.as_object();
                let live = get(agent, "liveness").and_then(Value::as_str) == Some("live");
                let linked = get(agent, "linked") == Some(&Value::Bool(true));
                if let (true, true, Some(pane_key)) = (live, linked, text(get(agent, "paneKey"))) {
                    return Some(Self {
                        worktree_id: worktree_id.to_owned(),
                        pane_key: pane_key.to_owned(),
                        handle: handle_of(get(agent, "terminal")),
                    });
                }
            }
        }
        None
    }
}

/// 같은 이름으로 만든 작업공간. worktree create 응답을 잃었을 때 쓴다.
pub fn find_workspace_named(result: &OrcaResult, name: &str) -> Option<String> {
    list(result.get("matches")).iter().find_map(|found| {
        let workspace = record(get(found.as_object(), "workspace"));
        let id = text(get(workspace, "id")).filter(|id| !has_control(id))?;
        (get(workspace, "name").and_then(Value::as_str) == Some(name)).then(|| id.to_owned())
    })
}

/// `repo list`에서 `owner/name` 저장소의 Orca ID를 고른다. 원격 주소(`github.com/owner/name`)가 같은 저장소만 본다.
/// 같은 원격의 클론이 여럿이면 폴더 이름이 저장소 이름과 같은 하나를 고른다. 그래도 하나가 아니면 고르지 않는다.
/// ID는 `--repo id:<ID>` 인자로 들어가므로 소문자 UUID만 받는다.
pub fn find_repo_for(result: &OrcaResult, workspace_repo: &str) -> Option<String> {
    let remote = format!("github.com/{workspace_repo}");
    let name = workspace_repo.rsplit('/').next().unwrap_or(workspace_repo);
    let matches: Vec<(String, Option<&str>)> = list(result.get("repos"))
        .iter()
        .filter_map(|repo| {
            let repo = repo.as_object()?;
            let key = text(get(record(repo.get("gitRemoteIdentity")), "canonicalKey"))?;
            if !key.eq_ignore_ascii_case(&remote) {
                return None;
            }
            let id = text(repo.get("id"))?.to_ascii_lowercase();
            if !UUID.is_match(&id) {
                return None;
            }
            let folder = text(repo.get("path")).and_then(|path| path.trim_end_matches('/').rsplit('/').next());
            Some((id, folder))
        })
        .collect();
    if let [(id, _)] = matches.as_slice() {
        return Some(id.clone());
    }
    let named: Vec<&String> = matches
        .iter()
        .filter(|(_, folder)| folder.is_some_and(|folder| folder.eq_ignore_ascii_case(name)))
        .map(|(id, _)| id)
        .collect();
    match named.as_slice() {
        [id] => Some((*id).clone()),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PsSnapshot {
    /// paneKey별 에이전트 상태.
    pub agents: HashMap<String, String>,
    /// worktree ID별 paneKey.
    pub panes_of: HashMap<String, Vec<String>>,
    pub working: usize,
    /// 잘린 응답에서는 없다는 사실을 믿을 수 없다.
    pub truncated: bool,
}

impl PsSnapshot {
    pub fn read(result: &OrcaResult) -> Self {
        let mut agents = HashMap::new();
        let mut panes_of: HashMap<String, Vec<String>> = HashMap::new();
        for worktree in list(result.get("worktrees")) {
            let row = worktree.as_object();
            let id = text(get(row, "worktreeId")).or_else(|| text(get(row, "id")));
            for agent in list(get(row, "agents")) {
                let agent = agent.as_object();
                let (Some(pane_key), Some(state)) = (text(get(agent, "paneKey")), text(get(agent, "state"))) else {
                    continue;
                };
                agents.insert(pane_key.to_owned(), state.to_owned());
                if let Some(id) = id {
                    panes_of.entry(id.to_owned()).or_default().push(pane_key.to_owned());
                }
            }
        }
        let working = agents.values().filter(|state| *state == "working").count();
        Self { agents, panes_of, working, truncated: result.get("truncated") == Some(&Value::Bool(true)) }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OrcaPane {
    pub handle: String,
    pub connected: bool,
}

impl OrcaPane {
    /// paneKey는 tabId:leafId다. handle은 Orca가 다시 뜨면 바뀌므로 이동할 때마다 다시 찾는다.
    pub fn find(result: &OrcaResult, pane_key: &str) -> Option<Self> {
        list(result.get("terminals")).iter().find_map(|terminal| {
            let row = terminal.as_object();
            if pane_of(get(row, "tabId"), get(row, "leafId")).as_deref() != Some(pane_key) {
                return None;
            }
            let handle = handle_of(get(row, "handle"))?;
            Some(Self { handle, connected: get(row, "connected") == Some(&Value::Bool(true)) })
        })
    }
}

pub fn focus_navigated(result: &OrcaResult) -> bool {
    get(record(result.get("focus")), "navigated") != Some(&Value::Bool(false))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const ONE: &str = "0000c0de-0000-4000-8000-000000000001";
    const TWO: &str = "0000c0de-0000-4000-8000-000000000002";
    const THREE: &str = "0000c0de-0000-4000-8000-000000000003";

    fn repo(id: &str, path: &str, key: Option<&str>) -> Value {
        let identity = key.map(|key| json!({ "canonicalKey": key, "remoteName": "origin" }));
        json!({ "id": id, "path": path, "kind": "git", "gitRemoteIdentity": identity })
    }

    fn repos(rows: Vec<Value>) -> OrcaResult {
        let Value::Object(result) = json!({ "repos": rows }) else { unreachable!() };
        result
    }

    #[test]
    fn 원격_주소가_같은_저장소_하나를_고른다() {
        let result = repos(vec![
            repo(ONE, "/synthetic/other", Some("github.com/contoso/other")),
            repo(&TWO.to_ascii_uppercase(), "/synthetic/work", Some("github.com/Contoso/Atlas")),
            repo(THREE, "/synthetic/plain", None),
        ]);
        assert_eq!(find_repo_for(&result, "contoso/atlas").as_deref(), Some(TWO));
    }

    #[test]
    fn 같은_원격의_클론이_여럿이면_폴더_이름이_같은_하나를_고른다() {
        let result = repos(vec![
            repo(ONE, "/synthetic/atlas-2", Some("github.com/contoso/atlas")),
            repo(TWO, "/synthetic/atlas/", Some("github.com/contoso/atlas")),
            repo(THREE, "/synthetic/atlas-3", Some("github.com/contoso/atlas")),
        ]);
        assert_eq!(find_repo_for(&result, "contoso/atlas").as_deref(), Some(TWO));
    }

    #[test]
    fn 하나로_고르지_못하면_없다() {
        let none = repos(vec![repo(ONE, "/synthetic/atlas", Some("github.com/contoso/atlas-specs"))]);
        assert_eq!(find_repo_for(&none, "contoso/atlas"), None);
        let ambiguous = repos(vec![
            repo(ONE, "/synthetic/atlas-2", Some("github.com/contoso/atlas")),
            repo(TWO, "/synthetic/atlas-3", Some("github.com/contoso/atlas")),
        ]);
        assert_eq!(find_repo_for(&ambiguous, "contoso/atlas"), None);
        let twins = repos(vec![
            repo(ONE, "/synthetic/a/atlas", Some("github.com/contoso/atlas")),
            repo(TWO, "/synthetic/b/atlas", Some("github.com/contoso/atlas")),
        ]);
        assert_eq!(find_repo_for(&twins, "contoso/atlas"), None);
        assert_eq!(find_repo_for(&repos(vec![]), "contoso/atlas"), None);
    }

    #[test]
    fn 인자에_넣을_수_없는_id는_받지_않는다() {
        for id in ["id:--json", "0000c0de 0000", "", "../x"] {
            let result = repos(vec![repo(id, "/synthetic/atlas", Some("github.com/contoso/atlas"))]);
            assert_eq!(find_repo_for(&result, "contoso/atlas"), None, "{id}");
        }
    }
}
