//! GraphQL 응답(Issue, PullRequest 노드)을 객체와 이벤트로 바꾼다.
//! 네트워크와 DB를 쓰지 않는다. updated_at은 읽지도 않는다. 판단은 이벤트 자체의 시각으로만 한다.
//!
//! 저장 규칙: 댓글과 본문은 판정(멘션 여부, 멘션 수)에만 쓰고 저장하지 않는다.
//! 이벤트의 text_head에는 객체의 제목을 넣는다.

use kairos_core::model::{NormalizedEvent, NormalizedObject, ObjectKind, SourceName};
use serde_json::Value;

use crate::actor::{Actor, is_me};
use crate::events::EventLog;
use crate::json::{nodes, number_text, text};
use crate::mention::MentionScanner;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParsedNode {
    pub(crate) object: NormalizedObject,
    pub(crate) events: Vec<NormalizedEvent>,
}

pub(crate) struct NodeParser<'a> {
    mentions: &'a MentionScanner,
}

impl NodeParser<'static> {
    pub(crate) fn new() -> Self {
        Self { mentions: MentionScanner::shared() }
    }
}

impl<'a> NodeParser<'a> {
    #[cfg(test)]
    pub(crate) fn with_scanner(mentions: &'a MentionScanner) -> Self {
        Self { mentions }
    }

    /// Issue 또는 PullRequest 노드 1개를 해석한다. 해석할 수 없는 노드는 None이다.
    /// 멘션을 판정하지 못한 노드도 None이다. 호출자가 못 읽은 노드로 세어 partial을 내고 watermark를 멈춘다.
    pub(crate) fn parse(&self, raw: &Value) -> Option<ParsedNode> {
        let head = NodeHead::read(raw)?;
        let events = EventLog::read(&head, self.mentions).ok()?;
        Some(ParsedNode { object: head.object(), events })
    }
}

/// 객체와 이벤트가 함께 쓰는 노드의 머리 값.
pub(crate) struct NodeHead<'a> {
    pub(crate) node: &'a Value,
    pub(crate) is_pr: bool,
    pub(crate) node_id: &'a str,
    pub(crate) object_key: String,
    external_id: String,
    pub(crate) title: Option<&'a str>,
    pub(crate) permalink: Option<&'a str>,
    pub(crate) author: Actor<'a>,
    pub(crate) state: &'static str,
    pub(crate) rollup: Option<&'a Value>,
    pub(crate) checks_state: Option<&'a str>,
}

impl<'a> NodeHead<'a> {
    fn read(node: &'a Value) -> Option<Self> {
        let is_pr = match node.get("__typename")?.as_str()? {
            "PullRequest" => true,
            "Issue" => false,
            _ => return None,
        };
        let repo = text(node.get("repository").and_then(|repository| repository.get("nameWithOwner")))?;
        let number = number_text(node.get("number"))?;
        let node_id = text(node.get("id"))?;
        let external_id = format!("{repo}#{number}");
        let merged = is_pr && text(node.get("mergedAt")).is_some();
        let open = node.get("state").and_then(Value::as_str) == Some("OPEN");
        let commit = nodes(node.get("commits")).first().and_then(|entry| entry.get("commit"));
        let rollup = commit.and_then(|commit| commit.get("statusCheckRollup")).filter(|rollup| rollup.is_object());
        Some(Self {
            node,
            is_pr,
            node_id,
            object_key: format!("gh:{external_id}"),
            external_id,
            title: text(node.get("title")),
            permalink: text(node.get("url")),
            author: Actor::of(node.get("author").filter(|author| author.is_object()))
                .with_association(node.get("authorAssociation")),
            state: if merged {
                "merged"
            } else if open {
                "open"
            } else {
                "closed"
            },
            rollup,
            checks_state: text(rollup.and_then(|rollup| rollup.get("state"))),
        })
    }

    pub(crate) fn authored_by_me(&self) -> bool {
        is_me(self.author.login)
    }

    pub(crate) fn head_ref_oid(&self) -> Option<&'a str> {
        self.is_pr.then(|| text(self.node.get("headRefOid"))).flatten()
    }

    fn object(&self) -> NormalizedObject {
        let node = self.node;
        let owned = |value: Option<&str>| value.map(str::to_owned);
        let assigned = nodes(node.get("assignees")).iter().any(|assignee| is_me(Actor::of(Some(assignee)).login));
        let requested = nodes(node.get("reviewRequests"))
            .iter()
            .any(|request| is_me(Actor::of(request.get("requestedReviewer")).login));
        let pr_field = |value: Option<&str>| owned(value.filter(|_| self.is_pr));
        let kind = if self.is_pr { ObjectKind::PullRequest } else { ObjectKind::Issue };
        NormalizedObject {
            permalink: owned(self.permalink),
            actor_id: owned(self.author.login),
            ts: owned(text(node.get("createdAt"))),
            text_head: owned(self.title),
            trust_tier: Some(self.author.trust()),
            state: Some(self.state.to_owned()),
            milestone_title: owned(text(node.get("milestone").and_then(|milestone| milestone.get("title")))),
            assigned_to_me: Some(assigned),
            authored_by_me: Some(self.authored_by_me()),
            review_requested_me: Some(self.is_pr && requested),
            is_draft: self.is_pr.then(|| node.get("isDraft").and_then(Value::as_bool) == Some(true)),
            review_decision: pr_field(text(node.get("reviewDecision"))),
            checks_state: pr_field(self.checks_state),
            head_ref_oid: owned(self.head_ref_oid()),
            ..NormalizedObject::new(self.object_key.clone(), SourceName::Github, kind, self.external_id.clone())
        }
    }
}
