use std::sync::LazyLock;

use kairos_core::model::{ActorKind, NOTION_SENDER};
use regex::Regex;

use crate::rule::{RuleEvent, RuleId};

// JS의 `/i`(u 없음)는 ASCII 글자끼리만 대소문자를 접는다. `(?i)`는 켈빈 기호까지 맞으므로 `-u`를 함께 쓴다.
static TESTFLIGHT: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i-u:testflight)").expect("상수 정규식"));
static APP_REVIEW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("(?i-u:app review|review of your|심사)").expect("상수 정규식"));
static READY_FOR_DEV: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i-u:ready for dev)").expect("상수 정규식"));
static FIREBASE: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i-u:firebase)").expect("상수 정규식"));
static INVITATION: LazyLock<Regex> = LazyLock::new(|| Regex::new("(?i-u:^(invitation|초대):)").expect("상수 정규식"));
static NO_REPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new("no-?reply").expect("상수 정규식"));

/// 주소의 도메인이 후보 중 하나이거나 그 하위 도메인이다.
fn is_from(domain: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| domain == *candidate || domain.strip_suffix(candidate).is_some_and(|rest| rest.ends_with('.')))
}

/// 발신자 도메인과 제목을 함께 본다. 도메인 목록은 가정이고 단계 1 기록으로 보정한다.
pub(super) fn rule(event: &RuleEvent) -> RuleId {
    if event.actor_kind == ActorKind::SelfActor {
        return RuleId::NoiseSelf;
    }
    let sender = event.actor_id.as_deref().unwrap_or("").to_lowercase();
    let domain = sender.rfind('@').map_or(sender.as_str(), |at| &sender[at + 1..]);
    let subject = event.text_head.as_deref().unwrap_or("");
    // 제목은 누구나 쓸 수 있다. 제목 판정은 자동 발신자에게만 적용해 사람 메일이 기록에서 사라지지 않게 한다.
    let automated = NO_REPLY.is_match(&sender) || event.actor_kind == ActorKind::Bot;
    // Gmail 검색이 이미 이 발신자를 빼므로 여기는 검색이 어긋나 들어온 메일을 기록만 남기는 안전망이다.
    if sender == NOTION_SENDER {
        return RuleId::NoiseNotionSource;
    }

    if is_from(domain, &["apple.com"]) && TESTFLIGHT.is_match(subject) {
        return RuleId::NoiseBuildMail;
    }
    if is_from(domain, &["apple.com"]) && APP_REVIEW.is_match(subject) {
        return RuleId::P1AppReviewMail;
    }
    if is_from(domain, &["sentry.io", "getsentry.com"]) {
        return RuleId::P1SentryMail;
    }
    if is_from(domain, &["figma.com"]) && READY_FOR_DEV.is_match(subject) {
        return RuleId::P2FigmaReady;
    }
    if is_from(domain, &["firebase.google.com", "firebase.com"]) || (automated && FIREBASE.is_match(subject)) {
        return RuleId::NoiseBuildMail;
    }
    if sender.starts_with("calendar-notification@") || (automated && INVITATION.is_match(subject)) {
        return RuleId::NoiseCalendarInvite;
    }
    if automated { RuleId::NoiseNoReply } else { RuleId::Unclassified }
}
