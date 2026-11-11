//! 플레이북의 조건과 카드의 고정 문구. 어떤 항목을 세션에 맡길 수 있는지는 여기서만 정한다.

use std::sync::LazyLock;

use kairos_core::KairosResult;
use kairos_core::js;
use kairos_core::model::{ObjectKind, SourceName, TrustTier};
use kairos_core::string_enum;
use kairos_core::text::sanitize_text;
use kairos_ledger::{EventRepo, ObjectRow};
use kairos_policy::ItemReason;
use kairos_store::Connection;
use regex::Regex;

use crate::item::ItemState;

/// 세션 프롬프트에 들어갈 수 있는 유일한 외부 값의 모양이다. 수락 쪽은 이 정규식을 다시 검사한다.
/// 조직은 프로필의 플레이북 대상(설계 7.2의 매핑표)에서 오고 이스케이프한다. 대상이 없으면 어떤 URL도 받지 않는다.
static ISSUE_URL: LazyLock<Regex> = LazyLock::new(|| {
    let owners: Vec<String> =
        kairos_core::profile::current().playbooks.owners().into_iter().map(regex::escape).collect();
    if owners.is_empty() {
        return Regex::new(r"[^\s\S]").expect("상수 정규식");
    }
    Regex::new(&format!(r"^https://github\.com/(?:{})/[A-Za-z0-9_.-]+/(?:pull|issues)/[0-9]+$", owners.join("|")))
        .expect("이스케이프한 조직의 정규식")
});

pub fn issue_url_matches(url: &str) -> bool {
    ISSUE_URL.is_match(url)
}

const URL_LIMIT: usize = 512;
static SAFE_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^https://[^{}]+$", js::WS)).expect("상수 정규식"));

string_enum! {
    /// 세션에 맡길 수 있는 일. qa_issue는 작업 공간에서 고치고 로컬에 커밋하는 프로필, 나머지는 읽는 프로필로 돈다. 외부에 쓰는 플레이북은 없다.
    pub enum Playbook {
        /// 내 담당 이슈의 원인 조사, 수정 계획, 수정과 로컬 커밋
        QaIssue => "qa_issue",
        /// 나에게 리뷰를 요청한 PR의 리뷰 의견 초안
        PrReview => "pr_review",
        /// 답장이 필요한 이슈 글의 답변 초안. PR은 뺐다. 인라인 리뷰 글을 세션이 읽지 못하고 내용 없는 승인도 답장 필요가 된다.
        ReplyDraft => "reply_draft",
    }
}

impl Playbook {
    /// 항목이 왜 열렸고 객체가 무엇인지로 고른다. 담당 이슈는 멘션으로 열려도 조사가 답변보다 넓어 먼저 본다.
    fn choose(reason: ItemReason, object: &ObjectRow) -> Option<Self> {
        match (object.kind, reason) {
            (ObjectKind::Issue, _) if object.assigned_to_me == Some(true) => Some(Self::QaIssue),
            (ObjectKind::Issue, ItemReason::ReplyOwed) => Some(Self::ReplyDraft),
            (ObjectKind::PullRequest, ItemReason::ReviewOwed) => Some(Self::PrReview),
            _ => None,
        }
    }

    /// 패널이 그대로 그리는 제안 문구. 무엇을 써 두고 어디서 멈추는지가 사용자가 누르는 범위다.
    pub const fn scope(self) -> &'static str {
        match self {
            Self::QaIssue => {
                "원인을 조사해 계획을 쓰고 코드를 고쳐 로컬에 커밋합니다. push, PR, 코멘트는 하지 않습니다."
            }
            Self::PrReview => "변경을 읽고 리뷰 의견 초안을 써 둡니다. 코드 변경, 코멘트, 승인은 하지 않습니다.",
            Self::ReplyDraft => "글을 읽고 답변 초안을 써 둡니다. 코드 변경과 코멘트는 하지 않습니다.",
        }
    }
}

pub const fn reason_label(reason: ItemReason) -> &'static str {
    match reason {
        ItemReason::ReplyOwed => "답장 필요",
        ItemReason::ReviewOwed => "리뷰 요청",
        ItemReason::Assigned => "담당 이슈",
        ItemReason::MyPrAttention => "내 PR 확인",
        ItemReason::MeetingNew => "일정 변경",
        ItemReason::DeployRisk => "배포 위험",
        ItemReason::WorkOwed => "할 일",
        ItemReason::Manual => "직접 추가",
    }
}

pub fn safe_url(value: Option<&str>) -> Option<String> {
    let url = sanitize_text(value.unwrap_or(""), URL_LIMIT);
    SAFE_URL.is_match(&url).then_some(url)
}

pub fn label_of(source: Option<&str>) -> &'static str {
    source.and_then(|source| source.parse::<SourceName>().ok()).map_or("kairos", SourceName::label)
}

/// 객체 조건: T0나 T1이 연 열린 이슈나 PR이고 매핑표에 있으며 URL 정규식을 통과한다. 어떤 일인지는 항목 이유로 고른다.
pub fn playbook_for(reason: ItemReason, state: ItemState, object: Option<&ObjectRow>) -> Option<Playbook> {
    let object = object.filter(|_| state == ItemState::Open)?;
    if object.state.as_deref() != Some("open") {
        return None;
    }
    if !matches!(object.trust_tier, Some(TrustTier::T0 | TrustTier::T1)) {
        return None;
    }
    let url = object.permalink.as_deref().filter(|url| ISSUE_URL.is_match(url))?;
    kairos_core::profile::current().playbooks.target_for_url(url)?;
    Playbook::choose(reason, object)
}

/// 수락과 kairos-run이 다시 보는 조건. 시작 버튼은 여기에 한 이슈 한 버튼 조건을 더한다.
///
/// 세션은 이슈의 댓글을 읽으므로 T2, T3 사람의 글이 있으면 세션 입력이 된다(설계 8절). 항목을 연 사람의 글도 같은 객체에 있어 이 조건에 든다.
/// 이벤트가 없는 등급은 모르는 것이라 믿지 않는다. 봇 글은 이슈마다 붙고 사람의 지시가 아니어서 세지 않는다.
/// 한 번 본 외부 글은 보존 기간과 정비가 지나도 `outsider_marks`로 남아 계속 막는다. 30일이 지나 이벤트로 저장하지 않은 글도 같다.
/// 이 판정은 DB만 본다. 처음 읽을 때 최근 20개 밖에 있던 글과 마지막 수집 뒤의 글은 모르므로,
/// kairos-run이 claude를 띄우기 직전에 GitHub에서 글 전체를 다시 읽어 같은 기준으로 본다(`LiveAuthorCheck`).
pub fn playbook_of(
    connection: &Connection,
    reason: ItemReason,
    state: ItemState,
    object_key: &str,
    object: Option<&ObjectRow>,
) -> KairosResult<Option<Playbook>> {
    if EventRepo::new(connection).has_outsider_on_object(object_key)? {
        return Ok(None);
    }
    Ok(playbook_for(reason, state, object))
}
