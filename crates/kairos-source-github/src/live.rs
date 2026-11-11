//! 세션 직전의 작성자 재확인(설계 8절). 세션이 읽을 이슈나 PR의 사람 글을 GitHub에서 모두 다시 읽는다.
//! 수집은 최근 20개와 30일 안의 글만 저장하고 틱 사이의 글은 아직 모른다. 그래서 DB만으로는 세션 입력을 다 알 수 없다.
//! 읽기 전용 GraphQL 질의만 쓴다. 다 읽었다고 보일 수 없으면 오류로 돌려 세션을 막는다.

use std::sync::{Arc, LazyLock};

use kairos_core::model::{ActorKind, TrustTier};
use kairos_core::ports::CancelToken;
use kairos_core::process::ProcessRunner;
use kairos_core::{KairosError, KairosResult};
use regex::Regex;
use serde_json::{Value, json};

use crate::actor::Actor;
use crate::client::GhClient;
use crate::config::GithubConfig;
use crate::error::GhError;
use crate::json::{text, word};

/// 질의 수 상한. 댓글 약 2천 개다. 넘으면 다 읽었다고 보이지 못한 것으로 친다.
const MAX_CALLS: usize = 20;

static TARGET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https://github\.com/([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/(?:pull|issues)/([0-9]+)$")
        .expect("상수 정규식")
});

/// 객체 본문과 각 연결의 첫 쪽. 리뷰 글은 스레드로 모두 닿는다.
const HEAD_QUERY: &str = "
query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    issueOrPullRequest(number: $number) {
      __typename
      ... on Issue {
        id number authorAssociation author { __typename login }
        comments(first: 100) {
          totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
        }
      }
      ... on PullRequest {
        id number authorAssociation author { __typename login }
        comments(first: 100) {
          totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
        }
        reviews(first: 100) {
          totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
        }
        reviewThreads(first: 50) {
          totalCount pageInfo { hasNextPage endCursor }
          nodes {
            id
            comments(first: 100) {
              totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
            }
          }
        }
      }
    }
  }
}
";

/// 이슈, PR, 리뷰 스레드의 다음 댓글 쪽.
const MORE_COMMENTS: &str = "
query($id: ID!, $after: String!) {
  node(id: $id) {
    ... on Issue {
      comments(first: 100, after: $after) {
        totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
      }
    }
    ... on PullRequest {
      comments(first: 100, after: $after) {
        totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
      }
    }
    ... on PullRequestReviewThread {
      comments(first: 100, after: $after) {
        totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
      }
    }
  }
}
";

const MORE_REVIEWS: &str = "
query($id: ID!, $after: String!) {
  node(id: $id) {
    ... on PullRequest {
      reviews(first: 100, after: $after) {
        totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
      }
    }
  }
}
";

const MORE_THREADS: &str = "
query($id: ID!, $after: String!) {
  node(id: $id) {
    ... on PullRequest {
      reviewThreads(first: 50, after: $after) {
        totalCount pageInfo { hasNextPage endCursor }
        nodes {
          id
          comments(first: 100) {
            totalCount pageInfo { hasNextPage endCursor } nodes { authorAssociation author { __typename login } }
          }
        }
      }
    }
  }
}
";

/// 세션 입력이 되는 사람 글의 판정.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveAuthors {
    /// 모두 읽었고 사람 글은 T0, T1뿐이다.
    Insiders,
    /// 수집과 같은 `Actor` 규칙으로 T3인 사람의 글이 있다. 외부 글로 단정해도 되는 경우다.
    Outsider,
    /// 외부 글은 없지만 누가 썼는지 단정할 수 없는 사람 글이 있다. 지운 계정이나 연관 정보가 빠진 글이다.
    /// 세션은 막되 외부 글로 단정하지 않는다. 수집은 연관 정보가 없으면 T1로 본다.
    Unknown,
}

/// 글 하나의 판정.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Writing {
    /// 세션 입력으로 믿는 글. 봇과 본인, T1인 사람이다.
    Trusted,
    Unknown,
    Outsider,
}

