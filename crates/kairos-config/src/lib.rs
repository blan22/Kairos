//! 설정 파일을 읽고 검증해 `Profile`을 만든다.
//!
//! 파일은 모두 데이터 폴더(`KAIROS_HOME`)에 있다. 세션 거부 목록이 이미 그 폴더를 막고 있어 세션이 설정을 바꾸지 못한다.
//!
//! | 층 | 파일 | 쓰는 사람 |
//! |---|---|---|
//! | 기본값 | 코드 | 개발자 |
//! | 조직 프로필 | `org.toml` | 조직 관리자가 배포 |
//! | 사용자 설정 | `config.toml` | 사용자 |
//! | 연동 결과 | `connections.toml` | `kairos connect` |
//!
//! 뒤 층이 앞 층을 덮는다. 본인 식별자는 `connections.toml` 위에 `config.toml`의 `[identity]`가 덮는다.
//! 보안 불변식은 어느 층에도 칸이 없다. 설정으로 풀 수 없다.

mod connections;
mod files;
mod raw;
mod values;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use kairos_core::profile::{Hosts, McpBinding, PlaybookTarget, Profile, Schedule};
use kairos_core::{catalog, connector};
use sha2::{Digest, Sha256};

pub use connections::{Connections, IdentityField, user_disabled_sources};
use raw::{RawConnections, RawIdentity, RawOrg, RawPlaybookTarget, RawUser, SCHEMA};
use values::At;

pub const ORG_FILE: &str = "org.toml";
pub const USER_FILE: &str = "config.toml";
pub const CONNECTIONS_FILE: &str = "connections.toml";

/// 알려진 호스트.
pub const HOSTS: [&str; 3] = ["orca", "claude-code", "vscode"];
/// `kairos mcp serve`가 열 수 있는 상태 변경 동작. 수락(세션 시작)은 사용자가 고른 경우에만 연다.
pub const MCP_ACTIONS: [&str; 5] = ["later", "done", "keep", "add", "accept"];

/// 설정 오류. 값은 넣지 않는다(비밀이 섞였을 수 있다).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError {
    pub file: &'static str,
    pub key: Option<String>,
    pub code: &'static str,
}

impl ConfigError {
    pub(crate) const fn file(file: &'static str, code: &'static str) -> Self {
        Self { file, key: None, code }
    }

    pub(crate) fn key(file: &'static str, key: &str, code: &'static str) -> Self {
        Self { file, key: Some(key.to_owned()), code }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            Some(key) => write!(f, "{} {}: {}", self.file, key, self.code),
            None => write!(f, "{}: {}", self.file, self.code),
        }
    }
}

/// 값이 어느 층에서 왔는가.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layer {
    Default,
    Org,
    User,
    Connections,
}

impl Layer {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "기본값",
            Self::Org => ORG_FILE,
            Self::User => USER_FILE,
            Self::Connections => CONNECTIONS_FILE,
        }
    }
}

/// 읽은 결과. `origins`는 `config show --origin`이 쓴다.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub profile: Profile,
    pub origins: BTreeMap<&'static str, Layer>,
    /// 있던 파일.
    pub files: Vec<&'static str>,
    /// 실행을 막지 않는 알림.
    pub warnings: Vec<String>,
}

/// 데이터 폴더의 설정을 읽는다. 파일이 하나도 없으면 빈 프로필이다.
pub fn load(root: &Path) -> Result<Loaded, ConfigError> {
    let org = parse::<RawOrg>(root, ORG_FILE)?;
    let user = parse::<RawUser>(root, USER_FILE)?;
    let connections = parse::<RawConnections>(root, CONNECTIONS_FILE)?;
    let files = [(ORG_FILE, org.is_some()), (USER_FILE, user.is_some()), (CONNECTIONS_FILE, connections.is_some())]
        .into_iter()
        .filter_map(|(name, present)| present.then_some(name))
        .collect();
    let mut builder = Builder::default();
    builder.org(&org.unwrap_or_default())?;
    builder.connections(&connections.unwrap_or_default())?;
    builder.user(&user.unwrap_or_default())?;
    builder.finish(files)
}

