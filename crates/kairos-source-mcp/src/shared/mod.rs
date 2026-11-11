//! MCP 소스의 공통 부분.
//!
//! 수집 모드와의 약속
//! - 호출 id는 `<source>:<query>`이고, 2쪽부터의 응답 id는 `<source>:<query>#<쪽>`이다.
//! - 후속 읽기(`McpCall::expand`)의 응답 id는 `<호출 id>!<대상 id>`다. 쪽으로 세지 않는다.
//! - 모드 자신의 실패는 is_error 응답의 본문을 `kairos:<코드>`로 낸다. 외부 문자열이 아니다.

mod failure;
mod frame;
mod pages;
mod trust;
mod window;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::Hash;

use kairos_core::Instant;
use kairos_core::model::{McpResponse, QuerySpec, SourceName, SourceResult};
use regex::Regex;
use serde_json::{Map, Value};

pub use failure::{Failure, ModeFailure, ParseFailure, body_of, failure_of, text_of};
pub use frame::ResultFrame;
pub(crate) use pages::has_next_token;
pub use pages::{PagesRead, read_pages};
pub(crate) use trust::{angle_address, is_trusted_address, trust_of};
pub(crate) use window::{DAY_MS, ceil_days, scan_start};
pub use window::{MAX_WINDOW_DAYS, MailWindow};

/// 응답 본문. `JSON.parse`한 객체다.
pub type JsonObject = Map<String, Value>;

/// `Date`가 담을 수 있는 가장 먼 시각.
const MAX_DATE_MS: f64 = 8.64e15;

/// 재시도로 메워지는 레코드의 코드. 건너뛴 구간이 남지 않으므로 `record_quarantined`와 달리 공백 고지가 아니다.
pub(crate) const RECORD_NULL: &str = "record_null";

/// 저장할 수 있는 순간. `Instant::parse`로 되읽히지 않는 값은 Ledger가 질의 전체를 contract_break로 막는다.
pub(crate) fn storable_instant(ms: f64) -> Option<Instant> {
    if !ms.is_finite() || ms.abs() > MAX_DATE_MS {
        return None;
    }
    let at = Instant::from_millis(ms as i64);
    Instant::parse(&at.to_iso()).map(|_| at)
}

/// 후속 읽기 응답의 표시.
const READ_MARK: char = '!';

/// 상수 정규식.
pub(crate) fn pattern(text: &str) -> Regex {
    Regex::new(text).expect("상수 정규식")
}

/// 설계 9.2의 신원 단언 기대값. 설정하지 않았으면 어떤 값과도 같지 않은 값을 준다.
/// 그런 소스는 등록 단계에서 연결 없음으로 빠지므로 여기까지 오지 않는다. 겹으로 막는다.
pub(crate) fn self_identity(source: SourceName) -> &'static str {
    source.self_identity().unwrap_or(UNSET_IDENTITY)
}

/// 응답의 어떤 식별자와도 같을 수 없는 값.
pub(crate) const UNSET_IDENTITY: &str = "\u{0}unset";

pub fn call_id(source: SourceName, query: &str) -> String {
    format!("{source}:{query}")
}

/// 호출 id의 쪽들. 응답 순서를 유지한다.
pub(crate) fn pages_of<'a>(responses: &'a [McpResponse], id: &str) -> Vec<&'a McpResponse> {
    let next_page = format!("{id}#");
    responses.iter().filter(|response| response.id == id || response.id.starts_with(&next_page)).collect()
}

/// 응답이 있는 질의만 해석한다.
pub(crate) fn parse_queries(
    source: SourceName,
    queries: &[QuerySpec],
    responses: &[McpResponse],
    mut parse_query: impl FnMut(&QuerySpec, &[&McpResponse]) -> SourceResult,
) -> Vec<SourceResult> {
    let mut results = Vec::new();
    for spec in queries {
        let pages = pages_of(responses, &call_id(source, &spec.id));
        if !pages.is_empty() {
            results.push(parse_query(spec, &pages));
        }
    }
    results
}

/// 후속 읽기 응답 하나.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetRead<'a> {
    pub target_id: &'a str,
    pub response: &'a McpResponse,
}

/// 후속 읽기 응답을 대상 id와 함께 돌려준다.
pub(crate) fn reads_of<'a>(responses: &'a [McpResponse], id: &str) -> Vec<TargetRead<'a>> {
    let prefix = format!("{id}{READ_MARK}");
    responses
        .iter()
        .filter_map(|response| {
            let target_id = response.id.strip_prefix(prefix.as_str())?;
            Some(TargetRead { target_id, response })
        })
        .collect()
}

/// `new Map(entries).values()`. 자리는 처음 본 키의 것이고 값은 마지막 것이다.
pub(crate) fn last_by_key<K: Eq + Hash, V>(entries: impl IntoIterator<Item = (K, V)>) -> Vec<V> {
    let mut slots = HashMap::new();
    let mut values: Vec<V> = Vec::new();
    for (key, value) in entries {
        match slots.entry(key) {
            Entry::Occupied(slot) => values[*slot.get()] = value,
            Entry::Vacant(slot) => {
                slot.insert(values.len());
                values.push(value);
            }
        }
    }
    values
}
