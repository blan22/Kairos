//! gh로 직접 읽는 GitHub 소스 (설계 3.3). 읽기 전용이다.
//!
//! 틱마다: 후보 검색(질의별 1회 이상, 병렬) -> GraphQL 20개씩 판정. updated_at은 판단에 쓰지 않는다.
//! 멘션과 닫힘 검색만 updated 필터로 후보를 좁힌다. 소유 객체(리뷰 요청, 담당, 내가 연 것)는
//! 상태 필터만 걸고 시각 필터를 걸지 않는다. 체크 상태는 이슈의 updated_at을 올리지 않기 때문이다.
//!
//! 설계의 네 검색에 닫힘 검색 하나를 더했다. 열림 검색은 닫힌 객체를 더 이상 돌려주지 않아
//! gh_closed, gh_merged 증거(종료 판정의 근거)가 생기지 않는다.
//! 한 객체가 여러 질의에 걸리면 우선순위가 가장 높은 질의만 객체와 이벤트를 낸다.

mod actor;
mod client;
mod collection;
mod config;
mod defs;
mod error;
mod events;
mod json;
mod live;
mod mention;
mod node_parser;
mod node_reader;
mod parallel;
mod query;
mod search;
mod source;
#[cfg(test)]
mod tests;
mod truncation;

pub use config::GithubConfig;
pub use live::{LiveAuthorReader, LiveAuthors};
pub use source::GithubSource;

/// 본인 GitHub 로그인(소문자). 프로필에서 온다. 설정하지 않았으면 어떤 로그인과도 같지 않은 값이고,
/// 그때 소스는 등록 단계에서 연결 없음으로 빠진다.
fn self_login() -> &'static str {
    kairos_core::profile::current().identity.github_login.as_deref().unwrap_or("\u{0}unset")
}

/// 검색 범위의 조직들. 첫 값이 기본 조직이다.
fn orgs() -> &'static [String] {
    &kairos_core::profile::current().org.github_orgs
}