/// 문자열에서 바로 읽는다. 시험과 `config validate --file`이 쓴다.
pub fn load_from_texts(
    org: Option<&str>,
    user: Option<&str>,
    connections: Option<&str>,
) -> Result<Loaded, ConfigError> {
    let org: Option<RawOrg> = org.map(|text| from_text(ORG_FILE, text)).transpose()?;
    let user: Option<RawUser> = user.map(|text| from_text(USER_FILE, text)).transpose()?;
    let connections: Option<RawConnections> = connections.map(|text| from_text(CONNECTIONS_FILE, text)).transpose()?;
    let mut builder = Builder::default();
    builder.org(&org.unwrap_or_default())?;
    builder.connections(&connections.unwrap_or_default())?;
    builder.user(&user.unwrap_or_default())?;
    builder.finish(Vec::new())
}

fn parse<T: serde::de::DeserializeOwned + HasSchema>(
    root: &Path,
    name: &'static str,
) -> Result<Option<T>, ConfigError> {
    files::read_owned(root, name)?.map(|text| from_text(name, &text)).transpose()
}

fn from_text<T: serde::de::DeserializeOwned + HasSchema>(name: &'static str, text: &str) -> Result<T, ConfigError> {
    // 오류 문구에는 값 조각이 들어 있어 내지 않는다. 줄 번호만 남긴다.
    let value: T = toml::from_str(text).map_err(|error| match error.span() {
        Some(span) => {
            let line = text[..span.start.min(text.len())].matches('\n').count() + 1;
            ConfigError::key(name, &format!("{line}번째 줄"), "config_parse_error")
        }
        None => ConfigError::file(name, "config_parse_error"),
    })?;
    match value.schema() {
        Some(SCHEMA) => Ok(value),
        Some(_) => Err(ConfigError::key(name, "schema", "config_schema_unsupported")),
        None => Err(ConfigError::key(name, "schema", "config_schema_missing")),
    }
}

trait HasSchema {
    fn schema(&self) -> Option<i64>;
}

impl HasSchema for RawOrg {
    fn schema(&self) -> Option<i64> {
        self.schema
    }
}

impl HasSchema for RawUser {
    fn schema(&self) -> Option<i64> {
        self.schema
    }
}

impl HasSchema for RawConnections {
    fn schema(&self) -> Option<i64> {
        self.schema
    }
}

#[derive(Default)]
struct Builder {
    profile: Profile,
    origins: BTreeMap<&'static str, Layer>,
    warnings: Vec<String>,
    orca_repos: BTreeMap<String, String>,
}

impl Builder {
    fn set(&mut self, key: &'static str, layer: Layer) {
        self.origins.insert(key, layer);
    }

    fn org(&mut self, raw: &RawOrg) -> Result<(), ConfigError> {
        const F: &str = ORG_FILE;
        self.profile = Profile::empty();
        if let Some(name) = &raw.name {
            values::label(At::new(F, "name"), name, 40)?;
        }
        let org = &mut self.profile.org;
        if !raw.github.orgs.is_empty() {
            org.github_orgs = raw
                .github
                .orgs
                .iter()
                .map(|value| values::github_name(At::new(F, "github.orgs"), value))
                .collect::<Result<_, _>>()?;
            values::unique(At::new(F, "github.orgs"), &org.github_orgs)?;
            self.origins.insert("org.github_orgs", Layer::Org);
        }
        if !raw.trust.email_domains.is_empty() {
            org.trusted_domains = raw
                .trust
                .email_domains
                .iter()
                .map(|value| values::domain(At::new(F, "trust.email_domains"), value))
                .collect::<Result<_, _>>()?;
            values::unique(At::new(F, "trust.email_domains"), &org.trusted_domains)?;
            self.origins.insert("org.trusted_domains", Layer::Org);
        }
        let calendar = &raw.calendar;
        let mut calendar_of = |value: &Option<String>, key: &'static str, origin: &'static str| {
            value.as_deref().map(|value| values::calendar_id(At::new(F, key), value)).transpose().inspect(|value| {
                if value.is_some() {
                    self.origins.insert(origin, Layer::Org);
                }
            })
        };
        let deploy = calendar_of(&calendar.deploy, "calendar.deploy", "org.deploy_calendar")?;
        let leave = calendar_of(&calendar.leave, "calendar.leave", "org.leave_calendar")?;
        let holidays = calendar_of(&calendar.holidays, "calendar.holidays", "org.holiday_calendar")?;
        let org = &mut self.profile.org;
        org.deploy_calendar = deploy;
        org.leave_calendar = leave;
        org.holiday_calendar = holidays;
        if let Some(tag) = &calendar.deploy_tag {
            org.deploy_tag = values::label(At::new(F, "calendar.deploy_tag"), tag, 20)?;
            self.origins.insert("org.deploy_tag", Layer::Org);
        }
        if let Some(workspace) = &raw.notion.workspace {
            self.profile.org.notion_workspace = Some(values::label(At::new(F, "notion.workspace"), workspace, 40)?);
            self.origins.insert("org.notion_workspace", Layer::Org);
        }
        let targets = playbook_targets(F, &raw.playbooks)?;
        if !targets.is_empty() {
            self.set("playbooks", Layer::Org);
        }
        self.profile.playbooks.targets = targets;
        Ok(())
    }

