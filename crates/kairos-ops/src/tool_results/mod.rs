//! 약 50KB를 넘는 MCP 결과는 하네스가 표식 글자로 바꾸고 원문을 아래 파일로 남긴다(측정).
//!   ~/.claude/projects/<cwd 슬러그>/<세션 UUID>/tool-results/<파일>
//! 파일은 0644이고 메일 제목과 Slack 본문이 그대로 들어 있다. --no-session-persistence로 막히지 않는다.
//! 그래서 ingest가 원문을 되찾은 뒤 지우고, collect가 끝날 때 남은 것을 다시 지운다.
//!
//! 여기는 보안 경계다. 표식의 경로는 외부 응답에서 왔다고 보고 계산한 폴더 밖을 읽지도 지우지도 않는다.

mod reader;
mod sweep;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use kairos_core::model::McpResponse;
use kairos_core::{KairosHome, KairosResult, js};
use kairos_source_mcp::shared::{ModeFailure, text_of};
use regex::Regex;
use serde_json::{Value, json};

use crate::posix;
use reader::ResultFile;
pub use sweep::ToolResultsSweep;

pub(crate) const RESULTS_DIR: &str = "tool-results";
/// 측정 최대는 187KB다. 저장되지 않는 응답이므로 메모리 상한만 둔다.
pub const MAX_RECOVERED_BYTES: u64 = 4 * 1024 * 1024;

/// 응답 전체가 이 형식일 때만 표식이다. 응답은 JSON이라 외부 문자열이 맨 앞에 올 수 없고, 미리보기는 표식 안쪽에 있다.
static PERSISTED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^<persisted-output>\nOutput too large \([0-9.,]+ ?[KMG]?B\)\. Full output saved to: (/[^\n]+)\n(?s:.)*\n</persisted-output>[{}]*$",
        js::WS
    ))
    .expect("상수 정규식")
});
/// MAX_MCP_OUTPUT_TOKENS를 넘은 응답. 파일은 collect 끝의 정리가 지운다.
static OVER_TOKEN_LIMIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^Error: result \([0-9,]+ characters(?: across [0-9,]+ lines?)?\) exceeds maximum allowed tokens\.")
        .expect("상수 정규식")
});
pub(crate) static SESSION_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u:^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$)").expect("상수 정규식")
});

pub fn collect_cwd(home: &KairosHome) -> PathBuf {
    posix::join(&[&home.root().to_string_lossy(), "collect-cwd"]).into()
}

/// Claude Code의 프로젝트 폴더 이름. 영숫자 밖 문자가 각각 하이픈이 된다(측정). cwd는 실제 경로로 잡힌다.
/// 원본 정규식은 UTF-16 단위로 돈다. BMP 밖 글자는 하이픈 둘이다.
pub fn project_slug(path: &str) -> String {
    let mut slug = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else {
            slug.extend(std::iter::repeat_n('-', c.len_utf16()));
        }
    }
    slug
}

pub fn collector_project_dir(home: &KairosHome, user_home: &Path) -> PathBuf {
    let cwd = collect_cwd(home);
    // 아직 수집한 적이 없으면 없는 폴더다. 경로만 계산한다.
    let real = fs::canonicalize(&cwd).unwrap_or(cwd);
    let slug = project_slug(&real.to_string_lossy());
    posix::join(&[&user_home.to_string_lossy(), ".claude", "projects", &slug]).into()
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveredResponses {
    pub responses: Vec<McpResponse>,
    /// 원문을 되찾은 응답 수.
    pub recovered: usize,
    /// 표식이었지만 되찾지 못한 응답 수. 해당 응답은 고정 코드의 실패가 되어 다음 틱에 다시 읽힌다.
    pub failed: usize,
}

/// 수집 프로젝트 폴더를 쥐고 그 안의 큰 응답 파일만 다룬다.
#[derive(Clone, Debug)]
pub struct ToolResults {
    project_dir: PathBuf,
    max_bytes: u64,
}

fn single_text(content: &Value) -> Option<String> {
    content.as_array().filter(|blocks| blocks.len() == 1).and_then(|_| text_of(content))
}

fn persisted_path(text: &str) -> Option<&str> {
    PERSISTED.captures(text)?.get(1).map(|path| path.as_str())
}

impl ToolResults {
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        Self { project_dir: project_dir.into(), max_bytes: MAX_RECOVERED_BYTES }
    }

    /// 수집기가 쓰는 프로젝트 폴더. `user_home`은 `kairos_core::home::user_home()`이다.
    pub fn for_collector(home: &KairosHome, user_home: &Path) -> Self {
        Self::new(collector_project_dir(home, user_home))
    }

    #[must_use]
    pub const fn max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    fn file<'a>(&'a self, path: &'a str) -> ResultFile<'a> {
        ResultFile { path, project_dir: &self.project_dir, max_bytes: self.max_bytes }
    }

    /// 표식의 원문을 읽기만 한다. 파일은 뒤의 ingest가 쓰고 지운다. 표식이 아니거나 못 읽으면 None이다.
    pub fn peek_response(&self, content: &Value) -> KairosResult<Option<String>> {
        let text = single_text(content);
        let Some(path) = text.as_deref().and_then(persisted_path) else {
            return Ok(None);
        };
        Ok(self.file(path).read(false)?.ok())
    }

    /// 파싱 전에 표식을 원문으로 바꾼다. 표식이 아닌 응답은 손대지 않는다.
    pub fn recover_responses(&self, responses: Vec<McpResponse>) -> KairosResult<RecoveredResponses> {
        let mut report = RecoveredResponses { responses: Vec::with_capacity(responses.len()), recovered: 0, failed: 0 };
        for response in responses {
            let text = single_text(&response.content);
            if text.as_deref().is_some_and(|text| OVER_TOKEN_LIMIT.is_match(text)) {
                report.failed += 1;
                report.responses.push(ModeFailure::ResponseTooLarge.response(response.id));
                continue;
            }
            let Some(path) = text.as_deref().and_then(persisted_path) else {
                report.responses.push(response);
                continue;
            };
            match self.file(path).read(true)? {
                Ok(original) => {
                    report.recovered += 1;
                    let content = json!([{ "type": "text", "text": original }]);
                    report.responses.push(McpResponse { content, ..response });
                }
                Err(failure) => {
                    report.failed += 1;
                    report.responses.push(failure.response(response.id));
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 슬러그는_원본처럼_utf16_단위로_센다() {
        assert_eq!(project_slug("/가/😀a"), "-----a");
    }

    #[test]
    fn 표식의_끝은_js_공백까지_받고_그_밖의_글자는_받지_않는다() {
        let marker = "<persisted-output>\nOutput too large (1KB). Full output saved to: /a/b\n\n</persisted-output>";
        assert_eq!(persisted_path(marker), Some("/a/b"));
        assert_eq!(persisted_path(&format!("{marker}\n\u{FEFF} ")), Some("/a/b"));
        assert_eq!(persisted_path(&format!("{marker}\u{85}")), None, "U+0085는 JS 공백이 아니다");
        assert_eq!(persisted_path(&format!("{marker} x")), None);
    }
}
