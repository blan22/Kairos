//! 노드 하나의 이벤트를 쌓는다. 이벤트 순서는 저장 순서가 되고 항목 생성 순서에 영향을 준다.
//! 그래서 본문, 댓글, 리뷰, 인라인 댓글, 타임라인, 병합과 닫힘, CI의 순서를 바꾸지 않는다.

use kairos_core::Instant;
use kairos_core::model::{EventType, NormalizedEvent, SourceName};
use serde_json::Value;

use crate::actor::{Actor, is_me};
use crate::json::{nodes, number_text, text, word};
use crate::mention::{MentionInfo, MentionScanner, ScanFailed};
use crate::node_parser::NodeHead;

const FAILED_CONCLUSIONS: [&str; 4] = ["FAILURE", "TIMED_OUT", "STARTUP_FAILURE", "CANCELLED"];
const FAILED_STATES: [&str; 2] = ["FAILURE", "ERROR"];

/// 사람의 글 하나: 본문, 댓글, 리뷰, 인라인 리뷰 댓글.
#[derive(Clone, Copy)]
struct Writing<'a> {
    actor: Actor<'a>,
    body: Option<&'a Value>,
    url: Option<&'a str>,
}

impl<'a> Writing<'a> {
    fn of(entry: &'a Value) -> Self {
        Self {
            actor: Actor::of(entry.get("author").filter(|author| author.is_object()))
                .with_association(entry.get("authorAssociation")),
            body: entry.get("bodyText"),
            url: text(entry.get("url")),
        }
    }
}

#[derive(Default)]
struct CheckTimes {
    failed_at: Option<Instant>,
    done_at: Option<Instant>,
}

fn check_times(rollup: Option<&Value>) -> CheckTimes {
    let mut times = CheckTimes::default();
    for context in nodes(rollup.and_then(|rollup| rollup.get("contexts"))) {
        let failed = if context.get("__typename").and_then(Value::as_str) == Some("CheckRun") {
            word(context.get("conclusion")).is_some_and(|word| FAILED_CONCLUSIONS.contains(&word))
        } else {
            word(context.get("state")).is_some_and(|word| FAILED_STATES.contains(&word))
        };
        let stamp = text(context.get("completedAt")).or_else(|| text(context.get("createdAt")));
        let Some(at) = stamp.and_then(Instant::parse) else {
            continue;
        };
        times.done_at = times.done_at.max(Some(at));
        if failed {
            times.failed_at = times.failed_at.max(Some(at));
        }
    }
    times
}

pub(crate) struct EventLog<'a> {
    head: &'a NodeHead<'a>,
    mentions: &'a MentionScanner,
    events: Vec<NormalizedEvent>,
}

impl<'a> EventLog<'a> {
    pub(crate) fn read(
        head: &'a NodeHead<'a>,
        mentions: &'a MentionScanner,
    ) -> Result<Vec<NormalizedEvent>, ScanFailed> {
        let mut log = Self { head, mentions, events: Vec::new() };
        log.body()?;
        log.comments()?;
        let my_review_times = log.reviews()?;
        log.review_threads()?;
        log.timeline(&my_review_times);
        log.closure();
        log.checks();
        Ok(log.events)
    }

    /// 시각이 없거나 해석되지 않으면 이벤트를 만들지 않는다.
    fn push(
        &mut self,
        dedupe_key: String,
        event_type: EventType,
        occurred_at: Option<&str>,
        actor: Actor<'_>,
    ) -> Option<&mut NormalizedEvent> {
        let occurred_at = occurred_at.filter(|at| Instant::parse(at).is_some())?;
        self.events.push(NormalizedEvent {
            actor_id: actor.login.map(str::to_owned),
            object_key: Some(self.head.object_key.clone()),
            thread_key: Some(self.head.object_key.clone()),
            permalink: self.head.permalink.map(str::to_owned),
            text_head: self.head.title.map(str::to_owned),
            trust_tier: Some(actor.trust()),
            ..NormalizedEvent::new(dedupe_key, SourceName::Github, event_type, occurred_at, actor.kind())
        });
        self.events.last_mut()
    }

    fn push_writing(
        &mut self,
        dedupe_key: String,
        event_type: EventType,
        at: Option<&str>,
        writing: Writing<'_>,
        mention: MentionInfo,
    ) {
        if let Some(event) = self.push(dedupe_key, event_type, at, writing.actor) {
            event.mentions_me = Some(mention.mentions_me);
            event.mention_count = Some(mention.mention_count);
            if let Some(url) = writing.url {
                event.permalink = Some(url.to_owned());
            }
        }
    }

    fn write(
        &mut self,
        id: Option<&str>,
        kind: &str,
        event_type: EventType,
        at: Option<&Value>,
        writing: Writing<'_>,
    ) -> Result<(), ScanFailed> {
        let Some(id) = id else {
            return Ok(());
        };
        let mention = self.mentions.scan(writing.body)?;
        self.push_writing(format!("gh:{kind}:{id}"), event_type, text(at), writing, mention);
        Ok(())
    }