/// 다음에 읽을 쪽이 있는 연결.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Field {
    Comments,
    Reviews,
    Threads,
}

impl Field {
    const fn key(self) -> &'static str {
        match self {
            Self::Comments => "comments",
            Self::Reviews => "reviews",
            Self::Threads => "reviewThreads",
        }
    }

    const fn more_query(self) -> &'static str {
        match self {
            Self::Comments => MORE_COMMENTS,
            Self::Reviews => MORE_REVIEWS,
            Self::Threads => MORE_THREADS,
        }
    }
}

struct Pending {
    owner: String,
    field: Field,
    after: String,
    seen: u64,
}

fn refused(code: &str) -> KairosError {
    KairosError::new(code)
}

fn failed(error: &GhError) -> KairosError {
    KairosError::new(format!("live_{}", error.code()))
}

/// GitHub이 지운 계정의 글에 붙이는 작성자. 실제 사용자는 이 이름을 가질 수 없다.
const GHOST_LOGIN: &str = "ghost";
/// GitHub의 `CommentAuthorAssociation` 값 전체. 이 밖의 값(빈 값 포함)은 누가 썼는지 단정하지 않는다.
const KNOWN_ASSOCIATIONS: [&str; 8] =
    ["OWNER", "MEMBER", "COLLABORATOR", "CONTRIBUTOR", "FIRST_TIMER", "FIRST_TIME_CONTRIBUTOR", "MANNEQUIN", "NONE"];

/// 세션 입력이 되는 사람 글의 판정. 봇은 수집 게이트와 같이 세지 않는다.
/// 연관 정보가 없으면 수집은 조직의 사람(T1)으로 보지만, 이 질의는 늘 그 값을 읽으므로 없거나 모르는 값이면 믿지 않는다.
/// 지운 계정도 누가 썼는지 모른다. 둘 다 세션은 막되 외부 글로 단정하지 않는다. 외부 글은 `Actor` 규칙의 T3뿐이다.
fn judge(node: &Value) -> Writing {
    let association = node.get("authorAssociation");
    let actor = Actor::of(node.get("author")).with_association(association);
    match actor.kind() {
        ActorKind::Bot | ActorKind::SelfActor => Writing::Trusted,
        ActorKind::Human
            if !word(association).is_some_and(|word| KNOWN_ASSOCIATIONS.contains(&word))
                || actor.login.is_none_or(|login| login.eq_ignore_ascii_case(GHOST_LOGIN)) =>
        {
            Writing::Unknown
        }
        ActorKind::Human if actor.trust() == TrustTier::T1 => Writing::Trusted,
        ActorKind::Human => Writing::Outsider,
    }
}

#[derive(Default)]
struct Walk {
    pending: Vec<Pending>,
    /// 작성자를 알 수 없는 글을 봤다. 외부 글이 더 있는지 끝까지 읽는다.
    unknown: bool,
}

impl Walk {
    /// 외부 글이면 참이다. 알 수 없는 글은 적어 두고 계속 읽는다.
    fn outsider(&mut self, node: &Value) -> bool {
        match judge(node) {
            Writing::Trusted => false,
            Writing::Unknown => {
                self.unknown = true;
                false
            }
            Writing::Outsider => true,
        }
    }
}

