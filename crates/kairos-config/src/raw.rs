//! 파일 모양. 모르는 키는 거부한다. 오타가 조용히 기본값이 되면 신뢰 범위가 달라진다.
//! 여기 있는 값은 검증 전이다. `values`를 거쳐야 `Profile`에 들어간다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 세 파일 모두 맨 위에 `schema = 1`을 둔다.
pub(crate) const SCHEMA: i64 = 1;

/// 본인 식별자. `connections.toml`이 쓰고, `config.toml`이 손으로 덮을 수 있다.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawIdentity {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPlaybookTarget {
    pub owner: String,
    pub repos: Vec<String>,
    pub workspace_repo: String,
    pub base_branch: String,
}

/// `org.toml`. 조직 관리자가 배포한다.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOrg {
    pub schema: Option<i64>,
    pub name: Option<String>,
    #[serde(default)]
    pub github: RawGithub,
    #[serde(default)]
    pub trust: RawTrust,
    #[serde(default)]
    pub calendar: RawCalendar,
    #[serde(default)]
    pub notion: RawNotion,
    #[serde(default)]
    pub playbooks: Vec<RawPlaybookTarget>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGithub {
    #[serde(default)]
    pub orgs: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawTrust {
    #[serde(default)]
    pub email_domains: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCalendar {
    pub deploy: Option<String>,
    pub leave: Option<String>,
    pub holidays: Option<String>,
    pub deploy_tag: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawNotion {
    pub workspace: Option<String>,
}

/// `config.toml`. 사용자가 쓴다.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawUser {
    pub schema: Option<i64>,
    pub identity: Option<RawIdentity>,
    #[serde(default)]
    pub schedule: RawSchedule,
    #[serde(default)]
    pub sources: RawSources,
    /// 조직 프로필에 더하는 개인 대상.
    #[serde(default)]
    pub playbooks: Vec<RawPlaybookTarget>,
    #[serde(default)]
    pub hosts: RawHosts,
    #[serde(default)]
    pub mcp: RawMcpServe,
    #[serde(default)]
    pub sessions: RawSessions,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSessions {
    /// `orca` 또는 `git`.
    pub host: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGitHost {
    pub worktree_root: Option<String>,
    /// `owner/name` → 로컬 클론의 절대 경로.
    #[serde(default)]
    pub repos: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSchedule {
    pub timezone: Option<String>,
    /// 근무 요일(`mon`~`sun`). 이 요일 밖은 쉬는 날이다.
    pub days: Option<Vec<String>>,
    pub work: Option<[String; 2]>,
    /// 두 시각이거나 빈 목록이다. 빈 목록은 점심 보류를 끈다.
    pub lunch: Option<Vec<String>>,
    pub bundles: Option<Vec<String>>,
    pub collect: Option<[String; 2]>,
}

/// `config.toml`과 `connections.toml`의 `[sources]`. 두 파일의 끈 소스를 합친다.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSources {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled: Vec<String>,
}

impl RawSources {
    pub(crate) fn is_empty(&self) -> bool {
        self.disabled.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawHosts {
    /// 켠 호스트. 순서가 reveal을 시도하는 순서다.
    pub enabled: Option<Vec<String>>,
    #[serde(default)]
    pub orca: RawOrcaHost,
    #[serde(default)]
    pub git: RawGitHost,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOrcaHost {
    /// `owner/name` → Orca 저장소 ID. Orca 저장소 ID는 컴퓨터마다 다르다.
    #[serde(default)]
    pub repos: BTreeMap<String, String>,
}

/// `kairos mcp serve`가 에이전트에 여는 동작.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMcpServe {
    #[serde(default)]
    pub allow_actions: Vec<String>,
}

/// `connections.toml`. `kairos connect`, `kairos sources`, `kairos setup`만 쓴다.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConnections {
    pub schema: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<RawIdentity>,
    /// `kairos sources on|off`가 쓴다.
    #[serde(default, skip_serializing_if = "RawSources::is_empty")]
    pub sources: RawSources,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub mcp: BTreeMap<String, RawBinding>,
}

/// 소스 하나의 MCP 바인딩. 커넥터가 정한 동작 이름을 실제 서버와 도구 이름에 잇는다.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawBinding {
    pub server: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<String>,
}
