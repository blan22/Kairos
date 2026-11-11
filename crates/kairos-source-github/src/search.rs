use std::sync::atomic::{AtomicI64, Ordering};

use crate::client::GhClient;
use crate::error::GhError;

const PAGE_SIZE: usize = 100;
const MAX_PAGES_PER_QUERY: usize = 3;
/// search는 분당 30회. 한 번 수집이 쓰는 검색 상한. 틱 간격 3분이라 분당 5회 이하다.
const MAX_SEARCH_CALLS: i64 = 15;

/// 질의들이 나눠 쓰는 검색 횟수. 질의가 스레드마다 돌아서 원자적으로 줄인다.
pub(crate) struct SearchBudget(AtomicI64);

impl SearchBudget {
    pub(crate) fn new() -> Self {
        Self(AtomicI64::new(MAX_SEARCH_CALLS))
    }

    fn take(&self) -> bool {
        self.0.try_update(Ordering::SeqCst, Ordering::SeqCst, |left| (left > 0).then(|| left - 1)).is_ok()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Candidates {
    pub(crate) ids: Vec<String>,
    pub(crate) total_count: f64,
    pub(crate) pages: i64,
    pub(crate) truncated: bool,
    pub(crate) incomplete: bool,
}

pub(crate) fn search_all(gh: &GhClient, query: &str, budget: &SearchBudget) -> Result<Candidates, GhError> {
    let mut ids = Vec::new();
    let mut total_count = 0.0;
    let mut pages = 0;
    let mut incomplete = false;
    for page in 1..=MAX_PAGES_PER_QUERY {
        if !budget.take() {
            break;
        }
        let result = gh.search(query, page)?;
        pages += 1;
        total_count = result.total_count;
        incomplete |= result.incomplete;
        let fetched = result.ids.len();
        ids.extend(result.ids);
        if fetched < PAGE_SIZE || ids.len() as f64 >= total_count {
            break;
        }
    }
    let truncated = (ids.len() as f64) < total_count;
    Ok(Candidates { ids, total_count, pages, truncated, incomplete })
}
