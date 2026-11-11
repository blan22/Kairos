use std::sync::Arc;
use std::time::Duration;

use kairos_core::process::{ProcessRequest, ProcessRunner};

const TIMEOUT: Duration = Duration::from_secs(20);
const ID_TIMEOUT: Duration = Duration::from_secs(2);
/// execFile의 기본 maxBuffer.
const MAX_OUTPUT: usize = 1024 * 1024;
/// `process.getuid`가 없을 때 원본이 쓰던 값.
const FALLBACK_UID: u32 = 501;

/// launchctl 호출. 종료 코드만 쓴다. 코드 없이 끝나면 1이다.
pub(super) struct Launchctl<'a> {
    runner: &'a Arc<dyn ProcessRunner>,
}

impl<'a> Launchctl<'a> {
    pub const fn new(runner: &'a Arc<dyn ProcessRunner>) -> Self {
        Self { runner }
    }

    pub fn run(&self, args: &[&str]) -> i32 {
        let request = ProcessRequest::new("/bin/launchctl", args.iter().copied(), TIMEOUT).max_output(MAX_OUTPUT);
        self.runner.run(&request).exit_code().unwrap_or(1)
    }

    /// `gui/<uid>`. `getuid`는 `unsafe`라서 id에게 묻는다.
    pub fn domain(&self) -> String {
        let output = self.runner.run(&ProcessRequest::new("/usr/bin/id", ["-u"], ID_TIMEOUT));
        let uid = if output.success() { output.stdout.trim().parse().ok() } else { None };
        format!("gui/{}", uid.unwrap_or(FALLBACK_UID))
    }
}
