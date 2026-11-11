//! GraphQL 질의. 읽기 전용이고 updated_at은 가져오지 않는다.

use std::sync::LazyLock;

const ACTOR: &str = "__typename login";
/// last:N 연결마다 함께 읽는다. 없으면 창 밖의 댓글이 ok로 사라진다.
const PAGE_INFO: &str = "pageInfo { hasPreviousPage }";
const COMMENT: &str = "databaseId createdAt bodyText url authorAssociation author { ${ACTOR} }";
const COMMON: &str = "
  __typename id number title url state createdAt closedAt bodyText authorAssociation
  author { ${ACTOR} }
  repository { nameWithOwner }
  milestone { title }
  assignees(first: 10) { nodes { login } }
  comments(last: 20) { ${PAGE_INFO} nodes { ${COMMENT} } }
";
const ASSIGN_EVENTS: &str = "
  ... on AssignedEvent { __typename id createdAt actor { ${ACTOR} } assignee { ... on User { login } } }
  ... on UnassignedEvent { __typename id createdAt actor { ${ACTOR} } assignee { ... on User { login } } }
";
const REVIEWER: &str = "requestedReviewer { ... on User { login } ... on Team { slug } }";
const REVIEW: &str = "databaseId state submittedAt bodyText url authorAssociation author { ${ACTOR} }";
const ISSUE_ITEM_TYPES: &str = "[ASSIGNED_EVENT, UNASSIGNED_EVENT]";
const PR_ITEM_TYPES: &str = "[ASSIGNED_EVENT, UNASSIGNED_EVENT, REVIEW_REQUESTED_EVENT, REVIEW_REQUEST_REMOVED_EVENT]";
const PR_ITEMS: &str = "${ASSIGN_EVENTS}
          ... on ReviewRequestedEvent { __typename id createdAt actor { ${ACTOR} } ${REVIEWER} }
          ... on ReviewRequestRemovedEvent { __typename id createdAt actor { ${ACTOR} } ${REVIEWER} }";

const NODES_TEMPLATE: &str = "
query($ids: [ID!]!) {
  viewer { login }
  nodes(ids: $ids) {
    ... on Issue {
      ${COMMON}
      timelineItems(last: 30, itemTypes: ${ISSUE_ITEM_TYPES}) { ${PAGE_INFO} nodes { ${ASSIGN_EVENTS} } }
    }
    ... on PullRequest {
      ${COMMON}
      isDraft mergedAt headRefOid reviewDecision
      mergedBy { ${ACTOR} }
      reviewRequests(first: 20) { nodes { ${REVIEWER} } }
      reviews(last: 10) { ${PAGE_INFO} nodes { ${REVIEW} } }
      reviewThreads(last: 10) { ${PAGE_INFO} nodes { comments(last: 5) { ${PAGE_INFO} nodes { ${COMMENT} } } } }
      commits(last: 1) { nodes { commit { statusCheckRollup { state contexts(first: 50) { nodes {
        __typename
        ... on CheckRun { conclusion completedAt }
        ... on StatusContext { state createdAt }
      } } } } } }
      timelineItems(last: 30, itemTypes: ${PR_ITEM_TYPES}) {
        ${PAGE_INFO}
        nodes {
          ${PR_ITEMS}
        }
      }
    }
  }
}
";

const EXPANDED_TEMPLATE: &str = "
query($ids: [ID!]!) {
  viewer { login }
  nodes(ids: $ids) {
    ... on Issue {
      id
      comments(last: 100) { ${PAGE_INFO} nodes { ${COMMENT} } }
      timelineItems(last: 100, itemTypes: ${ISSUE_ITEM_TYPES}) { ${PAGE_INFO} nodes { ${ASSIGN_EVENTS} } }
    }
    ... on PullRequest {
      id
      comments(last: 100) { ${PAGE_INFO} nodes { ${COMMENT} } }
      reviews(last: 100) { ${PAGE_INFO} nodes { ${REVIEW} } }
      reviewThreads(last: 100) { ${PAGE_INFO} nodes { comments(last: 20) { ${PAGE_INFO} nodes { ${COMMENT} } } } }
      timelineItems(last: 100, itemTypes: ${PR_ITEM_TYPES}) { ${PAGE_INFO} nodes { ${PR_ITEMS} } }
    }
  }
}
";

fn expand(template: &str) -> String {
    // 바깥 조각부터 푼다. 안쪽 조각이 다른 조각을 품고 있다.
    let fragments = [
        ("${COMMON}", COMMON),
        ("${ISSUE_ITEM_TYPES}", ISSUE_ITEM_TYPES),
        ("${PR_ITEM_TYPES}", PR_ITEM_TYPES),
        ("${PR_ITEMS}", PR_ITEMS),
        ("${ASSIGN_EVENTS}", ASSIGN_EVENTS),
        ("${REVIEWER}", REVIEWER),
        ("${REVIEW}", REVIEW),
        ("${COMMENT}", COMMENT),
        ("${PAGE_INFO}", PAGE_INFO),
        ("${ACTOR}", ACTOR),
    ];
    fragments.iter().fold(template.to_owned(), |text, (name, fragment)| text.replace(name, fragment))
}

/// 읽기 전용 질의.
pub(crate) static NODES_QUERY: LazyLock<String> = LazyLock::new(|| expand(NODES_TEMPLATE));

/// 고정 창이 잘린 노드만 넓게 다시 읽는다. 전체 질의의 창을 키우면 스레드마다 연결이 늘어 틱당 GraphQL 점수가 몇 배가 된다.
/// 응답의 연결로 첫 질의의 같은 연결을 바꾼다.
pub(crate) static EXPANDED_QUERY: LazyLock<String> = LazyLock::new(|| expand(EXPANDED_TEMPLATE));
