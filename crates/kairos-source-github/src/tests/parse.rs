use kairos_core::Instant;
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, ObjectKind};
use serde_json::{Value, json};

use super::fake::{nodes, spread};
use crate::actor::Actor;
use crate::mention::{MentionInfo, MentionScanner};
use crate::node_parser::{NodeParser, ParsedNode};
use crate::self_login;
use crate::truncation::truncated_connections;

fn parsed(name: &str) -> ParsedNode {
    NodeParser::new().parse(&nodes()[name]).unwrap()
}

fn by_key<'a>(events: &'a [NormalizedEvent], key: &str) -> Option<&'a NormalizedEvent> {
    events.iter().find(|event| event.dedupe_key == key)
}

fn kind_of(actor: &Value) -> ActorKind {
    Actor::of(Some(actor)).kind()
}

fn mention(body: &str) -> MentionInfo {
    MentionScanner::shared().scan(Some(&json!(body))).unwrap()
}

#[test]
fn actor_kind_bot_타입과_bot_접미사는_bot_본인는_self_나머지는_human() {
    assert_eq!(kind_of(&json!({ "__typename": "Bot", "login": "synthetic-ci-app" })), ActorKind::Bot);
    assert_eq!(kind_of(&json!({ "__typename": "User", "login": "synthetic[bot]" })), ActorKind::Bot);
    assert_eq!(kind_of(&json!({ "__typename": "User", "login": "kairos-dev" })), ActorKind::SelfActor);
    assert_eq!(kind_of(&json!({ "__typename": "User", "login": "synthetic-teammate" })), ActorKind::Human);
    assert_eq!(kind_of(&Value::Null), ActorKind::Human);
}

#[test]
fn 멘션_판정_이메일과_접두_일치는_멘션이_아니고_서로_다른_핸들_수를_센다() {
    assert_eq!(mention("확인 @kairos-dev @a-b @a-b"), MentionInfo { mentions_me: true, mention_count: 2 });
    assert!(!mention("a@kairos-dev.example").mentions_me);
    assert!(!mention("@kairos-devx").mentions_me);
    assert!(!mention("@kairos-dev-bot").mentions_me);
    assert!(mention("@Kairos-Dev,").mentions_me);
    assert_eq!(MentionScanner::shared().scan(Some(&Value::Null)), Ok(MentionInfo::default()));
}

#[test]
fn 내_pr_멘션_없는_사람_댓글을_잡고_봇과_본인_활동은_actor_kind로_구분해_저장한다() {
    let ParsedNode { object, events } = parsed("pr_authored");
    assert_eq!(object.object_key, "gh:example-org/widget#11");
    assert_eq!(object.kind, ObjectKind::PullRequest);
    assert_eq!(object.authored_by_me, Some(true));
    assert_eq!(object.milestone_title.as_deref(), Some("합성 마일스톤 알파"));
    assert_eq!(object.checks_state.as_deref(), Some("FAILURE"));
    assert_eq!(object.head_ref_oid.as_deref(), Some("aaaa1111"));
    assert_eq!(object.review_decision.as_deref(), Some("CHANGES_REQUESTED"));

    let human = by_key(&events, "gh:comment:101").unwrap();
    assert_eq!(human.actor_kind, ActorKind::Human);
    assert_eq!(human.mentions_me, Some(false));
    assert_eq!(by_key(&events, "gh:comment:102").unwrap().actor_kind, ActorKind::Bot);
    assert_eq!(by_key(&events, "gh:comment:103").unwrap().actor_kind, ActorKind::SelfActor);
    let mentioned = by_key(&events, "gh:comment:104").unwrap();
    assert_eq!(mentioned.mentions_me, Some(true));
    assert_eq!(mentioned.mention_count, Some(3));
    assert_eq!(by_key(&events, "gh:comment:105").unwrap().mentions_me, Some(false));
    // 인라인 리뷰 댓글은 다른 id 공간이라 키가 겹치지 않는다.
    assert_eq!(by_key(&events, "gh:rcomment:101").unwrap().actor_kind, ActorKind::Human);
}

#[test]
fn 변경_요청은_gh_review와_gh_changes_requested_둘_다_내고_작성_중_리뷰는_건너뛴다() {
    let events = parsed("pr_authored").events;
    assert_eq!(by_key(&events, "gh:review:201").unwrap().event_type, EventType::GhReview);
    assert_eq!(by_key(&events, "gh:changes:201").unwrap().event_type, EventType::GhChangesRequested);
    assert!(by_key(&events, "gh:review:202").is_none());
}