    fn identity(&mut self, file: &'static str, layer: Layer, raw: &RawIdentity) -> Result<(), ConfigError> {
        let identity = &mut self.profile.identity;
        if let Some(value) = &raw.github {
            identity.github_login = Some(values::github_name(At::new(file, "identity.github"), value)?);
            self.origins.insert("identity.github", layer);
        }
        if let Some(value) = &raw.google {
            identity.google_email = Some(values::email(At::new(file, "identity.google"), value)?);
            self.origins.insert("identity.google", layer);
        }
        if let Some(value) = &raw.slack {
            identity.slack_user = Some(values::slack_user(At::new(file, "identity.slack"), value)?);
            self.origins.insert("identity.slack", layer);
        }
        if let Some(value) = &raw.name {
            identity.display_name = Some(values::person_name(At::new(file, "identity.name"), value)?);
            self.origins.insert("identity.name", layer);
        }
        Ok(())
    }

    fn connections(&mut self, raw: &RawConnections) -> Result<(), ConfigError> {
        const F: &str = CONNECTIONS_FILE;
        if let Some(identity) = &raw.identity {
            self.identity(F, Layer::Connections, identity)?;
        }
        for (source, binding) in &raw.mcp {
            if !connector::BINDABLE_SOURCES.contains(&source.as_str()) {
                return Err(ConfigError::key(F, "mcp", "config_unknown_source"));
            }
            let server = values::label(At::new(F, "mcp.server"), &binding.server, 80)?;
            let mut tools = BTreeMap::new();
            for (capability, tool) in &binding.tools {
                // 커넥터가 정한 읽기 동작만 이을 수 있다. 쓰기 동사가 든 도구 이름은 받지 않는다.
                if connector::find(source, capability).is_none() {
                    return Err(ConfigError::key(F, "mcp.tools", "config_unknown_capability"));
                }
                let tool = values::tool_name(At::new(F, "mcp.tools"), tool)?;
                if connector::looks_like_write(&tool) {
                    return Err(ConfigError::key(F, "mcp.tools", "config_write_tool"));
                }
                tools.insert(capability.clone(), tool);
            }
            if let Some(at) = &binding.verified_at {
                values::no_secret(At::new(F, "mcp.verified_at"), at)?;
            }
            self.profile.bindings.insert(source.clone(), McpBinding { server, tools });
        }
        if !raw.mcp.is_empty() {
            self.set("bindings", Layer::Connections);
        }
        self.disabled_sources(F, Layer::Connections, &raw.sources.disabled)
    }

    /// 끈 소스. 앞 층에서 끈 소스에 더한다. 한 파일에서 끈 소스를 다른 파일이 되살리지 못한다.
    fn disabled_sources(&mut self, file: &'static str, layer: Layer, disabled: &[String]) -> Result<(), ConfigError> {
        if disabled.iter().any(|source| catalog::find(source).is_none()) {
            return Err(ConfigError::key(file, "sources.disabled", "config_unknown_source"));
        }
        values::unique(At::new(file, "sources.disabled"), disabled)?;
        for source in disabled {
            if !self.profile.disabled_sources.contains(source) {
                self.profile.disabled_sources.push(source.clone());
            }
        }
        if !disabled.is_empty() {
            self.set("sources.disabled", layer);
        }
        Ok(())
    }

