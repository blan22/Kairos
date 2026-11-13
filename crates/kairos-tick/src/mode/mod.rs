//! 수집 모드 실행. `claude -p`를 한 번 띄워 훅이 호출을 끝내면 모델 호출 없이 drop으로 끝난다(스파이크 결과).

mod judge;
mod probe;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};

pub use judge::{DROP_RESULT, RunVerdict, judge_claude_run};
pub use probe::{ClaudeProbe, SystemClaudeProbe};

const BASE_ENV: [&str; 5] = ["PATH", "HOME", "USER", "TMPDIR", "LANG"];

/// 훅 모듈이 적재되지 않았을 때 모델 요청이 향하는 곳. 아무도 받을 수 없는 목적지여야 한다.
/// 224.0.0.0은 IPv4 멀티캐스트라 TCP 연결을 커널이 connect에서 바로 거절한다(EAFNOSUPPORT, 측정).
/// SYN도 나가지 않고 DNS도 거치지 않는다. 같은 포트에서 듣는 로컬 리스너도 받지 못한다(측정).
/// 루프백 포트는 macOS에서 권한 없이 바인드되므로 다른 로컬 사용자가 요청과 자격증명을 받을 수 있다.
/// 포트는 의미가 없다. 클라이언트가 포트를 어떻게 읽어도 커널 단계에서 막힌다.
const MODEL_SINK_URL: &str = "http://224.0.0.0:9";

/// 모델 요청을 연결할 수 없는 곳으로 보내면 훅이 새도 요청이 나가지 않고 비용도 생기지 않는다. 다시 시도도 하지 않는다.
/// MCP는 이 경로를 쓰지 않는다. 저장된 claude.ai 로그인은 MCP 커넥터가 쓰므로 가짜 키로 덮지 않는다.
/// 환경의 마지막에 넣는다. 어떤 호출자 값도 이 둘을 바꾸지 못한다.
const GUARD_ENV: [(&str, &str); 2] = [("ANTHROPIC_BASE_URL", MODEL_SINK_URL), ("CLAUDE_CODE_MAX_RETRIES", "0")];

/// `claude -p --output-format json`의 결과 중 판정에 쓰는 필드.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeRun {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub missing: bool,
    pub stdout: String,
}

impl From<ProcessOutput> for ClaudeRun {
    fn from(output: ProcessOutput) -> Self {
        let exit_code = output.exit_code();
        // 출력 상한을 넘겨 죽인 것도 원본의 killed다. 그쪽 execFile이 시간 초과와 같이 표시한다.
        let timed_out = matches!(output.end, ProcessEnd::TimedOut | ProcessEnd::OutputTooLarge | ProcessEnd::Cancelled);
        Self { exit_code, timed_out, missing: output.end == ProcessEnd::Missing, stdout: output.stdout }
    }
}

/// 수집기 실행의 인자와 환경.
#[derive(Clone, Copy, Debug)]
pub struct ClaudeMode;

impl ClaudeMode {
    /// 스파이크 권장 조합이다. 플러그인 경로만 바뀐다.
    ///
    /// `--disable-slash-commands`는 사용자 스킬과 명령을 읽지 않게 한다. 수집은 둘 다 쓰지 않는다.
    /// 이 플래그가 없으면 claude가 `~/.claude/skills`의 링크를 따라가 데스크탑 같은 보호 폴더를 읽는다.
    /// 틱은 launchd가 띄우므로 macOS는 그 접근을 kairos의 것으로 보고 3분마다 폴더 접근 허용을 묻는다(측정).
    /// 플래그를 넣어도 MCP 서버, 플러그인, 훅은 그대로 뜬다(측정).
    pub fn args(plugin_dir: &str) -> Vec<String> {
        [
            "-p",
            "도구를 쓰지 말고 바로 종료하라. 답은 ok 한 단어.",
            "--tools",
            "",
            "--disable-slash-commands",
            "--permission-mode",
            "dontAsk",
            "--permission-prompts",
            "none",
            "--max-budget-usd",
            "0.02",
            "--no-session-persistence",
            "--output-format",
            "json",
            "--setting-sources",
            "project",
            "--settings",
            r#"{"enabledPlugins":{"slack@claude-plugins-official":true}}"#,
            "--plugin-dir",
            plugin_dir,
        ]
        .map(str::to_owned)
        .into()
    }

    /// `extra`는 호출자가 더하는 값이다. 모델 요청을 막는 `GUARD_ENV`는 그 뒤에 넣어 `extra`가 덮지 못한다.
    pub fn env(base: &BTreeMap<String, String>, extra: BTreeMap<String, String>) -> BTreeMap<String, String> {
        // SSH_AUTH_SOCK은 넘기지 않는다. kairos는 ssh를 쓰지 않고, 무인 틱이 사용자의 agent 키를 쓰면 안 된다.
        let mut env: BTreeMap<String, String> =
            BASE_ENV.iter().filter_map(|key| Some(((*key).to_owned(), base.get(*key)?.clone()))).collect();
        // 기본값이면 약 187KB 응답이 오류가 된다(측정). 이 값이면 파일로 저장되고 ingest가 되찾는다.
        env.insert("MAX_MCP_OUTPUT_TOKENS".to_owned(), "500000".to_owned());
        env.extend(extra);
        env.extend(GUARD_ENV.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())));
        env
    }

    /// 외부 프로세스는 셸 없이 argv로만 띄운다.
    /// 열린 stdin을 두면 claude -p가 입력을 3초 기다린다(측정). 틱 상한 20초에서 그만큼 잃는다. 러너가 바로 닫는다.
    pub fn launch(
        runner: &dyn ProcessRunner,
        bin: &str,
        args: Vec<String>,
        env: BTreeMap<String, String>,
        cwd: &Path,
        timeout: Duration,
    ) -> ClaudeRun {
        runner.run(&ProcessRequest::new(bin, args, timeout).env(env).cwd(cwd)).into()
    }
}
