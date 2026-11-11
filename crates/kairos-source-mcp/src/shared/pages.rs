use kairos_core::model::{McpResponse, Outcome};
use serde_json::Value;

use super::{Failure, JsonObject, ParseFailure, body_of, failure_of};

/// 쪽을 읽은 결과.
#[derive(Debug)]
pub struct PagesRead<T> {
    pub parsed: Vec<T>,
    /// 실패가 없으면 None. 앞쪽 페이지를 읽은 뒤의 실패면 parsed가 비어 있지 않다.
    pub failure: Option<Failure>,
}

/// 쪽 순서대로 읽고 첫 실패에서 멈춘다. 해석 실패는 읽은 내용까지 버린다.
pub fn read_pages<T>(
    pages: &[&McpResponse],
    mut parse_page: impl FnMut(JsonObject) -> Result<T, ParseFailure>,
) -> PagesRead<T> {
    let mut parsed = Vec::new();
    for page in pages {
        if page.is_error {
            return PagesRead { parsed, failure: Some(failure_of(page)) };
        }
        match body_of(page).and_then(&mut parse_page) {
            Ok(value) => parsed.push(value),
            Err(failure) => {
                let failure = Failure { outcome: Outcome::ParseError, error_code: failure.error_code() };
                return PagesRead { parsed: Vec::new(), failure: Some(failure) };
            }
        }
    }
    PagesRead { parsed, failure: None }
}

/// Google 계열 응답의 다음 쪽 표시.
pub(crate) fn has_next_token(body: &JsonObject) -> bool {
    body.get("nextPageToken").and_then(Value::as_str).is_some_and(|token| !token.is_empty())
}