    /// 본문 멘션: 본인이 쓴 본문은 신호가 아니다. 멘션이 없으면 이벤트를 만들지 않는다.
    fn body(&mut self) -> Result<(), ScanFailed> {
        let head = self.head;
        let body = head.node.get("bodyText");
        if self.mentions.scan(body)?.mentions_me && !head.authored_by_me() {
            let writing = Writing { actor: head.author, body, url: None };
            self.write(Some(head.node_id), "body", EventType::GhComment, head.node.get("createdAt"), writing)?;
        }
        Ok(())
    }

    fn comments(&mut self) -> Result<(), ScanFailed> {
        for comment in nodes(self.head.node.get("comments")) {
            self.comment(comment, "comment")?;
        }
        Ok(())
    }

    fn comment(&mut self, comment: &Value, kind: &str) -> Result<(), ScanFailed> {
        let id = number_text(comment.get("databaseId"));
        self.write(id.as_deref(), kind, EventType::GhComment, comment.get("createdAt"), Writing::of(comment))
    }

    /// 내 리뷰 시각을 돌려준다. 재요청과 첫 요청을 가르는 데 쓴다.
    fn reviews(&mut self) -> Result<Vec<Instant>, ScanFailed> {
        let mut my_review_times = Vec::new();
        for review in nodes(self.head.node.get("reviews")) {
            if text(review.get("state")) == Some("PENDING") {
                continue;
            }
            let at = review.get("submittedAt");
            let writing = Writing::of(review);
            if is_me(writing.actor.login) {
                my_review_times.extend(text(at).and_then(Instant::parse));
            }
            let id = number_text(review.get("databaseId"));
            self.write(id.as_deref(), "review", EventType::GhReview, at, writing)?;
            let Some(id) = id else {
                continue;
            };
            match review.get("state").and_then(Value::as_str) {
                Some("CHANGES_REQUESTED") => {
                    let mention = self.mentions.scan(writing.body)?;
                    let key = format!("gh:changes:{id}");
                    self.push_writing(key, EventType::GhChangesRequested, text(at), writing, mention);
                }
                Some("APPROVED") => {
                    let key = format!("gh:approved:{id}");
                    if let Some(event) = self.push(key, EventType::GhReviewApproved, text(at), writing.actor)
                        && let Some(url) = writing.url
                    {
                        event.permalink = Some(url.to_owned());
                    }
                }
                _ => {}
            }
        }
        Ok(my_review_times)
    }

    fn review_threads(&mut self) -> Result<(), ScanFailed> {
        for thread in nodes(self.head.node.get("reviewThreads")) {
            for comment in nodes(thread.get("comments")) {
                self.comment(comment, "rcomment")?;
            }
        }
        Ok(())
    }

    fn timeline(&mut self, my_review_times: &[Instant]) {
        for item in nodes(self.head.node.get("timelineItems")) {
            let Some(id) = text(item.get("id")) else {
                continue;
            };
            let at = text(item.get("createdAt"));
            let actor = Actor::of(item.get("actor"));
            let requested_me = is_me(Actor::of(item.get("requestedReviewer")).login);
            let assigned_me = is_me(Actor::of(item.get("assignee")).login);
            match item.get("__typename").and_then(Value::as_str) {
                Some("ReviewRequestedEvent") if requested_me => {
                    let requested_at = at.and_then(Instant::parse);
                    let again = requested_at.is_some_and(|at| my_review_times.iter().any(|review| *review < at));
                    let event_type = if again { EventType::GhReviewRerequested } else { EventType::GhReviewRequested };
                    self.push(format!("gh:rr:{id}"), event_type, at, actor);
                }
                Some("ReviewRequestRemovedEvent") if requested_me => {
                    self.push(format!("gh:rrr:{id}"), EventType::GhReviewRequestRemoved, at, actor);
                }
                Some("AssignedEvent") if assigned_me => {
                    self.push(format!("gh:assign:{id}"), EventType::GhAssigned, at, actor);
                }
                Some("UnassignedEvent") if assigned_me => {
                    self.push(format!("gh:unassign:{id}"), EventType::GhUnassigned, at, actor);
                }
                _ => {}
            }
        }
    }

    fn closure(&mut self) {
        let head = self.head;
        match head.state {
            "merged" => {
                let actor = Actor::of(head.node.get("mergedBy"));
                let at = text(head.node.get("mergedAt"));
                self.push(format!("gh:merged:{}", head.node_id), EventType::GhMerged, at, actor);
            }
            "closed" => {
                let at = text(head.node.get("closedAt"));
                self.push(format!("gh:closed:{}", head.node_id), EventType::GhClosed, at, Actor::of(None));
            }
            _ => {}
        }
    }

    /// CI: 내 열린 PR만. 같은 헤드 커밋이라도 완료 시각이 다르면 다른 사건이다.
    fn checks(&mut self) {
        let head = self.head;
        let Some(oid) = head.head_ref_oid().filter(|_| head.authored_by_me() && head.state == "open") else {
            return;
        };
        let times = check_times(head.rollup);
        let (label, event_type, at) = match (head.checks_state, times.failed_at, times.done_at) {
            (Some("FAILURE" | "ERROR"), Some(failed_at), _) => ("failed", EventType::GhCiFailed, failed_at),
            (Some("SUCCESS"), _, Some(done_at)) => ("ok", EventType::GhCiRecovered, done_at),
            _ => return,
        };
        let key = format!("gh:ci:{oid}:{label}:{}", at.millis());
        self.push(key, event_type, Some(&at.to_iso()), Actor::CHECKS);
    }
}
