//! 처분과 근거 있는 종료, 변경 요청.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, NormalizedObject, Outcome, SourceName, SourceResult};
use kairos_ledger::check_invariants;
use kairos_store::Database;

fn by(id: &str, at: &str, actor_kind: ActorKind) -> NormalizedEvent {
    NormalizedEvent { actor_kind, ..comment(id, at) }
}

#[test]
fn 처분_모든_이벤트가_규칙을_거쳐_기록된다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let bot = NormalizedEvent { actor_id: Some("ci[bot]".into()), ..by("bot", T1, ActorKind::Bot) };
    let events = vec![comment("human", T1), bot, by("me", T1, ActorKind::SelfActor)];
    ingest(&db, SourceResult { events, ..later() }, T1);
    assert_eq!(event_by_key(&db, "gh:comment:human").rule_id, "p2.owned_comment");
    assert_eq!(event_by_key(&db, "gh:comment:bot").rule_id, "noise.bot");
    assert_eq!(event_by_key(&db, "gh:comment:me").rule_id, "noise.self");
    assert_eq!(check_invariants(db.connection()).unwrap().events_without_disposition, 0);
}

#[test]
fn 멘션_없는_내_객체_댓글_17건의_합성_재생이_전부_opened나_folded다() {
    let db = test_db();
    let numbers: Vec<i64> = (1..=17).collect();
    seed(&db, SourceResult { objects: numbers.iter().copied().map(issue).collect(), ..result() });
    let events = numbers
        .iter()
        .map(|number| NormalizedEvent {
            object_key: Some(format!("gh:acme/widget#{}", number % 5 + 1)),
            ..comment(&format!("replay-{number}"), T1)
        })
        .collect();
    ingest(&db, SourceResult { events, ..later() }, T1);
    let stored: Vec<_> = all_events(&db).into_iter().filter(|event| event.dedupe_key.contains("replay")).collect();
    assert_eq!(stored.len(), 17);
    assert!(stored.iter().all(|event| event.disposition == "opened" || event.disposition == "folded"));
}

#[test]
fn 내_객체_댓글은_접히고_내가_답한_뒤의_댓글은_새로_열린다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let events = vec![
        comment("a", T1),
        comment("b", T2),
        by("me", "2026-03-02T01:07:00.000Z", ActorKind::SelfActor),
        comment("c", T3),
    ];
    ingest(&db, SourceResult { events, ..later() }, T3);
    assert_eq!(event_by_key(&db, "gh:comment:a").disposition, "opened");
    assert_eq!(event_by_key(&db, "gh:comment:b").disposition, "folded");
    assert_eq!(event_by_key(&db, "gh:comment:c").disposition, "opened");
    assert_eq!(event_by_key(&db, "gh:comment:a").close_reason.as_deref(), Some("done_evidence"));
    assert_eq!(event_by_key(&db, "gh:comment:c").close_reason, None);
}

#[test]
fn 근거_없는_종료_불가_결과에서_사라져도_닫히지_않는다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1), issue(2)], ..result() });
    ingest(
        &db,
        SourceResult {
            objects: vec![issue(1), issue(2)],
            events: vec![comment("open", T1)],
            snapshot: true,
            ..later()
        },
        T1,
    );
    for when in [T2, T3] {
        ingest(&db, SourceResult { objects: vec![issue(2)], snapshot: true, ..result_at(when) }, when);
    }
    assert_eq!(object_row(&db, "gh:acme/widget#1").missing_runs, 2);
    assert_eq!(event_by_key(&db, "gh:comment:open").close_reason, None);
    assert_eq!(would_show_events(&db).len(), 1);
}

#[test]
fn 근거_없는_종료_불가_증거가_있어도_partial_실행은_닫지_못하고_다음_ok가_닫는다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    ingest(&db, SourceResult { events: vec![comment("ask", T1)], ..later() }, T1);
    let reply = by("reply", T2, ActorKind::SelfActor);

    let partial = ingest(&db, SourceResult { outcome: Outcome::Partial, events: vec![reply.clone()], ..later() }, T2);
    assert_eq!(partial.closed, 0);
    assert_eq!(event_by_key(&db, "gh:comment:ask").close_reason, None);

    let retried = ingest(&db, SourceResult { events: vec![reply], watermark: Some(T3.into()), ..later() }, T3);
    assert_eq!(retried.closed, 1);
    let closed = event_by_key(&db, "gh:comment:ask");
    assert_eq!(closed.close_reason.as_deref(), Some("done_evidence"));
    assert_eq!(closed.close_evidence_event.as_deref(), Some("gh:comment:reply"));
    assert_eq!(check_invariants(db.connection()).unwrap().closures_without_evidence, 0);
}

