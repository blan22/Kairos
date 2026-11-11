//! 세션 잠금을 푼 설정. `kairos sessions on`이 그때의 설정 지문을 남긴다.
//! 세션 자격에 닿는 설정(조직, 신뢰 도메인, 본인 식별자, 플레이북 대상, 호스트, MCP로 연 동작)이 바뀌면 지문이 달라지고
//! 세션은 다시 잠긴다. 사용자가 차단 시험(S3, S4)을 다시 보고 켜야 열린다(설계 4.3).
//!
//! control.json의 형식은 그대로 두려고 따로 둔다. `sessionTestPassedAt`이 있어도 지문이 다르면 잠긴 것으로 읽는다.

use std::fs;

use kairos_core::home::write_file_atomic;
use kairos_core::{KairosHome, KairosResult};

use super::KairosControl;

pub const SESSION_APPROVAL_FILE: &str = "session-approval";

/// 지문의 길이. sha256 16진수다.
const FINGERPRINT_LEN: usize = 64;

#[derive(Clone, Copy, Debug)]
pub struct SessionApproval<'a> {
    home: &'a KairosHome,
}

/// 세션 잠금의 실제 상태.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionLock {
    Open,
    /// 켠 적이 없거나 끔.
    Locked,
    /// 켰지만 그 뒤 설정이 바뀜.
    ConfigChanged,
}

impl<'a> SessionApproval<'a> {
    pub const fn new(home: &'a KairosHome) -> Self {
        Self { home }
    }

    /// 지금 프로필의 지문.
    pub fn current() -> String {
        kairos_config::session_fingerprint(kairos_core::profile::current())
    }

    pub fn record(&self) -> KairosResult<()> {
        write_file_atomic(&self.home.file(SESSION_APPROVAL_FILE), &format!("{}\n", Self::current()))
    }

    pub fn clear(&self) -> KairosResult<()> {
        match fs::remove_file(self.home.file(SESSION_APPROVAL_FILE)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    /// 남긴 지문이 지금 설정과 같은가. 파일이 없거나 모양이 다르면 아니다.
    pub fn matches(&self) -> bool {
        let Ok(text) = fs::read_to_string(self.home.file(SESSION_APPROVAL_FILE)) else {
            return false;
        };
        let stored = text.trim();
        stored.len() == FINGERPRINT_LEN && stored == Self::current()
    }

    pub fn lock(&self, control: &KairosControl) -> SessionLock {
        match (control.sessions_allowed(), self.matches()) {
            (false, _) => SessionLock::Locked,
            (true, true) => SessionLock::Open,
            (true, false) => SessionLock::ConfigChanged,
        }
    }

    /// 판단에 쓰는 control. 지문이 다르면 세션을 끈 것으로 읽는다. 파일에는 쓰지 않는다.
    #[must_use]
    pub fn effective(&self, control: KairosControl) -> KairosControl {
        if self.lock(&control) == SessionLock::ConfigChanged { control.with_sessions(None) } else { control }
    }
}
