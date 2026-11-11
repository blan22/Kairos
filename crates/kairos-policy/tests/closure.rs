//! 종료 판정.
#![allow(clippy::unwrap_used)]

use kairos_core::Instant;
use kairos_core::model::{ActorKind, EventType, ObjectKind, Outcome};
use kairos_policy::{
    CloseReason, ClosureDecision, ClosureInput, ClosureJudge, EvidenceEvent, OpenedEvent, RuleId, RuleObject,
};

const DEFAULT_NOW: &str = "2026-03-02T03:00:00.000Z";

fn evidence(event_type: EventType) -> EvidenceEvent {
    EvidenceEvent {
        dedupe_key: "evidence".into(),
        event_type,
        actor_kind: ActorKind::SelfActor,
        actor_id: None,
        thread_key: "thread-1".into(),
        occurred_at: "2026-03-02T02:00:00.000Z".into(),
    }
}

fn opened(rule_id: &str) -> OpenedEvent {
    OpenedEvent {
        rule_id: rule_id.into(),
        thread_key: "thread-1".into(),
        occurred_at: "2026-03-02T01:00:00.000Z".into(),
        actor_id: None,
    }
}

fn judged(opened: &OpenedEvent, candidates: &[EvidenceEvent]) -> ClosureDecision {
    judged_at(opened, candidates, None, DEFAULT_NOW)
}

fn judged_at(
    opened: &OpenedEvent,
    candidates: &[EvidenceEvent],
    object: Option<&RuleObject>,
    now: &str,
) -> ClosureDecision {
    let now = Instant::parse(now).unwrap();
    ClosureJudge::judge(&ClosureInput { run_outcome: Outcome::Ok, opened, candidates, object, now })
}

fn closed(reason: CloseReason) -> ClosureDecision {
    ClosureDecision::Close { reason, evidence: Some("evidence".into()) }
}

fn expired() -> ClosureDecision {
    ClosureDecision::Close { reason: CloseReason::Expired, evidence: None }
}

fn schedule(all_day: bool, end_date: Option<&str>, start_at: Option<&str>, end_at: Option<&str>) -> RuleObject {
    RuleObject {
        kind: ObjectKind::Meeting,
        state: None,
        all_day,
        start_date: None,
        end_date: end_date.map(Into::into),
        start_at: start_at.map(Into::into),
        end_at: end_at.map(Into::into),
        my_response: None,
        busy: None,
        assigned_to_me: false,
        authored_by_me: false,
    }
}

#[test]
fn 근거_없는_종료_불가_어떤_규칙도_증거_없이_닫히지_않는다() {
    for rule_id in RuleId::ALL {
        assert_eq!(judged(&opened(rule_id.as_str()), &[]), ClosureDecision::KeepOpen, "{rule_id}");
    }
}

#[test]
fn 근거_없는_종료_불가_ok가_아닌_실행은_증거가_있어도_닫지_못한다() {
    let candidates = [evidence(EventType::GhComment)];
    let opened = opened("p1.direct_mention");
    let now = Instant::parse(DEFAULT_NOW).unwrap();
    for outcome in Outcome::ALL.iter().copied().filter(|outcome| *outcome != Outcome::Ok) {
        let input = ClosureInput { run_outcome: outcome, opened: &opened, candidates: &candidates, object: None, now };
        assert_eq!(ClosureJudge::judge(&input), ClosureDecision::KeepOpen, "{outcome}");
    }
}

#[test]
fn 근거_없는_종료_불가_더_이른_증거_다른_스레드_다른_사람은_닫지_못한다() {
    let earlier = EvidenceEvent { occurred_at: "2026-03-02T00:00:00.000Z".into(), ..evidence(EventType::GhComment) };
    let same = EvidenceEvent { occurred_at: "2026-03-02T01:00:00.000Z".into(), ..evidence(EventType::GhComment) };
    let other_thread = EvidenceEvent { thread_key: "thread-2".into(), ..evidence(EventType::GhComment) };
    let human = EvidenceEvent { actor_kind: ActorKind::Human, ..evidence(EventType::GhComment) };
    for item in [earlier, same, other_thread, human] {
        assert_eq!(judged(&opened("p1.direct_mention"), &[item]), ClosureDecision::KeepOpen);
    }
}

#[test]
fn work_owed는_어떤_이벤트로도_닫히지_않는다() {
    let candidates = [evidence(EventType::GhClosed), evidence(EventType::GhComment), evidence(EventType::GhMerged)];
    assert_eq!(judged(&opened("p1.sentry_mail"), &candidates), ClosureDecision::KeepOpen);
}

