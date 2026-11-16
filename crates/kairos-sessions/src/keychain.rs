//! 세션 토큰의 키체인 저장. 토큰은 인자에 넣지 않는다(같은 컴퓨터의 다른 사용자도 ps로 인자를 읽는다).
//! macOS는 `security`다. 쓰기는 `security -i`에 명령을 stdin으로 넘기고, 읽기는 `find-generic-password -w`의 표준 출력으로 받는다.
//! Linux는 Secret Service의 `secret-tool`이다. 쓰기는 비밀 값을 stdin으로 넘긴다.
//! Windows는 세션을 열지 않으므로 키체인이 없다. 읽기는 `Unavailable`, 지우기는 지울 것이 없다.

use std::sync::Arc;
use std::time::Duration;

use kairos_core::process::{ProcessEnd, ProcessRequest, ProcessRunner};
use kairos_core::{KairosError, KairosResult};

use crate::profile::SessionToken;

pub const KEYCHAIN_SERVICE: &str = "kairos";
pub const KEYCHAIN_ACCOUNT: &str = "session-gh-token";
/// 시험은 `KAIROS_SECURITY`로 가짜를 준다.
pub const SECURITY_BIN: &str = "/usr/bin/security";
/// Linux의 Secret Service 도구. 시험은 같은 `KAIROS_SECURITY`로 가짜를 준다.
pub const SECRET_TOOL_BIN: &str = "/usr/bin/secret-tool";
const SECRET_LABEL: &str = "kairos-session-token";
const SECRET_ATTRIBUTES: [&str; 4] = ["service", KEYCHAIN_SERVICE, "account", KEYCHAIN_ACCOUNT];
const TIMEOUT: Duration = Duration::from_secs(10);
/// `security`가 항목을 찾지 못했을 때의 종료 코드.
const NOT_FOUND: i32 = 44;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    Security,
    SecretTool,
    None,
}

/// 키체인의 세션 토큰.
#[derive(Clone)]
pub struct Keychain {
    runner: Arc<dyn ProcessRunner>,
    bin: String,
    backend: Backend,
}

/// 읽은 결과. 키체인을 쓸 수 없으면 파일로 넘어간다.
#[derive(Debug)]
pub enum KeychainRead {
    Found(SessionToken),
    Missing,
    /// 실행 파일이 없거나 실패했다.
    Unavailable,
}

impl Keychain {
    /// macOS `security`.
    pub fn new(runner: Arc<dyn ProcessRunner>, bin: impl Into<String>) -> Self {
        Self { runner, bin: bin.into(), backend: Backend::Security }
    }

    /// Linux `secret-tool`.
    pub fn secret_tool(runner: Arc<dyn ProcessRunner>, bin: impl Into<String>) -> Self {
        Self { runner, bin: bin.into(), backend: Backend::SecretTool }
    }

    /// 키체인이 없는 OS(Windows).
    pub fn unavailable(runner: Arc<dyn ProcessRunner>) -> Self {
        Self { runner, bin: String::new(), backend: Backend::None }
    }

    fn secret_tool_request(&self, verb: &str, extra: &[&str]) -> ProcessRequest {
        let args = std::iter::once(verb).chain(extra.iter().copied()).chain(SECRET_ATTRIBUTES);
        ProcessRequest::new(self.bin.clone(), args, TIMEOUT).max_output(4096)
    }

    /// `secret-tool lookup`. 없으면 출력 없이 1로 끝난다. 키링을 쓸 수 없으면 1과 함께 까닭을 stderr에 쓴다.
    fn secret_tool_read(&self) -> KairosResult<KeychainRead> {
        let output = self.runner.run(&self.secret_tool_request("lookup", &[]));
        match output.end {
            ProcessEnd::Exited(0) => SessionToken::parse(&output.stdout).map(KeychainRead::Found),
            ProcessEnd::Exited(1) if output.stdout.is_empty() && output.stderr.trim().is_empty() => {
                Ok(KeychainRead::Missing)
            }
            _ => Ok(KeychainRead::Unavailable),
        }
    }

