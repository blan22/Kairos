use super::{ActorKind, EventType, ObjectKind, SourceName, TrustTier};

/// 객체 스냅샷. 담당, 내 PR, 리뷰 대기, 일정을 나타낸다. 제안의 입력이 아니라 보드 조회와 판정 근거다.
///
/// object_key 규칙 (설계 3.1)
/// - 일정 `gcal:<event id>` (시작 시각을 넣지 않는다. 그래야 이동이 같은 객체의 변경으로 잡힌다)
/// - GitHub `gh:<owner>/<repo>#<number>`, Gmail `gmail:<thread id>`, DM `slack:dm:<channel>`,
///   Notion 메일 `notion-mail:<thread id>`
///
/// 일정 필드
/// - 종일: all_day=true, start_date와 end_date는 `LocalDate::from_all_day`로 얻은 YYYY-MM-DD.
///   시작은 포함, 끝은 제외다. start_at, end_at은 None이다. 순간 변환을 금지한다.
/// - 시간: all_day=false, start_at과 end_at은 dateTime 원문 그대로(오프셋 유지).
/// - series_id는 recurringEventId, busy는 transparent나 AVAILABILITY_FREE일 때 false.
/// - my_response는 accepted, declined, tentative, needsAction 중 하나. 참석자는 수만 남긴다.
///
/// GitHub 필드
/// - state는 open, closed, merged. 일정은 confirmed, cancelled.
/// - ts에 updated_at을 넣지 않는다. updated_at은 사람의 활동과 무관하게 바뀐다.
///
/// 필드 순서는 snapshot_hash의 입력 순서다. 바꾸면 안 된다.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedObject {
    pub object_key: String,
    pub source: SourceName,
    pub kind: ObjectKind,
    pub external_id: String,
    pub permalink: Option<String>,
    pub actor_id: Option<String>,
    /// Slack channel ID 또는 캘린더 ID.
    pub channel_id: Option<String>,
    /// 소스 자체 시각. 생성 시각이나 메시지 ts.
    pub ts: Option<String>,
    /// 제목이나 앞부분. Ledger가 정제하고 120자로 자른다. 본문 전체를 넣지 않는다.
    pub text_head: Option<String>,
    pub trust_tier: Option<TrustTier>,
    pub all_day: Option<bool>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub series_id: Option<String>,
    pub attendee_count: Option<i64>,
    pub my_response: Option<String>,
    pub busy: Option<bool>,
    pub state: Option<String>,
    pub milestone_title: Option<String>,
    pub assigned_to_me: Option<bool>,
    pub authored_by_me: Option<bool>,
    pub review_requested_me: Option<bool>,
    pub is_draft: Option<bool>,
    pub review_decision: Option<String>,
    pub checks_state: Option<String>,
    pub head_ref_oid: Option<String>,
}

impl NormalizedObject {
    /// 필수 값만 받고 나머지는 비운다. 다른 필드는 구조체 갱신 문법으로 채운다.
    pub fn new(
        object_key: impl Into<String>,
        source: SourceName,
        kind: ObjectKind,
        external_id: impl Into<String>,
    ) -> Self {
        Self {
            object_key: object_key.into(),
            source,
            kind,
            external_id: external_id.into(),
            permalink: None,
            actor_id: None,
            channel_id: None,
            ts: None,
            text_head: None,
            trust_tier: None,
            all_day: None,
            start_date: None,
            end_date: None,
            start_at: None,
            end_at: None,
            series_id: None,
            attendee_count: None,
            my_response: None,
            busy: None,
            state: None,
            milestone_title: None,
            assigned_to_me: None,
            authored_by_me: None,
            review_requested_me: None,
            is_draft: None,
            review_decision: None,
            checks_state: None,
            head_ref_oid: None,
        }
    }
}

/// 시간축의 사건 1건.
///
/// dedupe_key 규칙 (설계 3.1): `gh:comment:<id>`, `gh:review:<id>`, `gh:rr:<event id>`,
/// `slack:<channel>:<ts>`, `gmail:<thread id>`, `notion-mail:<thread id>:<블록 순번>`.
/// 같은 사건은 재수집해도 같은 키여야 한다.
///
/// thread_key: 종료 증거를 찾는 단위. Slack은 `slack:<channel>:<thread_ts>`, DM은 `slack:<channel>`.
/// 생략하면 object_key, 그것도 없으면 dedupe_key를 쓴다.
///
/// occurred_at: 사건 자체의 시각. updated_at이 아니다. Ledger가 UTC ISO로 바꿔 저장한다.
/// mention_count: 한 메시지에서 멘션된 사람 수. 3명 이상이면 P2로 내려간다.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedEvent {
    pub dedupe_key: String,
    pub source: SourceName,
    pub event_type: EventType,
    pub occurred_at: String,
    pub actor_kind: ActorKind,
    pub actor_id: Option<String>,
    pub object_key: Option<String>,
    pub thread_key: Option<String>,
    pub mentions_me: Option<bool>,
    pub mention_count: Option<i64>,
    pub channel_id: Option<String>,
    pub permalink: Option<String>,
    pub text_head: Option<String>,
    pub trust_tier: Option<TrustTier>,
}

impl NormalizedEvent {
    /// 필수 값만 받고 나머지는 비운다.
    pub fn new(
        dedupe_key: impl Into<String>,
        source: SourceName,
        event_type: EventType,
        occurred_at: impl Into<String>,
        actor_kind: ActorKind,
    ) -> Self {
        Self {
            dedupe_key: dedupe_key.into(),
            source,
            event_type,
            occurred_at: occurred_at.into(),
            actor_kind,
            actor_id: None,
            object_key: None,
            thread_key: None,
            mentions_me: None,
            mention_count: None,
            channel_id: None,
            permalink: None,
            text_head: None,
            trust_tier: None,
        }
    }
}
