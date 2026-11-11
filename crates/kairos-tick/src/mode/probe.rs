use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kairos_core::process::{ProcessRequest, ProcessRunner};
use kairos_core::{Instant, KairosResult, js};
use serde_json::Value;

use super::ClaudeMode;
use crate::halt::js_number_text;
use crate::lock::mtime_ms;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_MAX_OUTPUT: usize = 1024 * 1024;

/// claude 실행 파일에 묻는 두 가지. 시험은 가짜를 넘긴다.
pub trait ClaudeProbe: Send + Sync {
    /// claude 버전과 바이너리 mtime을 합친 값. 바뀌면 계약 시험이다.
    fn fingerprint(&self) -> KairosResult<Option<String>>;
    /// claude auth status의 loginExpiresAt. 실패하면 None이다.
    fn login_expiry(&self) -> KairosResult<Option<String>>;
}

#[derive(Clone)]
pub struct SystemClaudeProbe {
    runner: Arc<dyn ProcessRunner>,
    bin: Option<String>,
    env: BTreeMap<String, String>,
}

impl SystemClaudeProbe {
    /// `env`는 프로세스 환경이다. `ClaudeMode::env`로 거른 값만 자식에게 간다.
    pub fn new(runner: Arc<dyn ProcessRunner>, bin: Option<String>, env: BTreeMap<String, String>) -> Self {
        Self { runner, bin: bin.filter(|bin| !bin.is_empty()), env }
    }

    fn text(&self, bin: &str, args: &[&str]) -> Option<String> {
        let env = ClaudeMode::env(&self.env, BTreeMap::new());
        let request =
            ProcessRequest::new(bin, args.iter().copied(), PROBE_TIMEOUT).max_output(PROBE_MAX_OUTPUT).env(env);
        let output = self.runner.run(&request);
        output.success().then_some(output.stdout)
    }
}

impl ClaudeProbe for SystemClaudeProbe {
    fn fingerprint(&self) -> KairosResult<Option<String>> {
        let Some(bin) = &self.bin else {
            return Ok(None);
        };
        let version = self.text(bin, &["--version"]).unwrap_or_default();
        Ok(mtime_ms(Path::new(bin)).ok().map(|mtime| format!("{bin}|{}|{}", js_number_text(mtime), js::trim(&version))))
    }

    fn login_expiry(&self) -> KairosResult<Option<String>> {
        let Some(bin) = &self.bin else {
            return Ok(None);
        };
        let Some(text) = self.text(bin, &["auth", "status"]) else {
            return Ok(None);
        };
        let parsed = js::parse_json(&text).ok();
        let value = parsed.as_ref().and_then(|body| body.get("loginExpiresAt")).and_then(Value::as_str);
        Ok(value.filter(|value| Instant::parse(value).is_some()).map(str::to_owned))
    }
}