    /// 있으면 형식을 검사한다. 형식이 틀린 값은 오류다(파일로 넘어가지 않는다).
    pub fn read(&self) -> KairosResult<KeychainRead> {
        match self.backend {
            Backend::Security => {}
            Backend::SecretTool => return self.secret_tool_read(),
            Backend::None => return Ok(KeychainRead::Unavailable),
        }
        let request = ProcessRequest::new(
            self.bin.clone(),
            ["find-generic-password", "-s", KEYCHAIN_SERVICE, "-a", KEYCHAIN_ACCOUNT, "-w"],
            TIMEOUT,
        )
        .max_output(4096);
        let output = self.runner.run(&request);
        match output.end {
            ProcessEnd::Exited(0) => SessionToken::parse(&output.stdout).map(KeychainRead::Found),
            ProcessEnd::Exited(NOT_FOUND) => Ok(KeychainRead::Missing),
            _ => Ok(KeychainRead::Unavailable),
        }
    }

    /// 토큰을 지운다. 지웠으면 true, 원래 없었으면 false다. 데이터까지 지우는 제거가 쓴다.
    pub fn delete(&self) -> KairosResult<bool> {
        match self.backend {
            Backend::Security => {}
            Backend::None => return Ok(false),
            // clear는 없어도 0으로 끝난다. 있었는지는 먼저 읽어서 안다.
            Backend::SecretTool => {
                return match self.secret_tool_read() {
                    Ok(KeychainRead::Missing) => Ok(false),
                    Ok(KeychainRead::Unavailable) => Err(KairosError::new("keychain_delete_failed")),
                    _ => match self.runner.run(&self.secret_tool_request("clear", &[])).end {
                        ProcessEnd::Exited(0) => Ok(true),
                        _ => Err(KairosError::new("keychain_delete_failed")),
                    },
                };
            }
        }
        let request = ProcessRequest::new(
            self.bin.clone(),
            ["delete-generic-password", "-s", KEYCHAIN_SERVICE, "-a", KEYCHAIN_ACCOUNT],
            TIMEOUT,
        )
        .max_output(4096);
        match self.runner.run(&request).end {
            ProcessEnd::Exited(0) => Ok(true),
            ProcessEnd::Exited(NOT_FOUND) => Ok(false),
            _ => Err(KairosError::new("keychain_delete_failed")),
        }
    }

