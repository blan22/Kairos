//! 펼친 줄과 에이전트 제안이 보여 주는 근거. 패널은 코드를 고정 문구로 바꿔 그린다.
//!
//! 외부 글(제목, 본문, 마일스톤 이름, 사람 이름)은 넣지 않는다. 값은 규칙 ID, 고정 사실 코드, 수, 시각,
//! 플레이북 URL 정규식을 통과한 저장소 이름, 프로필에서 검증한 작업 저장소와 기준 브랜치뿐이다.
//! golden이 지키는 `Presenter`의 출력은 바꾸지 않는다. CLI가 `--json` 응답을 만든 뒤 채운다(`sources`, `reveal`과 같다).

use std::sync::LazyLock;

use kairos_core::KairosResult;
use kairos_core::model::{ObjectKind, TrustTier};
use kairos_ledger::{EventHead, EventRepo, ObjectRepo, ObjectRow};
use kairos_store::Connection;
use regex::Regex;
use serde::Serialize;

use super::playbook::{Playbook, issue_url_matches};
use super::views::ItemView;
use crate::repo::ItemRepo;

/// 정책의 규칙 ID 모양. 저장된 문자열이라 다시 검사한다.
static RULE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9_]{1,16}\.[a-z0-9_]{1,40}$").expect("상수 정규식"));
/// 일정 시각의 모양. 패널은 ISO 시각만 읽는다.
static ISO_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2}(?:\.\d{1,3})?)?(?:Z|[+-]\d{2}:\d{2})$").expect("상수 정규식")
});

/// 제안이 나온 조건. 플레이북이 있으면 모두 참이다(`playbook_for`, `CardBuilder::offer_of`).
const OPEN: &str = "open";
const INSIDERS: &str = "insiders";
const MAPPED: &str = "mapped";
const NO_SESSION: &str = "no_session";

/// 근거 한 묶음. 키 순서가 JSON 순서다.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhyView {
    /// 항목을 연 규칙. 긴급, 오늘, 나머지 순으로 고른 이벤트의 규칙이다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// 이 항목에 이어진 소식 수. 항목을 연 소식도 센다.
    pub events: usize,
    /// 객체에서 읽은 사실 코드. 순서가 화면 순서다.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<&'static str>,
    /// 일정의 참석자 수.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attendees: Option<i64>,
    /// 일정이 시작하는 시각.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub starts_at: Option<String>,
    /// 에이전트 제안의 근거. 제안이 붙은 줄에만 있다.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<OfferWhy>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferWhy {
    /// 대상 저장소(`owner/name`). 플레이북 URL 정규식을 통과한 주소에서 뽑는다.
    pub repo: String,
    /// 세션이 일하는 저장소(`owner/name`)와 기준 브랜치. 프로필이 검증한 값이다.
    pub workspace: String,
    pub base: String,
    /// 제안이 나온 조건 코드. 순서가 화면 순서다.
    pub checks: Vec<&'static str>,
}

const fn flag(value: Option<bool>) -> bool {
    matches!(value, Some(true))
}

/// GitHub 이슈와 PR의 사실.
fn code_facts(object: &ObjectRow, facts: &mut Vec<&'static str>) {
    facts.push(if object.kind == ObjectKind::Issue { "issue" } else { "pull_request" });
    for (value, code) in [
        (object.assigned_to_me, "assigned_me"),
        (object.review_requested_me, "review_requested"),
        (object.authored_by_me, "authored_me"),
        (object.is_draft, "draft"),
    ] {
        if flag(value) {
            facts.push(code);
        }
    }
    match object.review_decision.as_deref() {
        Some("CHANGES_REQUESTED") => facts.push("changes_requested"),
        Some("APPROVED") => facts.push("approved"),
        _ => {}
    }
    match object.checks_state.as_deref() {
        Some("FAILURE" | "ERROR") => facts.push("checks_failed"),
        Some("PENDING" | "EXPECTED") => facts.push("checks_pending"),
        Some("SUCCESS") => facts.push("checks_passed"),
        _ => {}
    }
    if object.milestone_title.as_deref().is_some_and(|title| !title.is_empty()) {
        facts.push("milestone");
    }
}

/// 객체의 사실 코드. 모르는 값은 버린다.
fn facts_of(object: &ObjectRow) -> Vec<&'static str> {
    let mut facts = Vec::new();
    match object.kind {
        ObjectKind::Issue | ObjectKind::PullRequest => code_facts(object, &mut facts),
        ObjectKind::Meeting => {
            match object.my_response.as_deref() {
                Some("needsAction") => facts.push("response_needed"),
                Some("accepted") => facts.push("attending"),
                Some("tentative") => facts.push("tentative"),
                Some("declined") => facts.push("declined"),
                _ => {}
            }
            if flag(object.all_day) {
                facts.push("all_day");
            }
        }
        ObjectKind::DmConversation => facts.push("dm"),
        ObjectKind::MailThread => facts.push("mail"),
        _ => {}
    }
    facts
}