#[test]
fn 종료_규칙_표() {
    use ActorKind::{Bot, Human, SelfActor};
    use CloseReason::{DoneEvidence, Withdrawn};
    let cases = [
        ("p1.direct_mention", EventType::GhComment, SelfActor, DoneEvidence),
        ("p1.direct_mention", EventType::SlackMessage, SelfActor, DoneEvidence),
        ("p2.review_requested", EventType::GhReview, SelfActor, DoneEvidence),
        ("p2.review_requested", EventType::GhReviewRequestRemoved, Human, Withdrawn),
        ("p2.review_requested", EventType::GhMerged, Human, DoneEvidence),
        ("p2.assigned", EventType::GhClosed, Human, DoneEvidence),
        ("p2.assigned", EventType::GhUnassigned, Human, Withdrawn),
        ("p1.ci_failed", EventType::GhCiRecovered, Bot, DoneEvidence),
        ("p1.changes_requested", EventType::GhChangesCleared, Human, DoneEvidence),
        ("p1.changes_requested", EventType::GhMerged, Human, DoneEvidence),
        ("p0.meeting_imminent", EventType::ObjectWithdrawn, Human, Withdrawn),
        ("p0.meeting_imminent", EventType::CalCancelled, Human, Withdrawn),
        ("p0.deploy_day", EventType::DeployCleared, Bot, Withdrawn),
    ];
    for (rule_id, event_type, actor_kind, reason) in cases {
        let candidate = EvidenceEvent { actor_kind, ..evidence(event_type) };
        assert_eq!(judged(&opened(rule_id), &[candidate]), closed(reason), "{rule_id} {event_type}");
    }
}

#[test]
fn ci_실패는_사람_리뷰로_닫히지_않고_변경_요청은_ci_회복으로_닫히지_않는다() {
    let review = [EvidenceEvent { actor_kind: ActorKind::Human, ..evidence(EventType::GhReview) }];
    assert_eq!(judged(&opened("p1.ci_failed"), &review), ClosureDecision::KeepOpen);
    let recovered = [EvidenceEvent { actor_kind: ActorKind::Bot, ..evidence(EventType::GhCiRecovered) }];
    assert_eq!(judged(&opened("p1.changes_requested"), &recovered), ClosureDecision::KeepOpen);
}

#[test]
fn 변경_요청은_코멘트_리뷰와_다른_사람의_승인으로_닫히지_않고_같은_리뷰어의_승인으로_닫힌다() {
    let opened = OpenedEvent { actor_id: Some("alice".into()), ..opened("p1.changes_requested") };
    let by = |event_type, actor: &str| EvidenceEvent {
        actor_kind: ActorKind::Human,
        actor_id: Some(actor.into()),
        ..evidence(event_type)
    };
    let judge_with = |candidate: EvidenceEvent| judged(&opened, &[candidate]);
    assert_eq!(judge_with(by(EventType::GhReview, "bob")), ClosureDecision::KeepOpen);
    assert_eq!(judge_with(by(EventType::GhReview, "alice")), ClosureDecision::KeepOpen);
    assert_eq!(judge_with(by(EventType::GhComment, "alice")), ClosureDecision::KeepOpen);
    assert_eq!(judge_with(by(EventType::GhReviewApproved, "bob")), ClosureDecision::KeepOpen);
    assert_eq!(judge_with(by(EventType::GhReviewApproved, "alice")), closed(CloseReason::DoneEvidence));
}

#[test]
fn 시각_경과_종료는_일정에만_있고_종일_일정은_끝_날짜로_판단한다() {
    let ended = schedule(false, None, Some("2026-03-02T10:00:00+09:00"), Some("2026-03-02T11:00:00+09:00"));
    assert_eq!(judged_at(&opened("p0.meeting_imminent"), &[], Some(&ended), DEFAULT_NOW), expired());
    assert_eq!(judged_at(&opened("p1.direct_mention"), &[], Some(&ended), DEFAULT_NOW), ClosureDecision::KeepOpen);

    let all_day = schedule(true, Some("2026-11-06"), None, None);
    let deploy = opened("p0.deploy_day");
    assert_eq!(judged_at(&deploy, &[], Some(&all_day), "2026-11-05T14:59:00Z"), ClosureDecision::KeepOpen);
    assert_eq!(judged_at(&deploy, &[], Some(&all_day), "2026-11-05T15:00:00Z"), expired());
}

// 아래는 원본에 없는 시험이다. NaN 비교의 의미를 고정한다.

#[test]
fn 시각을_해석할_수_없는_증거는_건너뛰지_않는다() {
    let candidate = EvidenceEvent { occurred_at: "a".into(), ..evidence(EventType::GhComment) };
    assert_eq!(judged(&opened("p1.direct_mention"), &[candidate]), closed(CloseReason::DoneEvidence));
    let unknown_opened = OpenedEvent { occurred_at: "a".into(), ..opened("p1.direct_mention") };
    assert_eq!(judged(&unknown_opened, &[evidence(EventType::GhComment)]), closed(CloseReason::DoneEvidence));
}

#[test]
fn 끝을_해석할_수_없는_일정은_끝나지_않는다() {
    let now = Instant::parse(DEFAULT_NOW).unwrap();
    assert!(!ClosureJudge::has_ended(None, now));
    assert!(!ClosureJudge::has_ended(Some(&schedule(false, None, None, None)), now));
    assert!(!ClosureJudge::has_ended(Some(&schedule(false, None, Some("a"), Some("b"))), now));
    assert!(!ClosureJudge::has_ended(Some(&schedule(true, None, None, None)), now));
    let start_only = schedule(false, None, Some("2026-03-02T00:00:00Z"), None);
    assert!(ClosureJudge::has_ended(Some(&start_only), now));
}
