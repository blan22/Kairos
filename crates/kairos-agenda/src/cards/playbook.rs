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
/// GitHub 조직 이름은 대소문자를 가리지 않고, 설정은 소문자로 바꿔 둔다. 주소는 GitHub가 준 대소문자 그대로라 조직 부분만 대소문자를 무시한다.
/// 유니코드 접기(`ſ`, 켈빈 기호 등)로 ASCII 밖 글자가 통과하지 않게 ASCII 대소문자만 무시한다(`-u`).
static ISSUE_URL: LazyLock<Regex> = LazyLock::new(|| {
    let owners: Vec<String> =
        kairos_core::profile::current().playbooks.owners().into_iter().map(regex::escape).collect();
    if owners.is_empty() {
        return Regex::new(r"[^\s\S]").expect("상수 정규식");
    }
    Regex::new(&format!(r"^https://github\.com/(?i-u:{})/[A-Za-z0-9_.-]+/(?:pull|issues)/[0-9]+$", owners.join("|")))
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

/// GitHub 이슈와 PR 주소의 모양. 매핑표 밖 조직도 받는다. 제안이 없는 까닭에 저장소 이름을 쓸 때만 쓰고, 세션 입력에는 쓰지 않는다.
static GITHUB_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https://github\.com/([A-Za-z0-9][A-Za-z0-9-]{0,38})/([A-Za-z0-9_.-]{1,100})/(?:pull|issues)/[0-9]+$")
        .expect("상수 정규식")
});

/// 주소에서 `owner/name`을 뽑는다. 모양이 틀리거나 이름이 `.`, `..`이면 None이다.
pub fn github_repo_of(url: &str) -> Option<String> {
    let found = GITHUB_URL.captures(url)?;
    let (owner, name) = (found.get(1)?.as_str(), found.get(2)?.as_str());
    (name != "." && name != "..").then(|| format!("{owner}/{name}"))
}

string_enum! {
    /// 에이전트 제안이 없는 까닭. 고칠 수 없는 조건을 먼저 보고, 설정으로 풀 수 있는 저장소 조건(`unmapped`)은 뒤에 본다.
    pub enum NoOffer {
        /// 미뤄 둔 일이다. 다시 올라오면 제안한다.
        Parked => "parked",
        /// 이슈나 PR이 닫혔거나 병합됐다.
        Closed => "closed",
        /// 이 사유와 종류의 일을 맡는 플레이북이 없다(내 PR의 CI, PR 답장 같은 일).
        Unsupported => "unsupported",
        /// 연 사람이 본인(T0)이나 조직의 사람(T1)이 아니다.
        UntrustedAuthor => "untrusted_author",
        /// 조직 밖 사람의 글을 본 적이 있다.
        Outsider => "outsider",
        /// 플레이북 대상 저장소가 아니다. 사용자가 `config.toml`의 `[[playbooks]]`로 더할 수 있다.
        Unmapped => "unmapped",
        /// 같은 이슈에 이미 세션이 있다.
        Session => "session",
        /// 같은 이슈의 다른 항목에 제안이 붙어 있다.
        Sibling => "sibling",
    }
}

/// 제안이 없는 GitHub 이슈와 PR의 까닭. 판정은 `playbook_of`의 조건을 같은 객체에서 다시 본다. 제안이 있어야 하는데 없다면
/// 세션이나 한 이슈 한 버튼 조건이 막은 것이다. 이슈와 PR이 아니거나 열린 항목이 아니면 None이다.
pub fn no_offer_of(
    connection: &Connection,
    reason: ItemReason,
    state: ItemState,
    object_key: &str,
    object: Option<&ObjectRow>,
) -> KairosResult<Option<NoOffer>> {
    let Some(object) = object.filter(|object| matches!(object.kind, ObjectKind::Issue | ObjectKind::PullRequest))
    else {
        return Ok(None);
    };
    match state {
        ItemState::Parked => return Ok(Some(NoOffer::Parked)),
        ItemState::Open => {}
        ItemState::Accepted | ItemState::Closed => return Ok(None),
    }
    if object.state.as_deref() != Some("open") {
        return Ok(Some(NoOffer::Closed));
    }
    if Playbook::choose(reason, object).is_none() {
        return Ok(Some(NoOffer::Unsupported));
    }
    if !matches!(object.trust_tier, Some(TrustTier::T0 | TrustTier::T1)) {
        return Ok(Some(NoOffer::UntrustedAuthor));
    }
    if EventRepo::new(connection).has_outsider_on_object(object_key)? {
        return Ok(Some(NoOffer::Outsider));
    }
    let mapped = object
        .permalink
        .as_deref()
        .filter(|url| ISSUE_URL.is_match(url))
        .and_then(|url| kairos_core::profile::current().playbooks.target_for_url(url));
    if mapped.is_none() {
        return Ok(Some(NoOffer::Unmapped));
    }
    let running = crate::repo::ReadOnlyRunRepo::new(connection).active_on_object(object_key)?;
    Ok(Some(if running.is_some() { NoOffer::Session } else { NoOffer::Sibling }))
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
