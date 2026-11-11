//! 다섯 검색 질의와 수집의 상수.

use std::sync::LazyLock;

use kairos_core::Instant;
use kairos_core::model::{CollectContext, CursorState, QuerySpec};
use regex::{NoExpand, Regex};

use crate::{orgs, self_login};

pub(crate) const OVERLAP_SECONDS: i64 = 300;
const INTERVAL_SECONDS: i64 = 180;
const HOUR_MS: i64 = 3600 * 1000;

static TRAILING_MILLIS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\.[0-9]{3}Z$").expect("상수 정규식"));

pub(crate) struct QueryDef {
    pub(crate) id: &'static str,
    pub(crate) snapshot: bool,
    /// 로그인 앞에 붙는 검색 조건.
    terms: &'static str,
    /// 첫 수집의 검색 창. None이면 시각 필터를 걸지 않는다.
    /// 이전 이벤트는 baseline이라 길게 볼 이유가 없다. 닫힌 객체는 양이 많아 짧게 본다.
    first_run_lookback_ms: Option<i64>,
}

/// 우선순위 순서다. 앞 질의가 객체를 소유한다.
pub(crate) const DEFS: [QueryDef; 5] = [
    QueryDef {
        id: "review_requested",
        snapshot: true,
        terms: "is:open review-requested:",
        first_run_lookback_ms: None,
    },
    QueryDef { id: "assigned", snapshot: true, terms: "is:open assignee:", first_run_lookback_ms: None },
    QueryDef { id: "authored", snapshot: true, terms: "is:open author:", first_run_lookback_ms: None },
    QueryDef { id: "mentions", snapshot: false, terms: "mentions:", first_run_lookback_ms: Some(7 * 24 * HOUR_MS) },
    QueryDef { id: "closed", snapshot: false, terms: "is:closed involves:", first_run_lookback_ms: Some(24 * HOUR_MS) },
];

impl QueryDef {
    pub(crate) fn spec(&self) -> QuerySpec {
        QuerySpec::new(self.id, INTERVAL_SECONDS, OVERLAP_SECONDS)
    }

    /// 이 질의의 검색 시작. 시각 필터가 없는 질의는 None이다.
    pub(crate) fn since(&self, context: &CollectContext) -> Option<Instant> {
        let lookback = self.first_run_lookback_ms?;
        let cursor = context.cursor(self.id).and_then(CursorState::window_start);
        Some(cursor.unwrap_or_else(|| context.now.minus_ms(lookback)))
    }

    pub(crate) fn search_text(&self, since: Option<Instant>) -> String {
        let filter = since.map_or_else(String::new, |at| {
            format!(" updated:>={}", TRAILING_MILLIS.replace(&at.to_iso(), NoExpand("Z")))
        });
        let scope: Vec<String> = orgs().iter().map(|org| format!("org:{org}")).collect();
        format!("{} {}{}{filter}", scope.join(" "), self.terms, self_login())
    }
}

/// 아직 확인하지 못한 가장 이른 시각. 질의마다 커서가 달라도 빠짐이 없게 가장 앞을 쓴다.
pub(crate) fn earliest_window(context: &CollectContext) -> Option<Instant> {
    DEFS.iter().filter_map(|def| context.cursor(def.id)?.window_start()).min()
}

/// 이 질의가 같은 소스에서 가장 앞선 질의보다 뒤처진 시간(ms). 다른 질의가 전진하는 동안만 늘어
/// 수집하지 않은 밤사이와 주말은 세지 않는다. ok가 아직 없으면 저장한 첫 실행부터 잰다.
pub(crate) fn lag_behind(context: &CollectContext, id: &str) -> i64 {
    let at = |text: Option<&String>| text.and_then(|text| Instant::parse(text));
    let newest = DEFS.iter().filter_map(|def| at(context.cursor(def.id)?.watermark.as_ref())).max();
    let own = context.cursor(id).and_then(|cursor| at(cursor.watermark.as_ref()).or(at(cursor.first_run_at.as_ref())));
    newest.zip(own).map_or(0, |(newest, own)| newest.ms_since(own).max(0))
}

/// 이 질의가 ok 없이 partial로 이어진 시간(ms). GitHub은 progress_at을 이어진 partial의 첫 시각으로 쓴다.
/// 검색 창은 watermark로만 정해 progress_at을 창에 쓰지 않는다. ok가 지운다.
pub(crate) fn partial_for(context: &CollectContext, id: &str) -> i64 {
    let since = context.cursor(id).and_then(|cursor| Instant::parse(cursor.progress_at.as_deref()?));
    since.map_or(0, |since| context.now.ms_since(since).max(0))
}