#[test]
fn 승인_리뷰는_gh_review_approved를_함께_내고_코멘트_리뷰는_내지_않는다() {
    let review = |id: i64, state: &str| {
        json!({
            "databaseId": id, "state": state, "submittedAt": "2026-03-01T03:00:00Z", "bodyText": "합성",
            "url": format!("https://example.invalid/r/{id}"), "authorAssociation": "MEMBER",
            "author": { "__typename": "User", "login": "synthetic-alice" },
        })
    };
    let reviews = json!({ "reviews": { "nodes": [review(301, "APPROVED"), review(302, "COMMENTED")] } });
    let events = NodeParser::new().parse(&spread(&nodes()["pr_authored"], reviews)).unwrap().events;
    let approved = by_key(&events, "gh:approved:301").unwrap();
    assert_eq!(approved.event_type, EventType::GhReviewApproved);
    assert_eq!(approved.actor_id.as_deref(), Some("synthetic-alice"));
    assert!(by_key(&events, "gh:approved:302").is_none());
}

#[test]
fn 잘린_연결_보이는_항목이_since_이후일_때만_잘린_것이고_리뷰_스레드는_시각과_무관하다() {
    let since = Instant::parse("2026-03-01T00:00:00.000Z");
    let comments =
        |at: &str, hidden: bool| json!({ "pageInfo": { "hasPreviousPage": hidden }, "nodes": [{ "createdAt": at }] });
    let recent = comments("2026-03-01T01:00:00Z", true);
    assert_eq!(truncated_connections(&json!({ "comments": recent }), since), ["comments"]);
    let old = comments("2026-02-01T01:00:00Z", true);
    // 창 앞쪽이 이미 본 구간이면 잃는 것이 없다.
    assert!(truncated_connections(&json!({ "comments": old }), since).is_empty());
    let whole = comments("2026-03-01T01:00:00Z", false);
    assert!(truncated_connections(&json!({ "comments": whole }), since).is_empty());
    let threads = json!({ "reviewThreads": { "pageInfo": { "hasPreviousPage": true }, "nodes": [] } });
    assert_eq!(truncated_connections(&threads, since), ["reviewThreads"]);
    let thread_comments =
        json!({ "reviewThreads": { "pageInfo": { "hasPreviousPage": false }, "nodes": [{ "comments": recent }] } });
    assert_eq!(truncated_connections(&thread_comments, since), ["threadComments"]);
    // 첫 수집 이전은 baseline이다.
    assert!(truncated_connections(&json!({ "comments": recent }), None).is_empty());
}

#[test]
fn 저장_금지_이벤트에는_본문이_없고_text_head는_객체_제목이다() {
    for event in parsed("pr_authored").events {
        assert_eq!(event.text_head.as_deref(), Some("합성 알림 목록 개선"));
        assert_eq!(event.thread_key.as_deref(), Some("gh:example-org/widget#11"));
        assert!(!format!("{event:?}").contains("멘션 없는 사람 댓글"));
    }
}

#[test]
fn ci_실패는_실패한_체크의_완료_시각으로_낸다() {
    let events = parsed("pr_authored").events;
    let failed = events.iter().find(|event| event.event_type == EventType::GhCiFailed).unwrap();
    assert_eq!(failed.occurred_at, "2026-03-01T04:10:00.000Z");
    assert_eq!(failed.actor_kind, ActorKind::Bot);
    let at = Instant::parse("2026-03-01T04:10:00Z").unwrap().millis();
    assert_eq!(failed.dedupe_key, format!("gh:ci:aaaa1111:failed:{at}"));
}

#[test]
fn 내_pr이_성공이면_gh_ci_recovered_남의_pr과_병합된_pr은_ci_이벤트가_없다() {
    let mut mine = nodes()["pr_authored"].clone();
    mine["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"] = json!("SUCCESS");
    let events = NodeParser::new().parse(&mine).unwrap().events;
    let recovered = events.iter().find(|event| event.event_type == EventType::GhCiRecovered).unwrap();
    assert_eq!(recovered.occurred_at, "2026-03-01T04:10:00.000Z");
    let has_ci =
        |events: &[NormalizedEvent]| events.iter().any(|event| event.event_type.as_str().starts_with("gh_ci_"));
    assert!(!has_ci(&parsed("pr_review").events));
    assert!(!has_ci(&parsed("pr_merged").events));
}

