//! `모든 last 연결이 pageInfo를 읽는다`.

use kairos_core::js;
use regex::Regex;
use serde_json::Value;

use crate::query::{EXPANDED_QUERY, NODES_QUERY};

#[test]
fn 모든_last_연결이_page_info를_읽는다() {
    let window = Regex::new(r"[A-Za-z0-9_]+\(last: [0-9]+").unwrap();
    let paged = Regex::new(&format!(r"[A-Za-z0-9_]+\(last: [0-9]+[^)]*\) \{{[{}]*pageInfo", js::WS)).unwrap();
    for query in [NODES_QUERY.as_str(), EXPANDED_QUERY.as_str()] {
        // commits(last: 1)은 헤드 커밋만 본다. 창 밖이 곧 옛 커밋이라 잘림이 없다.
        let windows = window.find_iter(query).filter(|found| !found.as_str().starts_with("commits")).count();
        assert!(windows > 0);
        assert_eq!(paged.find_iter(query).count(), windows);
    }
}

/// 넓은 질의에 없는 연결은 잘려도 고칠 수 없어 watermark가 멈춘다.
#[test]
fn 넓은_질의는_잘림을_판정하는_연결을_모두_다시_읽는다() {
    let named = |name: &str| EXPANDED_QUERY.matches(&format!("{name}(last: 100")).count();
    assert_eq!((named("comments"), named("timelineItems")), (2, 2), "이슈와 PR 모두");
    assert_eq!((named("reviews"), named("reviewThreads")), (1, 1), "PR에만 있다");
}

/// 고정 자료는 `NODES_QUERY`, `EXPANDED_QUERY`를 JSON으로 그대로 뽑은 것이다.
#[test]
fn 질의는_고정값과_바이트까지_같다() {
    let original: Value = serde_json::from_str(include_str!("../../tests/fixtures/github/queries.json")).unwrap();
    assert_eq!(original["NODES_QUERY"].as_str(), Some(NODES_QUERY.as_str()));
    assert_eq!(original["EXPANDED_QUERY"].as_str(), Some(EXPANDED_QUERY.as_str()));
}
