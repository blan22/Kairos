//! `connections.toml` 고치기. `kairos connect`, `kairos sources`, `kairos setup`만 쓴다. 고친 뒤 세 파일을 합쳐 검증이 통과해야 저장한다.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;

use kairos_core::platform::CreateMode;

use crate::raw::{RawBinding, RawConnections, RawIdentity, RawUser, SCHEMA};
use crate::{CONNECTIONS_FILE, ConfigError, ORG_FILE, USER_FILE, files, from_text};

/// 본인 식별자 칸.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityField {
    Github,
    Google,
    Slack,
    Name,
}

/// 읽어 둔 연동 결과. 고친 뒤 `save`한다.
#[derive(Clone, Debug)]
pub struct Connections {
    raw: RawConnections,
}

impl Connections {
    pub fn read(root: &Path) -> Result<Self, ConfigError> {
        let raw = match files::read_owned(root, CONNECTIONS_FILE)? {
            Some(text) => from_text::<RawConnections>(CONNECTIONS_FILE, &text)?,
            None => RawConnections { schema: Some(SCHEMA), ..RawConnections::default() },
        };
        Ok(Self { raw })
    }

    pub fn set_identity(&mut self, field: IdentityField, value: &str) {
        let identity = self.raw.identity.get_or_insert_with(RawIdentity::default);
        let slot = match field {
            IdentityField::Github => &mut identity.github,
            IdentityField::Google => &mut identity.google,
            IdentityField::Slack => &mut identity.slack,
            IdentityField::Name => &mut identity.name,
        };
        *slot = Some(value.to_owned());
    }

    pub fn set_binding(
        &mut self,
        source: &str,
        server: &str,
        tools: BTreeMap<String, String>,
        verified_at: Option<String>,
    ) {
        self.raw.mcp.insert(source.to_owned(), RawBinding { server: server.to_owned(), tools, verified_at });
    }

    pub fn remove_binding(&mut self, source: &str) -> bool {
        self.raw.mcp.remove(source).is_some()
    }

    /// 이 파일에서 끈 소스.
    pub fn disabled_sources(&self) -> &[String] {
        &self.raw.sources.disabled
    }

    /// 소스를 켜거나 끈다. 바뀌었으면 true. 이름은 저장할 때 검증한다.
    pub fn set_source_enabled(&mut self, source: &str, enabled: bool) -> bool {
        let disabled = &mut self.raw.sources.disabled;
        let present = disabled.iter().any(|name| name == source);
        match (enabled, present) {
            (true, true) => disabled.retain(|name| name != source),
            (false, false) => disabled.push(source.to_owned()),
            _ => return false,
        }
        true
    }

    /// 세 파일을 합쳐 검증한 뒤 0600으로 바꿔 쓴다. 검증에 실패하면 쓰지 않는다.
    pub fn save(&self, root: &Path) -> Result<(), ConfigError> {
        let text =
            toml::to_string(&self.raw).map_err(|_| ConfigError::file(CONNECTIONS_FILE, "config_write_failed"))?;
        let content = format!("# kairos connect, sources, setup이 쓰는 파일이다. 손으로 고치지 않는다.\n{text}");
        let org = files::read_owned(root, ORG_FILE)?;
        let user = files::read_owned(root, USER_FILE)?;
        crate::load_from_texts(org.as_deref(), user.as_deref(), Some(&content))?;
        let failed = |_| ConfigError::file(CONNECTIONS_FILE, "config_write_failed");
        let temporary = root.join(format!(".{CONNECTIONS_FILE}.{}.tmp", std::process::id()));
        let mut file =
            fs::OpenOptions::new().write(true).create_new(true).create_mode(0o600).open(&temporary).map_err(failed)?;
        let written = file.write_all(content.as_bytes()).and_then(|()| file.sync_all());
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(ConfigError::file(CONNECTIONS_FILE, "config_write_failed"));
        }
        fs::rename(&temporary, root.join(CONNECTIONS_FILE)).map_err(failed)
    }
}

/// `config.toml`에서 손으로 끈 소스. `kairos sources on`으로 되살리지 못한다.
pub fn user_disabled_sources(root: &Path) -> Result<Vec<String>, ConfigError> {
    let Some(text) = files::read_owned(root, USER_FILE)? else {
        return Ok(Vec::new());
    };
    Ok(from_text::<RawUser>(USER_FILE, &text)?.sources.disabled)
}