/// 카드의 제목을 고르는 순서와 같다. 긴급 규칙이 먼저이고, 없으면 오늘, 그다음 가장 최근 소식이다.
fn rule_of(heads: &[EventHead], opened: Option<&EventHead>) -> Option<String> {
    let first = heads
        .iter()
        .find(|head| head.rule_id.starts_with("p0."))
        .or_else(|| heads.iter().find(|head| head.rule_id.starts_with("p1.")))
        .or_else(|| heads.first())
        .or(opened)?;
    RULE.is_match(&first.rule_id).then(|| first.rule_id.clone())
}

/// `https://github.com/<owner>/<name>/(pull|issues)/<n>`에서 `owner/name`을 뽑는다. 정규식을 통과한 주소만 받는다.
fn repo_of(url: &str) -> Option<String> {
    if !issue_url_matches(url) {
        return None;
    }
    let mut parts = url.strip_prefix("https://github.com/")?.split('/');
    Some(format!("{}/{}", parts.next()?, parts.next()?))
}

/// 플레이북마다 사유 조건이 다르다(`Playbook::choose`).
const fn reason_check(playbook: Playbook) -> &'static str {
    match playbook {
        Playbook::QaIssue => "assigned_me",
        Playbook::PrReview => "review_requested",
        Playbook::ReplyDraft => "reply_owed",
    }
}

fn offer_of(playbook: Playbook, object: &ObjectRow) -> Option<OfferWhy> {
    let url = object.permalink.as_deref()?;
    let repo = repo_of(url)?;
    let target = kairos_core::profile::current().playbooks.target_for_url(url)?;
    // 등급 조건은 제안을 낼 때 이미 통과했다. 그래도 같은 객체에서 다시 보고 아니면 낱말을 내지 않는다.
    let insiders = matches!(object.trust_tier, Some(TrustTier::T0 | TrustTier::T1));
    let checks = [reason_check(playbook), OPEN, MAPPED]
        .into_iter()
        .chain(insiders.then_some(INSIDERS))
        .chain([NO_SESSION])
        .collect();
    Some(OfferWhy { repo, workspace: target.workspace_repo.clone(), base: target.base_branch.clone(), checks })
}

/// 항목 뷰에 근거를 채운다. 연결이 읽기 전용이어도 된다.
pub struct Explainer<'a> {
    connection: &'a Connection,
}

impl<'a> Explainer<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub fn why_of(&self, view: &ItemView) -> KairosResult<Option<WhyView>> {
        let Some(item) = ItemRepo::new(self.connection).get(view.id)? else {
            return Ok(None);
        };
        let events = EventRepo::new(self.connection);
        let heads = events.heads_of_item(item.id)?;
        let opened = match item.opened_by_event.as_deref() {
            Some(key) if heads.is_empty() => events.head_by_key(key)?,
            _ => None,
        };
        let object = ObjectRepo::new(self.connection).get(&item.object_key)?;
        // 이어진 소식이 없어도 항목을 연 소식은 하나다.
        let count = heads.len().max(usize::from(opened.is_some()));
        let mut why = WhyView { rule: rule_of(&heads, opened.as_ref()), events: count, ..WhyView::default() };
        if let Some(object) = object.as_ref() {
            why.facts = facts_of(object);
            if object.kind == ObjectKind::Meeting {
                why.attendees = object.attendee_count.filter(|count| *count > 0);
                why.starts_at = object.start_at.clone().filter(|at| ISO_TIME.is_match(at));
            }
            why.offer = view.playbook.and_then(|playbook| offer_of(playbook, object));
        }
        Ok(Some(why))
    }

    /// 뷰마다 근거를 채운다. 항목이 사라진 뷰는 비워 둔다.
    pub fn fill<'v>(&self, views: impl IntoIterator<Item = &'v mut ItemView>) -> KairosResult<()> {
        for view in views {
            view.why = self.why_of(view)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(rule: &str) -> EventHead {
        EventHead { rule_id: rule.to_owned(), text_head: None, permalink: None }
    }

    #[test]
    fn 규칙은_긴급_오늘_최근_순으로_고른다() {
        let heads = [head("p2.owned_comment"), head("p1.ci_failed"), head("p0.deploy_day")];
        assert_eq!(rule_of(&heads, None).as_deref(), Some("p0.deploy_day"));
        assert_eq!(rule_of(&heads[..2], None).as_deref(), Some("p1.ci_failed"));
        assert_eq!(rule_of(&heads[..1], None).as_deref(), Some("p2.owned_comment"));
        assert_eq!(rule_of(&[], Some(&head("p2.assigned"))).as_deref(), Some("p2.assigned"));
    }

    #[test]
    fn 모양이_틀린_규칙은_내지_않는다() {
        assert_eq!(rule_of(&[head("p1.<img>")], None), None);
        assert_eq!(rule_of(&[head("")], None), None);
    }
}
