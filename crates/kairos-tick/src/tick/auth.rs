//! auth.json. 로그인 만료 시각을 시간당 한 번 확인해 둔다.

use std::fs;
use std::path::PathBuf;

use kairos_agenda::pretty_json;
use kairos_core::home::write_file_atomic;
use kairos_core::{Instant, KairosHome, KairosResult, js};
use serde_json::{Value, json};

use crate::mode::ClaudeProbe;

pub const AUTH_CHECK_INTERVAL_MS: i64 = 3_600_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthRecord {
    pub checked_at: Option<String>,
    pub login_expires_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AuthFile {
    path: PathBuf,
}

impl AuthFile {
    pub fn new(home: &KairosHome) -> Self {
        Self { path: home.file("auth.json") }
    }

    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    fn parsed(&self) -> Option<Value> {
        let bytes = fs::read(&self.path).ok()?;
        js::parse_json(&String::from_utf8_lossy(&bytes)).ok().filter(|value| !value.is_null())
    }

    /// 파일이 없거나 깨졌으면 None이다.
    pub fn read(&self) -> Option<AuthRecord> {
        let parsed = self.parsed()?;
        let text = |key: &str| parsed.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(AuthRecord { checked_at: text("checkedAt"), login_expires_at: text("loginExpiresAt") })
    }

    pub(super) fn refresh(&self, now: Instant, probe: &dyn ClaudeProbe) -> KairosResult<()> {
        let checked_at = self.read().and_then(|record| record.checked_at).filter(|at| !at.is_empty());
        // 해석할 수 없는 시각은 NaN이라 비교가 거짓이고 다시 확인한다.
        let recent = checked_at
            .and_then(|at| Instant::parse(&at))
            .is_some_and(|checked| now.ms_since(checked) < AUTH_CHECK_INTERVAL_MS);
        if recent {
            return Ok(());
        }
        let login_expires_at = probe.login_expiry()?;
        let body = json!({ "checkedAt": now, "loginExpiresAt": login_expires_at });
        write_file_atomic(&self.path, &pretty_json(&body)?)
    }
}
