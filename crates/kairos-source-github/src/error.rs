use kairos_core::model::Outcome;
use kairos_core::ports::CancelToken;

/// 호출 실패. 외부 문자열은 담지 않고 outcome과 코드만 둔다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GhError {
    outcome: Outcome,
    code: String,
}

impl GhError {
    pub(crate) fn new(outcome: Outcome, code: impl Into<String>) -> Self {
        Self { outcome, code: code.into() }
    }

    /// 예상하지 못한 실패. 취소 뒤라면 취소의 결과로 본다.
    pub(crate) fn unexpected(cancel: &CancelToken) -> Self {
        if cancel.is_cancelled() {
            Self::new(Outcome::Timeout, "gh_timeout")
        } else {
            Self::new(Outcome::ParseError, "gh_unexpected")
        }
    }

    pub(crate) fn outcome(&self) -> Outcome {
        self.outcome
    }

    pub(crate) fn code(&self) -> &str {
        &self.code
    }
}