    fn user(&mut self, raw: &RawUser) -> Result<(), ConfigError> {
        const F: &str = USER_FILE;
        if let Some(identity) = &raw.identity {
            self.identity(F, Layer::User, identity)?;
        }
        self.schedule(raw)?;
        self.disabled_sources(F, Layer::User, &raw.sources.disabled)?;
        let extra = playbook_targets(F, &raw.playbooks)?;
        if !extra.is_empty() {
            self.profile.playbooks.targets.extend(extra);
            self.set("playbooks", Layer::User);
        }
        if let Some(enabled) = &raw.hosts.enabled {
            if enabled.iter().any(|host| !HOSTS.contains(&host.as_str())) {
                return Err(ConfigError::key(F, "hosts.enabled", "config_unknown_host"));
            }
            values::unique(At::new(F, "hosts.enabled"), enabled)?;
            self.profile.hosts = Hosts { enabled: enabled.clone() };
            self.set("hosts.enabled", Layer::User);
        }
        for (repo, id) in &raw.hosts.orca.repos {
            let repo = values::full_repo(At::new(F, "hosts.orca.repos"), repo)?;
            let id = values::orca_repo(At::new(F, "hosts.orca.repos"), id)?;
            self.orca_repos.insert(repo, id);
        }
        match raw.sessions.host.as_deref() {
            None => {}
            Some("orca") => self.set("sessions.host", Layer::User),
            Some("git") => {
                self.profile.session_host = kairos_core::profile::SessionHost::Git;
                self.set("sessions.host", Layer::User);
            }
            Some(_) => return Err(ConfigError::key(F, "sessions.host", "config_unknown_host")),
        }
        if let Some(root) = &raw.hosts.git.worktree_root {
            self.profile.git.worktree_root = Some(values::absolute_path(At::new(F, "hosts.git.worktree_root"), root)?);
        }
        for (repo, path) in &raw.hosts.git.repos {
            let repo = values::full_repo(At::new(F, "hosts.git.repos"), repo)?;
            let path = values::absolute_path(At::new(F, "hosts.git.repos"), path)?;
            self.profile.git.repos.insert(repo, path);
        }
        for action in &raw.mcp.allow_actions {
            if !MCP_ACTIONS.contains(&action.as_str()) {
                return Err(ConfigError::key(F, "mcp.allow_actions", "config_unknown_action"));
            }
        }
        values::unique(At::new(F, "mcp.allow_actions"), &raw.mcp.allow_actions)?;
        if !raw.mcp.allow_actions.is_empty() {
            self.profile.mcp_actions.clone_from(&raw.mcp.allow_actions);
            self.set("mcp.allow_actions", Layer::User);
        }
        Ok(())
    }

    fn schedule(&mut self, raw: &RawUser) -> Result<(), ConfigError> {
        const F: &str = USER_FILE;
        let schedule = &raw.schedule;
        let mut next: Schedule = self.profile.schedule.clone();
        if let Some(zone) = &schedule.timezone {
            next.zone = values::zone(At::new(F, "schedule.timezone"), zone)?;
            self.set("schedule.timezone", Layer::User);
        }
        if let Some(days) = &schedule.days {
            next.days = values::days(At::new(F, "schedule.days"), days)?;
            self.set("schedule.days", Layer::User);
        }
        if let Some(work) = &schedule.work {
            next.work = values::range(At::new(F, "schedule.work"), work)?;
            self.set("schedule.work", Layer::User);
        }
        if let Some(lunch) = &schedule.lunch {
            next.lunch = values::optional_range(At::new(F, "schedule.lunch"), lunch)?;
            self.set("schedule.lunch", Layer::User);
        }
        if let Some(collect) = &schedule.collect {
            next.tick = values::range(At::new(F, "schedule.collect"), collect)?;
            self.set("schedule.collect", Layer::User);
        }
        if let Some(bundles) = &schedule.bundles {
            let minutes: Vec<u32> = bundles
                .iter()
                .map(|value| values::clock(At::new(F, "schedule.bundles"), value))
                .collect::<Result<_, _>>()?;
            if minutes.is_empty() || minutes.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(ConfigError::key(F, "schedule.bundles", "config_bundles_not_ascending"));
            }
            next.bundles = minutes;
            self.set("schedule.bundles", Layer::User);
        }
        if next.work.start < next.tick.start || next.work.end > next.tick.end {
            self.warnings
                .push("schedule.work가 schedule.collect 밖으로 나가 그 시간에는 새 일을 모으지 않습니다".to_owned());
        }
        self.profile.schedule = next;
        Ok(())
    }

    fn finish(mut self, files: Vec<&'static str>) -> Result<Loaded, ConfigError> {
        for target in &mut self.profile.playbooks.targets {
            target.orca_repo = self.orca_repos.get(&target.workspace_repo).cloned();
        }
        // 한 저장소는 대상 하나에만 든다. 둘이면 어느 기준 브랜치로 일할지 모른다.
        let covered: Vec<String> = self
            .profile
            .playbooks
            .targets
            .iter()
            .flat_map(|target| target.repos.iter().map(move |repo| format!("{}/{repo}", target.owner)))
            .collect();
        values::unique(At::new(USER_FILE, "playbooks"), &covered)?;
        let profile = &self.profile;
        if profile.org.github_orgs.is_empty() && profile.identity.github_login.is_some() {
            self.warnings.push("github.orgs가 없어 GitHub 소스가 돌지 않습니다(org.toml)".to_owned());
        }
        for target in &profile.playbooks.targets {
            if !profile.org.is_org(&target.owner) {
                self.warnings.push(format!("플레이북 대상 {}은 github.orgs 밖이라 수집되지 않습니다", target.owner));
            }
        }
        Ok(Loaded { profile: self.profile, origins: self.origins, files, warnings: self.warnings })
    }
}