impl Walk {
    /// 연결의 한 쪽을 본다. 외부 글이 있으면 참이다. 다음 쪽은 쌓아 두고, 끝 쪽이면 본 수가 totalCount와 같아야 한다.
    fn connection(&mut self, owner: &str, field: Field, value: Option<&Value>, seen: u64) -> KairosResult<bool> {
        let value = value.filter(|value| value.is_object()).ok_or_else(|| refused("live_shape"))?;
        let total = value.get("totalCount").and_then(Value::as_u64).ok_or_else(|| refused("live_shape"))?;
        let nodes = value.get("nodes").and_then(Value::as_array).ok_or_else(|| refused("live_shape"))?;
        let seen = seen + nodes.len() as u64;
        for node in nodes {
            // 권한이나 오류로 비운 노드는 누가 썼는지 모른다.
            if !node.is_object() {
                return Err(refused("live_hidden_node"));
            }
            let outsider = match field {
                Field::Threads => {
                    let thread = text(node.get("id")).ok_or_else(|| refused("live_shape"))?;
                    self.connection(thread, Field::Comments, node.get("comments"), 0)?
                }
                Field::Comments | Field::Reviews => self.outsider(node),
            };
            if outsider {
                return Ok(true);
            }
        }
        let page = value.get("pageInfo");
        match page.and_then(|page| page.get("hasNextPage")).and_then(Value::as_bool) {
            Some(true) => {
                let after = text(page.and_then(|page| page.get("endCursor"))).ok_or_else(|| refused("live_shape"))?;
                self.pending.push(Pending { owner: owner.to_owned(), field, after: after.to_owned(), seen });
            }
            Some(false) if seen == total => {}
            Some(false) => return Err(refused("live_incomplete")),
            None => return Err(refused("live_shape")),
        }
        Ok(false)
    }
}

/// claude를 띄우기 직전에 대상의 사람 글 작성자를 모두 다시 읽는다. 등급은 수집과 같은 `Actor`로 매긴다.
pub struct LiveAuthorReader {
    runner: Arc<dyn ProcessRunner>,
    config: GithubConfig,
}

impl LiveAuthorReader {
    pub fn new(runner: Arc<dyn ProcessRunner>, config: GithubConfig) -> Self {
        Self { runner, config }
    }

    /// 다 읽지 못했거나 응답 모양이 다르면 오류다. 오류 코드는 `live_`로 시작한다.
    /// 외부 글을 하나라도 보면 `Outsider`다. 외부 글 없이 알 수 없는 글만 있으면 끝까지 읽은 뒤 `Unknown`이다.
    pub fn read(&self, url: &str) -> KairosResult<LiveAuthors> {
        let captures = TARGET.captures(url).ok_or_else(|| refused("live_bad_url"))?;
        let (owner, name) = (&captures[1], &captures[2]);
        let number: u64 = captures[3].parse().map_err(|_| refused("live_bad_url"))?;
        if !kairos_core::profile::current().org.is_org(owner) {
            return Err(refused("live_bad_url"));
        }
        let client =
            GhClient::connect(self.runner.clone(), &self.config, CancelToken::new()).map_err(|e| failed(&e))?;
        let variables = json!({ "owner": owner, "name": name, "number": number });
        let head = client.query(HEAD_QUERY, variables).map_err(|e| failed(&e))?;
        let node = head
            .pointer("/repository/issueOrPullRequest")
            .filter(|node| node.is_object())
            .ok_or_else(|| refused("live_not_found"))?;
        if node.get("number").and_then(Value::as_u64) != Some(number) {
            return Err(refused("live_shape"));
        }
        let id = text(node.get("id")).ok_or_else(|| refused("live_shape"))?;
        let fields: &[Field] = match text(node.get("__typename")) {
            Some("Issue") => &[Field::Comments],
            Some("PullRequest") => &[Field::Comments, Field::Reviews, Field::Threads],
            _ => return Err(refused("live_shape")),
        };
        let mut walk = Walk::default();
        if walk.outsider(node) {
            return Ok(LiveAuthors::Outsider);
        }
        for field in fields {
            if walk.connection(id, *field, node.get(field.key()), 0)? {
                return Ok(LiveAuthors::Outsider);
            }
        }
        let mut calls = 1;
        while let Some(next) = walk.pending.pop() {
            calls += 1;
            if calls > MAX_CALLS {
                return Err(refused("live_too_long"));
            }
            let variables = json!({ "id": next.owner, "after": next.after });
            let data = client.query(next.field.more_query(), variables).map_err(|e| failed(&e))?;
            let value = data.get("node").and_then(|node| node.get(next.field.key()));
            if walk.connection(&next.owner, next.field, value, next.seen)? {
                return Ok(LiveAuthors::Outsider);
            }
        }
        Ok(if walk.unknown { LiveAuthors::Unknown } else { LiveAuthors::Insiders })
    }
}
