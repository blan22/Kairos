#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_core::model::{QuerySpec, SourceMode, SourceName};
use kairos_core::ports::{Clock, FixedClock};
use kairos_source_github::GithubConfig;
use kairos_tick::{SourceRegistry, not_connected_results};

fn standard() -> SourceRegistry {
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(at(T0)));
    SourceRegistry::standard(clock, missing_runner(), GithubConfig::from_vars(|_| None))
}

#[test]
fn 다섯_소스가_모두_목록에_있고_notion은_메일을_읽는_mcp_소스다() {
    let registry = standard();
    let names: Vec<SourceName> = registry.sources().iter().map(|source| source.name()).collect();
    assert_eq!(names, SourceName::ALL);
    let notion = registry.find(SourceName::Notion).unwrap();
    assert!(matches!(notion.mode(), SourceMode::Mcp(_)));
    assert_eq!(notion.queries(), [QuerySpec::new("mail", 900, 0)]);
    assert!(matches!(registry.find(SourceName::Github).unwrap().mode(), SourceMode::Direct(_)));
}

#[test]
fn 연결이_없는_소스는_틱마다_source_run에_not_connected로_남는다() {
    let db = test_db();
    let notion = unconnected(SourceName::Notion, vec![QuerySpec::new("mail", 900, 0)]);
    for row in not_connected_results(notion.as_ref(), at(T0)) {
        assert_eq!(ingest(&db, row, T0).outcome, kairos_core::model::Outcome::NotConnected);
    }
    assert_eq!(count(&db, "source_run"), 1);
}

/// 폴더 동적 로드가 명시적 등록으로 바뀌어 남은 약속은 빈자리를 채우고 순서를 지키는 것이다.
#[test]
fn 구현이_없는_소스도_목록에서_사라지지_않고_이름_순서를_지킨다() {
    let gmail = unconnected(SourceName::Gmail, Vec::new());
    let last = unconnected(SourceName::Gmail, vec![QuerySpec::new("recent", 3600, 0)]);
    let registry = SourceRegistry::new(vec![gmail, last]);
    let names: Vec<SourceName> = registry.sources().iter().map(|source| source.name()).collect();
    assert_eq!(names, SourceName::ALL);
    let reason = |name: SourceName| match registry.find(name).unwrap().mode() {
        SourceMode::NotConnected { reason } => reason.to_owned(),
        _ => panic!("연결 없는 소스여야 한다"),
    };
    assert_eq!(reason(SourceName::Gmail), "연결 없음");
    assert_eq!(registry.find(SourceName::Gmail).unwrap().queries().len(), 1, "같은 이름은 뒤의 것이 이긴다");
    assert_eq!(reason(SourceName::Calendar), "소스 구현 없음");
    assert_eq!(registry.find(SourceName::Calendar).unwrap().queries(), [QuerySpec::new("default", 180, 0)]);
}
