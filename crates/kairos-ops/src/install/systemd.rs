//! `systemctl --user` 호출. 종료 코드만 쓴다. 코드 없이 끝나면 1이다.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kairos_core::process::{ProcessRequest, ProcessRunner};

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT: usize = 1024 * 1024;
const CANDIDATES: [&str; 2] = ["/usr/bin/systemctl", "/bin/systemctl"];
/// `systemctl is-active`가 돌고 있지 않은 유닛에 내는 코드.
pub(super) const INACTIVE: i32 = 3;

/// systemd 사용자 세션이 없을 때의 안내. WSL은 systemd를 켜야 사용자 timer가 돈다.
pub(super) const NO_USER_SESSION: &str = "systemd 사용자 세션에 연결하지 못했습니다. WSL이면 /etc/wsl.conf의 [boot]에 systemd=true를 넣고 wsl --shutdown 뒤 다시 여세요";

pub(super) struct Systemctl<'a> {
    runner: &'a Arc<dyn ProcessRunner>,
    bin: &'static str,
}

impl<'a> Systemctl<'a> {
    pub fn new(runner: &'a Arc<dyn ProcessRunner>) -> Self {
        let bin = CANDIDATES.into_iter().find(|path| Path::new(path).is_file()).unwrap_or(CANDIDATES[0]);
        Self { runner, bin }
    }

    pub fn run(&self, args: &[&str]) -> i32 {
        let args = std::iter::once("--user").chain(args.iter().copied());
        let request = ProcessRequest::new(self.bin, args, TIMEOUT).max_output(MAX_OUTPUT);
        self.runner.run(&request).exit_code().unwrap_or(1)
    }
}
