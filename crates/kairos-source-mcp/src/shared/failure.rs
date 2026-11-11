use std::sync::LazyLock;

use kairos_core::js;
use kairos_core::model::{McpResponse, Outcome};
use kairos_core::string_enum;
use regex::Regex;
use serde_json::{Value, json};

use super::JsonObject;

/// 응답이 실패인 까닭. `error_code`는 고정 문자열만 쓴다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Failure {
    pub outcome: Outcome,
    pub error_code: &'static str,
}

/// 응답을 해석하지 못했다는 신호. 코드는 고정 문자열만 쓴다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParseFailure {
    error_code: &'static str,
}

impl ParseFailure {
    pub const fn new(error_code: &'static str) -> Self {
        Self { error_code }
    }

    pub const fn error_code(self) -> &'static str {
        self.error_code
    }
}

string_enum! {
    /// 모드 자신의 실패. is_error 응답의 본문이 이 값이다. 외부 문자열이 아니다.
    pub enum ModeFailure {
        ToolMissing => "kairos:tool_missing",
        Refused => "kairos:refused",
        NotAllowed => "kairos:not_allowed",
        Timeout => "kairos:timeout",
        Hung => "kairos:hung",
        Threw => "kairos:threw",
        RecoverMissing => "kairos:recover_missing",
        RecoverOutside => "kairos:recover_outside",
        RecoverTooLarge => "kairos:recover_too_large",
        ResponseTooLarge => "kairos:response_too_large",
    }
}

impl ModeFailure {
    pub const fn failure(self) -> Failure {
        let (outcome, error_code) = match self {
            Self::ToolMissing => (Outcome::ToolMissing, "mcp_not_connected"),
            Self::Refused => (Outcome::ContractBreak, "mode_refused"),
            Self::NotAllowed => (Outcome::ContractBreak, "not_allowed"),
            Self::Timeout => (Outcome::Timeout, "mode_deadline"),
            // 시작한 호출이 마감까지 끝나지 않았다. 시작도 못 한 timeout과 달리 그 대상의 문제일 수 있다.
            Self::Hung => (Outcome::Timeout, "mode_hung"),
            Self::Threw => (Outcome::ContractBreak, "mcp_call_threw"),
            // 큰 응답 회수 실패. 저장하지 않았으므로 다음 틱에 다시 읽힌다.
            Self::RecoverMissing => (Outcome::ParseError, "large_response_missing"),
            Self::RecoverOutside => (Outcome::ContractBreak, "large_response_outside"),
            Self::RecoverTooLarge => (Outcome::ParseError, "large_response_over_limit"),
            Self::ResponseTooLarge => (Outcome::ParseError, "mcp_response_too_large"),
        };
        Failure { outcome, error_code }
    }

    /// 이 실패를 나타내는 응답. 큰 응답을 되찾지 못한 쪽도 같은 모양으로 낸다.
    pub fn response(self, id: impl Into<String>) -> McpResponse {
        McpResponse { id: id.into(), is_error: true, content: json!([{ "type": "text", "text": self.as_str() }]) }
    }
}

// 영문 패턴이라 대소문자 접기를 ASCII로 묶는다. 유니코드 접기는 켈빈 기호 같은 글자까지 맞춘다.
static AUTH_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i-u:\b40[13]\b|unauthori[sz]ed|unauthenticated|invalid_auth|not_authed|token_(?:expired|revoked)|re-?authenticate|login required)",
    )
    .expect("상수 정규식")
});
// JS 비유니코드 정규식의 `.`은 UTF-16 한 단위다. 그래서 낱말 사이의 한 글자에서 BMP 밖을 뺀다.
static RATE_ERROR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u:\b429\b|quota)|(?i-u:rate)[^\n\r\x{2028}\x{2029}\x{10000}-\x{10FFFF}]?(?i-u:limit)")
        .expect("상수 정규식")
});

/// MCP content 블록의 글자만 잇는다. 형식이 다르면 None이다.
pub fn text_of(content: &Value) -> Option<String> {
    let texts: Vec<&str> = content
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect();
    (!texts.is_empty()).then(|| texts.concat())
}

/// is_error 응답을 outcome으로 바꾼다. 본문은 판정에만 쓰고 오류 코드에 옮기지 않는다.
pub fn failure_of(response: &McpResponse) -> Failure {
    let text = text_of(&response.content).unwrap_or_default();
    if let Ok(mode) = text.parse::<ModeFailure>() {
        return mode.failure();
    }
    if AUTH_ERROR.is_match(&text) {
        return Failure { outcome: Outcome::Auth, error_code: "mcp_auth" };
    }
    if RATE_ERROR.is_match(&text) {
        return Failure { outcome: Outcome::RateLimited, error_code: "mcp_rate_limited" };
    }
    Failure { outcome: Outcome::ContractBreak, error_code: "mcp_tool_error" }
}

pub fn body_of(response: &McpResponse) -> Result<JsonObject, ParseFailure> {
    let text = text_of(&response.content).ok_or(ParseFailure::new("no_text_block"))?;
    match js::parse_json(&text) {
        Ok(Value::Object(body)) => Ok(body),
        Ok(_) => Err(ParseFailure::new("not_object")),
        Err(_) => Err(ParseFailure::new("not_json")),
    }
}