#[test]
fn 리뷰_요청_첫_요청_내_리뷰_뒤의_재요청_다른_사람_요청_무시_요청_해제() {
    let ParsedNode { object, events } = parsed("pr_review");
    assert_eq!(object.review_requested_me, Some(true));
    assert_eq!(object.is_draft, Some(true));
    assert_eq!(by_key(&events, "gh:rr:RRE_1").unwrap().event_type, EventType::GhReviewRequested);
    assert_eq!(by_key(&events, "gh:rr:RRE_2").unwrap().event_type, EventType::GhReviewRerequested);
    assert!(by_key(&events, "gh:rr:RRE_3").is_none());
    assert_eq!(by_key(&events, "gh:rrr:RRR_1").unwrap().event_type, EventType::GhReviewRequestRemoved);
}

#[test]
fn 배정_내_배정과_해제만_본문_멘션은_gh_comment_마일스톤은_객체에_남는다() {
    let ParsedNode { object, events } = parsed("issue_assigned");
    assert_eq!(object.kind, ObjectKind::Issue);
    assert_eq!(object.assigned_to_me, Some(true));
    assert_eq!(object.milestone_title.as_deref(), Some("합성 마일스톤 알파"));
    assert_eq!(by_key(&events, "gh:assign:AE_1").unwrap().event_type, EventType::GhAssigned);
    assert!(by_key(&events, "gh:assign:AE_2").is_none());
    assert_eq!(by_key(&events, "gh:unassign:UE_1").unwrap().event_type, EventType::GhUnassigned);
    assert_eq!(by_key(&events, "gh:body:I_synth_c").unwrap().mentions_me, Some(true));
}

#[test]
fn 병합과_닫힘은_각자의_시각으로_이벤트를_낸다() {
    let merged = parsed("pr_merged");
    assert_eq!(merged.object.state.as_deref(), Some("merged"));
    assert_eq!(by_key(&merged.events, "gh:merged:PR_synth_d").unwrap().event_type, EventType::GhMerged);
    let closed = parsed("issue_closed");
    assert_eq!(closed.object.state.as_deref(), Some("closed"));
    assert_eq!(by_key(&closed.events, "gh:closed:I_synth_e").unwrap().occurred_at, "2026-03-01T10:00:00Z");
}

/// 원본 시험은 키로 찾아 보므로 순서를 고정하지 않는다. 이벤트 순서는 저장 순서라서 따로 시험한다.
/// 고정 자료의 기대값은 원본 `parseNode`의 출력이다. 시각을 일부러 뒤집어 둔 노드가 들어 있다.
#[test]
fn 이벤트_순서는_원본과_같다() {
    let text = include_str!("../../tests/fixtures/github/event_order.json").replace("__LOGIN__", self_login());
    let cases: Value = serde_json::from_str(&text).unwrap();
    for case in cases.as_array().unwrap() {
        let events = NodeParser::new().parse(&case["node"]).unwrap().events;
        let got: Vec<String> = events.iter().map(|e| format!("{} {}", e.event_type, e.dedupe_key)).collect();
        assert_eq!(json!(got), case["events"], "{}", case["node"]["id"]);
    }
}

// 탈퇴한 계정의 글은 author가 null로 온다. 멘션은 그대로 신호다.
#[test]
fn 작성자가_null이어도_본문_멘션과_댓글이_남는다() {
    let comment =
        json!({ "databaseId": 9, "createdAt": "2026-03-01T00:00:00Z", "bodyText": "@kairos-dev", "author": null });
    let patch = json!({ "author": null, "bodyText": "@kairos-dev 봐주세요", "comments": { "nodes": [comment] } });
    let events = NodeParser::new().parse(&spread(&nodes()["issue_assigned"], patch)).unwrap().events;
    let body = by_key(&events, "gh:body:I_synth_c").unwrap();
    assert_eq!((body.mentions_me, body.actor_id.as_deref()), (Some(true), None));
    assert_eq!(by_key(&events, "gh:comment:9").unwrap().mentions_me, Some(true));
}

#[test]
fn 해석할_수_없는_노드는_none이다() {
    let parser = NodeParser::new();
    assert_eq!(parser.parse(&Value::Null), None);
    assert_eq!(parser.parse(&json!({ "__typename": "Discussion" })), None);
    assert_eq!(parser.parse(&json!({ "__typename": "Issue", "id": "x" })), None);
}
