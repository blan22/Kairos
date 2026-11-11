//! 외부 프로세스 실행. 셸을 거치지 않고 argv로만 띄운다.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant as MonotonicInstant};

use crate::ports::CancelToken;

const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// 실행 요청. `env`가 None이면 부모 환경을 물려받고, Some이면 그 값만 준다.
#[derive(Clone)]
pub struct ProcessRequest {
    pub program: String,
    pub args: Vec<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub cwd: Option<PathBuf>,
    pub stdin: Option<String>,
    pub timeout: Duration,
    pub max_output: usize,
    pub cancel: Option<CancelToken>,
}

/// env에 `GH_TOKEN`이, stdin에 외부 응답이 들어 있다. 값을 찍지 않는다.
impl std::fmt::Debug for ProcessRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessRequest")
            .field("program", &self.program)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl ProcessRequest {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
        timeout: Duration,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: None,
            cwd: None,
            stdin: None,
            timeout,
            max_output: 8 * 1024 * 1024,
            cancel: None,
        }
    }

    #[must_use]
    pub fn env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = Some(env);
        self
    }

    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    #[must_use]
    pub fn stdin(mut self, stdin: impl Into<String>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }

    #[must_use]
    pub fn max_output(mut self, bytes: usize) -> Self {
        self.max_output = bytes;
        self
    }

    #[must_use]
    pub fn cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = Some(cancel);
        self
    }
}

/// 프로세스가 끝난 모양.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessEnd {
    /// 종료 코드로 끝났다.
    Exited(i32),
    /// 신호로 죽었다.
    Signaled,
    /// 시간 제한에 걸려 죽였다.
    TimedOut,
    /// 취소 신호로 죽였다.
    Cancelled,
    /// 출력 상한을 넘어 죽였다.
    OutputTooLarge,
    /// 실행 파일이 없다.
    Missing,
    /// 그 밖의 이유로 띄우지 못했다.
    SpawnFailed,
}

#[derive(Clone, Debug)]
pub struct ProcessOutput {
    pub end: ProcessEnd,
    pub stdout: String,
    pub stderr: String,
}

impl ProcessOutput {
    pub fn success(&self) -> bool {
        self.end == ProcessEnd::Exited(0)
    }

    pub fn exit_code(&self) -> Option<i32> {
        match self.end {
            ProcessEnd::Exited(code) => Some(code),
            _ => None,
        }
    }

    /// 시험용 생성 도우미.
    pub fn exited(code: i32, stdout: impl Into<String>) -> Self {
        Self { end: ProcessEnd::Exited(code), stdout: stdout.into(), stderr: String::new() }
    }

    pub fn ended(end: ProcessEnd) -> Self {
        Self { end, stdout: String::new(), stderr: String::new() }
    }
}

pub trait ProcessRunner: Send + Sync {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcessRunner;

/// 파이프를 끝까지 비운다. 상한을 넘으면 표시만 남기고 계속 읽어 자식이 쓰기에서 멈추지 않게 한다.
fn drain(mut pipe: impl Read + Send + 'static, limit: usize, overflow: Arc<AtomicBool>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0_u8; 16 * 1024];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => return kept,
                Ok(read) if kept.len() + read > limit => overflow.store(true, Ordering::SeqCst),
                Ok(read) => kept.extend_from_slice(&chunk[..read]),
            }
        }
    })
}