fn playbook_targets(file: &'static str, raw: &[RawPlaybookTarget]) -> Result<Vec<PlaybookTarget>, ConfigError> {
    raw.iter()
        .map(|target| {
            let owner = values::github_name(At::new(file, "playbooks.owner"), &target.owner)?;
            if target.repos.is_empty() {
                return Err(ConfigError::key(file, "playbooks.repos", "config_empty_list"));
            }
            let repos: Vec<String> = target
                .repos
                .iter()
                .map(|repo| values::repo_name(At::new(file, "playbooks.repos"), repo))
                .collect::<Result<_, _>>()?;
            values::unique(At::new(file, "playbooks.repos"), &repos)?;
            Ok(PlaybookTarget {
                owner,
                repos,
                workspace_repo: values::full_repo(At::new(file, "playbooks.workspace_repo"), &target.workspace_repo)?,
                orca_repo: None,
                base_branch: values::base_branch(At::new(file, "playbooks.base_branch"), &target.base_branch)?,
            })
        })
        .collect()
}

/// 세션 자격에 닿는 값의 지문. 이 값이 바뀌면 세션 잠금이 다시 걸린다(설계 4.3).
/// 조직, 신뢰 도메인, 신뢰 캘린더, 본인 식별자, 플레이북 대상, 켠 호스트, MCP로 연 동작, 세션 바닥값 판번호가 든다.
pub fn session_fingerprint(profile: &Profile) -> String {
    /// 세션 프로필(거부 목록, 샌드박스)을 바꾸면 올린다. 올리면 모든 사용자의 세션 잠금이 다시 걸린다.
    const SESSION_FLOOR_VERSION: u32 = 2;
    let mut lines = vec![format!("floor={SESSION_FLOOR_VERSION}")];
    let identity = &profile.identity;
    lines.push(format!(
        "identity={:?}|{:?}|{:?}|{:?}",
        identity.github_login, identity.google_email, identity.slack_user, identity.display_name
    ));
    let org = &profile.org;
    lines.push(format!("orgs={:?}", org.github_orgs));
    lines.push(format!("domains={:?}", org.trusted_domains));
    lines.push(format!("calendars={:?}|{:?}", org.deploy_calendar, org.leave_calendar));
    for target in &profile.playbooks.targets {
        lines.push(format!(
            "target={}|{:?}|{}|{:?}|{}",
            target.owner, target.repos, target.workspace_repo, target.orca_repo, target.base_branch
        ));
    }
    lines.push(format!("hosts={:?}", profile.hosts.enabled));
    lines.push(format!("mcp={:?}", profile.mcp_actions));
    lines.push(format!("session_host={}", profile.session_host.as_str()));
    lines.push(format!("git={:?}|{:?}", profile.git.repos, profile.git.worktree_root));
    let digest = Sha256::digest(lines.join("\n").as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