    /// 토큰을 쓰거나 바꾼다. 형식은 호출자가 `SessionToken::parse`로 이미 봤다.
    pub fn write(&self, token: &SessionToken) -> KairosResult<()> {
        match self.backend {
            Backend::Security => {}
            Backend::None => return Err(KairosError::new("keychain_write_failed")),
            // 줄바꿈 없이 넘긴다. secret-tool은 stdin을 끝까지 그대로 저장한다.
            Backend::SecretTool => {
                let request = self.secret_tool_request("store", &[&format!("--label={SECRET_LABEL}")]);
                return match self.runner.run(&request.stdin(token.expose().to_owned())).end {
                    ProcessEnd::Exited(0) => Ok(()),
                    _ => Err(KairosError::new("keychain_write_failed")),
                };
            }
        }
        let command = format!(
            "add-generic-password -U -s {KEYCHAIN_SERVICE} -a {KEYCHAIN_ACCOUNT} -l kairos-session-token -w {}\n",
            token.expose()
        );
        let request = ProcessRequest::new(self.bin.clone(), ["-i"], TIMEOUT).stdin(command).max_output(4096);
        let output = self.runner.run(&request);
        if matches!(output.end, ProcessEnd::Exited(0))
            && !output.stderr.contains("rror")
            && !output.stdout.contains("rror")
        {
            Ok(())
        } else {
            Err(KairosError::new("keychain_write_failed"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kairos_core::process::ProcessOutput;

    use super::*;

    struct Fake {
        end: ProcessEnd,
        stdout: &'static str,
        seen: Mutex<Vec<(Vec<String>, Option<String>)>>,
    }

    impl ProcessRunner for Fake {
        fn run(&self, request: &ProcessRequest) -> ProcessOutput {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((request.args.clone(), request.stdin.clone()));
            ProcessOutput { end: self.end, stdout: self.stdout.to_owned(), stderr: String::new() }
        }
    }

    fn keychain(end: ProcessEnd, stdout: &'static str) -> (Keychain, Arc<Fake>) {
        let fake = Arc::new(Fake { end, stdout, seen: Mutex::new(Vec::new()) });
        (Keychain::new(fake.clone(), "security"), fake)
    }

    #[test]
    fn 읽기는_찾음_없음_못씀을_가른다() {
        let token = "github_pat_SYNTHETIC0123456789abcdef\n";
        assert!(matches!(keychain(ProcessEnd::Exited(0), token).0.read(), Ok(KeychainRead::Found(_))));
        assert!(matches!(keychain(ProcessEnd::Exited(44), "").0.read(), Ok(KeychainRead::Missing)));
        assert!(matches!(keychain(ProcessEnd::Missing, "").0.read(), Ok(KeychainRead::Unavailable)));
        assert!(keychain(ProcessEnd::Exited(0), "ghp_classic\n").0.read().is_err());
    }

    fn secret_tool(end: ProcessEnd, stdout: &'static str) -> (Keychain, Arc<Fake>) {
        let fake = Arc::new(Fake { end, stdout, seen: Mutex::new(Vec::new()) });
        (Keychain::secret_tool(fake.clone(), "secret-tool"), fake)
    }

    #[test]
    fn secret_tool은_속성으로_찾고_비밀은_stdin으로만_넘긴다() {
        let token = "github_pat_SYNTHETIC0123456789abcdef";
        let (found, fake) = secret_tool(ProcessEnd::Exited(0), token);
        assert!(matches!(found.read(), Ok(KeychainRead::Found(_))));
        assert_eq!(
            fake.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner)[0].0,
            ["lookup", "service", "kairos", "account", "session-gh-token"]
        );
        assert!(matches!(secret_tool(ProcessEnd::Exited(1), "").0.read(), Ok(KeychainRead::Missing)));
        assert!(matches!(secret_tool(ProcessEnd::Missing, "").0.read(), Ok(KeychainRead::Unavailable)));

        let (keychain, fake) = secret_tool(ProcessEnd::Exited(0), "");
        let parsed = SessionToken::parse(token).unwrap_or_else(|_| unreachable!());
        assert!(keychain.write(&parsed).is_ok());
        let seen = fake.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(
            seen[0].0,
            ["store", "--label=kairos-session-token", "service", "kairos", "account", "session-gh-token"]
        );
        assert_eq!(seen[0].1.as_deref(), Some(token));
        assert!(!seen[0].0.iter().any(|arg| arg.contains("github_pat")));
    }

    #[test]
    fn 키체인이_없는_os는_읽지_못하고_지울_것도_없다() {
        let keychain = Keychain::unavailable(Arc::new(Fake {
            end: ProcessEnd::Missing,
            stdout: "",
            seen: Mutex::new(Vec::new()),
        }));
        assert!(matches!(keychain.read(), Ok(KeychainRead::Unavailable)));
        assert!(matches!(keychain.delete(), Ok(false)));
    }

    #[test]
    fn 쓰기는_토큰을_인자에_넣지_않는다() {
        let (keychain, fake) = keychain(ProcessEnd::Exited(0), "");
        let token = SessionToken::parse("github_pat_SYNTHETIC0123456789abcdef").unwrap_or_else(|_| unreachable!());
        assert!(keychain.write(&token).is_ok());
        let seen = fake.seen.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(seen[0].0, ["-i"]);
        assert!(seen[0].1.as_deref().is_some_and(|stdin| stdin.contains("github_pat_SYNTHETIC")));
    }
}