impl ProcessRunner for SystemProcessRunner {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        let mut command = Command::new(&request.program);
        command.args(&request.args).stdout(Stdio::piped()).stderr(Stdio::piped());
        // 열린 stdin을 두면 claude -p가 입력을 3초 기다린다(측정). 넘길 내용이 없으면 바로 닫는다.
        command.stdin(if request.stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        if let Some(env) = &request.env {
            command.env_clear().envs(env);
        }
        if let Some(cwd) = &request.cwd {
            command.current_dir(cwd);
        }
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return ProcessOutput::ended(ProcessEnd::Missing);
            }
            Err(_) => return ProcessOutput::ended(ProcessEnd::SpawnFailed),
        };
        if let (Some(mut pipe), Some(input)) = (child.stdin.take(), request.stdin.clone()) {
            // 큰 입력에서 자식이 stdout을 먼저 채우면 서로 막힌다. 쓰기는 따로 돈다.
            thread::spawn(move || {
                let _ = pipe.write_all(input.as_bytes());
            });
        }
        let overflow = Arc::new(AtomicBool::new(false));
        let stdout = child.stdout.take().map(|pipe| drain(pipe, request.max_output, overflow.clone()));
        let stderr = child.stderr.take().map(|pipe| drain(pipe, request.max_output, overflow.clone()));

        let started = MonotonicInstant::now();
        let end = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.code().map_or(ProcessEnd::Signaled, ProcessEnd::Exited),
                Ok(None) => {}
                Err(_) => break ProcessEnd::SpawnFailed,
            }
            let stop = if overflow.load(Ordering::SeqCst) {
                Some(ProcessEnd::OutputTooLarge)
            } else if started.elapsed() >= request.timeout {
                Some(ProcessEnd::TimedOut)
            } else if request.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
                Some(ProcessEnd::Cancelled)
            } else {
                None
            };
            if let Some(end) = stop {
                let _ = child.kill();
                let _ = child.wait();
                break end;
            }
            thread::sleep(POLL_INTERVAL);
        };
        // 자식이 끝나도 손자가 파이프를 물려받아 쥐고 있으면 읽기가 끝나지 않는다. 시간 제한은 파이프가 닫힐 때까지 살아 있어야 한다.
        // Node의 execFile도 stdio가 닫힐 때까지 타이머를 둔다. 상한 없이 기다리면 틱이 잠금을 쥔 채 영원히 멈춘다.
        let end = if matches!(end, ProcessEnd::Exited(_) | ProcessEnd::Signaled) {
            let finished =
                |handle: &Option<thread::JoinHandle<Vec<u8>>>| handle.as_ref().is_none_or(|pipe| pipe.is_finished());
            loop {
                if finished(&stdout) && finished(&stderr) {
                    break end;
                }
                if started.elapsed() >= request.timeout {
                    break ProcessEnd::TimedOut;
                }
                if request.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
                    break ProcessEnd::Cancelled;
                }
                thread::sleep(POLL_INTERVAL);
            }
        } else {
            end
        };
        // 읽기가 끝난 뒤에 본다. 끝나기 직전에 상한을 넘은 출력을 정상으로 넘기지 않는다.
        let end = if overflow.load(Ordering::SeqCst) { ProcessEnd::OutputTooLarge } else { end };
        let text = |handle: Option<thread::JoinHandle<Vec<u8>>>| match (handle, end) {
            (Some(handle), ProcessEnd::Exited(_) | ProcessEnd::Signaled) => {
                handle.join().map(|bytes| String::from_utf8_lossy(&bytes).into_owned()).unwrap_or_default()
            }
            // 죽인 경우에는 기다리지 않는다. 읽기 스레드는 프로세스와 함께 끝난다.
            _ => String::new(),
        };
        ProcessOutput { end, stdout: text(stdout), stderr: text(stderr) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(request: ProcessRequest) -> ProcessOutput {
        SystemProcessRunner.run(&request)
    }

    #[test]
    fn 출력과_종료_코드를_돌려준다() {
        let out = run(ProcessRequest::new("/bin/sh", ["-c", "echo 가; echo 나 >&2; exit 3"], Duration::from_secs(5)));
        assert_eq!(out.end, ProcessEnd::Exited(3));
        assert_eq!(out.stdout, "가\n");
        assert_eq!(out.stderr, "나\n");
    }

    #[test]
    fn stdin을_넘긴다() {
        let out = run(ProcessRequest::new("/bin/cat", Vec::<String>::new(), Duration::from_secs(5)).stdin("입력"));
        assert_eq!(out.stdout, "입력");
    }

    #[test]
    fn 시간_제한에_걸리면_죽인다() {
        let started = MonotonicInstant::now();
        let out = run(ProcessRequest::new("/bin/sleep", ["5"], Duration::from_millis(50)));
        assert_eq!(out.end, ProcessEnd::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn 취소하면_죽인다() {
        let cancel = CancelToken::new();
        cancel.cancel();
        let out = run(ProcessRequest::new("/bin/sleep", ["5"], Duration::from_secs(5)).cancel(cancel));
        assert_eq!(out.end, ProcessEnd::Cancelled);
    }

    #[test]
    fn 없는_파일은_missing이다() {
        let out = run(ProcessRequest::new("/nonexistent/kairos", Vec::<String>::new(), Duration::from_secs(1)));
        assert_eq!(out.end, ProcessEnd::Missing);
    }

    #[test]
    fn 출력_상한을_넘으면_죽인다() {
        let out =
            run(ProcessRequest::new("/usr/bin/yes", Vec::<String>::new(), Duration::from_secs(5)).max_output(1024));
        assert_eq!(out.end, ProcessEnd::OutputTooLarge);
    }

    #[test]
    fn 손자가_파이프를_쥐어도_시간_제한_안에_돌아온다() {
        let started = MonotonicInstant::now();
        let out = run(ProcessRequest::new("/bin/sh", ["-c", "(sleep 30 &) ; exit 0"], Duration::from_millis(300)));
        assert_eq!(out.end, ProcessEnd::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn 손자가_파이프를_놓으면_정상_종료다() {
        let script = "(sleep 30 >/dev/null 2>&1 &) ; echo 끝";
        let out = run(ProcessRequest::new("/bin/sh", ["-c", script], Duration::from_secs(5)));
        assert_eq!(out.end, ProcessEnd::Exited(0));
        assert_eq!(out.stdout, "끝\n");
    }

    #[test]
    fn 디버그_출력에_환경과_입력을_찍지_않는다() {
        let env = BTreeMap::from([("GH_TOKEN".to_owned(), "synthetic-value".to_owned())]);
        let request =
            ProcessRequest::new("/bin/true", Vec::<String>::new(), Duration::from_secs(1)).env(env).stdin("본문");
        let text = format!("{request:?}");
        assert!(!text.contains("synthetic-value") && !text.contains("본문"));
    }

    #[test]
    fn 환경을_지정하면_그_값만_준다() {
        let env = BTreeMap::from([("ONLY".to_owned(), "1".to_owned())]);
        let out = run(ProcessRequest::new("/usr/bin/env", Vec::<String>::new(), Duration::from_secs(5)).env(env));
        assert_eq!(out.stdout, "ONLY=1\n");
    }
}