#[test]
fn 종료는_스레드_단위다() {
    let db = test_db();
    seed(&db, SourceResult { objects: vec![issue(1)], ..result() });
    let ask = NormalizedEvent {
        thread_key: Some("thread-a".into()),
        mentions_me: Some(true),
        mention_count: Some(1),
        ..comment("ask", T1)
    };
    let elsewhere =
        NormalizedEvent { thread_key: Some("thread-b".into()), ..by("elsewhere", T2, ActorKind::SelfActor) };
    ingest(&db, SourceResult { events: vec![ask, elsewhere], ..later() }, T2);
    assert_eq!(event_by_key(&db, "gh:comment:ask").close_reason, None);
}

fn changes_requested(at: &str, actor: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some(actor.into()),
        object_key: Some("gh:acme/widget#1".into()),
        ..NormalizedEvent::new(
            format!("gh:changes:{actor}"),
            SourceName::Github,
            EventType::GhChangesRequested,
            at,
            ActorKind::Human,
        )
    }
}

fn reviewed(event_type: EventType, id: &str, at: &str, actor: &str) -> NormalizedEvent {
    NormalizedEvent { dedupe_key: format!("gh:{event_type}:{id}"), event_type, ..changes_requested(at, actor) }
}

fn pending_review() -> NormalizedObject {
    NormalizedObject { review_decision: Some("CHANGES_REQUESTED".into()), ..pull_request(1) }
}

/// 변경 요청을 받은 내 PR을 만든다.
fn with_changes_requested(db: &Database) {
    seed(db, SourceResult { objects: vec![pending_review()], ..result() });
    let events = vec![changes_requested(T1, "alice")];
    ingest(db, SourceResult { objects: vec![pending_review()], events, ..later() }, T1);
}

#[test]
fn 변경_요청은_다른_사람의_코멘트_리뷰로_닫히지_않는다() {
    let db = test_db();
    with_changes_requested(&db);
    assert_eq!(event_by_key(&db, "gh:changes:alice").rule_id, "p1.changes_requested");

    let review = reviewed(EventType::GhReview, "11", T2, "bob");
    ingest(&db, SourceResult { objects: vec![pending_review()], events: vec![review], ..result_at(T2) }, T2);
    assert_eq!(event_by_key(&db, "gh:changes:alice").close_reason, None);
    assert!(would_show_events(&db).iter().any(|row| row.0 == "gh:changes:alice"));
}

#[test]
fn 변경_요청은_같은_리뷰어의_승인으로_닫힌다() {
    let db = test_db();
    with_changes_requested(&db);
    let approved = reviewed(EventType::GhReviewApproved, "12", T2, "alice");
    ingest(&db, SourceResult { objects: vec![pending_review()], events: vec![approved], ..result_at(T2) }, T2);
    let row = event_by_key(&db, "gh:changes:alice");
    assert_eq!(row.close_reason.as_deref(), Some("done_evidence"));
    assert_eq!(row.close_evidence_event.as_deref(), Some("gh:gh_review_approved:12"));
    assert_eq!(check_invariants(db.connection()).unwrap().closures_without_evidence, 0);
}

#[test]
fn review_decision이_변경_요청을_벗어나면_증거_이벤트로_닫히고_빈_값은_증거가_아니다() {
    let db = test_db();
    with_changes_requested(&db);
    let run = |when: &str, review_decision: Option<&str>| {
        let object = NormalizedObject { review_decision: review_decision.map(Into::into), ..pull_request(1) };
        ingest(&db, SourceResult { objects: vec![object], ..result_at(when) }, when);
    };
    run(T2, None);
    assert_eq!(event_by_key(&db, "gh:changes:alice").close_reason, None, "필수 리뷰가 없는 저장소는 값이 비어 있다");

    run(T3, Some("CHANGES_REQUESTED"));
    run("2026-03-02T01:12:00.000Z", Some("REVIEW_REQUIRED"));
    let row = event_by_key(&db, "gh:changes:alice");
    assert_eq!(row.close_reason.as_deref(), Some("done_evidence"));
    assert_eq!(event_by_key(&db, &row.close_evidence_event.unwrap()).event_type, "gh_changes_cleared");
    assert_eq!(check_invariants(db.connection()).unwrap().closures_without_evidence, 0);
}
