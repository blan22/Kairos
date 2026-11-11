use kairos_core::model::{ActorKind, EventType, ObjectKind};
use kairos_core::string_enum;

string_enum! {
    /// 등급. 순서가 표시 순서다.
    pub enum Tier {
        P0 => "P0",
        P1 => "P1",
        P2 => "P2",
        Record => "record",
    }
}

string_enum! {
    pub enum Disposition {
        Opened => "opened",
        Folded => "folded",
        Noise => "noise",
        Unclassified => "unclassified",
    }
}

string_enum! {
    pub enum ItemReason {
        ReplyOwed => "reply_owed",
        ReviewOwed => "review_owed",
        Assigned => "assigned",
        MyPrAttention => "my_pr_attention",
        MeetingNew => "meeting_new",
        DeployRisk => "deploy_risk",
        WorkOwed => "work_owed",
        Manual => "manual",
    }
}

string_enum! {
    /// 규칙 표. 등급과 항목 사유는 `tier`, `reason`이 정한다.
    pub enum RuleId {
        P0MeetingImminent => "p0.meeting_imminent",
        P0DeployDay => "p0.deploy_day",
        P1DirectMention => "p1.direct_mention",
        P1ChangesRequested => "p1.changes_requested",
        P1CiFailed => "p1.ci_failed",
        P1ReviewRerequested => "p1.review_rerequested",
        P1DeployD1 => "p1.deploy_d1",
        P1DeployOwnedComment => "p1.deploy_owned_comment",
        P1AppReviewMail => "p1.app_review_mail",
        P1SentryMail => "p1.sentry_mail",
        P1NotionMention => "p1.notion_mention",
        P2Assigned => "p2.assigned",
        P2ReviewRequested => "p2.review_requested",
        P2OwnedComment => "p2.owned_comment",
        P2MultiMention => "p2.multi_mention",
        P2FigmaReady => "p2.figma_ready",
        P2NotionComment => "p2.notion_comment",
        /// 설계 5절 표에 없다. 형식을 몰라도 사람이 봐야 하므로 숨기지 않는다. 제목 없이 메일 링크만 있다.
        P2NotionUnknown => "p2.notion_unknown",
        /// 설계 5절 표에 없다. 사용자 결정(회의와 일정 변경 1순위)을 따라 기록에서 빠지지 않게 P2로 둔다.
        P2MeetingChange => "p2.meeting_change",
        NoiseBot => "noise.bot",
        NoiseSelf => "noise.self",
        NoiseInvolves => "noise.involves",
        NoiseNoReply => "noise.no_reply",
        NoiseBuildMail => "noise.build_mail",
        NoiseCalendarInvite => "noise.calendar_invite",
        NoiseTeamCalendar => "noise.team_calendar",
        NoiseNeedsAction => "noise.needs_action",
        NoiseMeetingDeclined => "noise.meeting_declined",
        NoiseMeetingFree => "noise.meeting_free",
        NoiseMeetingUpdate => "noise.meeting_update",
        NoiseDmLog => "noise.dm_log",
        NoiseLifecycle => "noise.lifecycle",
        NoiseNotionSource => "noise.notion_source",
        NoiseNotionEdit => "noise.notion_edit",
        NoiseNotionPromo => "noise.notion_promo",
        Unclassified => "unclassified",
    }
}

impl RuleId {
    pub const fn tier(self) -> Tier {
        match self {
            Self::P0MeetingImminent | Self::P0DeployDay => Tier::P0,
            Self::P1DirectMention
            | Self::P1ChangesRequested
            | Self::P1CiFailed
            | Self::P1ReviewRerequested
            | Self::P1DeployD1
            | Self::P1DeployOwnedComment
            | Self::P1AppReviewMail
            | Self::P1SentryMail
            | Self::P1NotionMention => Tier::P1,
            Self::P2Assigned
            | Self::P2ReviewRequested
            | Self::P2OwnedComment
            | Self::P2MultiMention
            | Self::P2FigmaReady
            | Self::P2NotionComment
            | Self::P2NotionUnknown
            | Self::P2MeetingChange => Tier::P2,
            Self::NoiseBot
            | Self::NoiseSelf
            | Self::NoiseInvolves
            | Self::NoiseNoReply
            | Self::NoiseBuildMail
            | Self::NoiseCalendarInvite
            | Self::NoiseTeamCalendar
            | Self::NoiseNeedsAction
            | Self::NoiseMeetingDeclined
            | Self::NoiseMeetingFree
            | Self::NoiseMeetingUpdate
            | Self::NoiseDmLog
            | Self::NoiseLifecycle
            | Self::NoiseNotionSource
            | Self::NoiseNotionEdit
            | Self::NoiseNotionPromo
            | Self::Unclassified => Tier::Record,
        }
    }

    /// 항목이 되는 규칙의 사유. 기록만 하는 규칙은 None이다.
    pub const fn reason(self) -> Option<ItemReason> {
        match self {
            Self::P0MeetingImminent | Self::P2MeetingChange => Some(ItemReason::MeetingNew),
            Self::P0DeployDay | Self::P1DeployD1 => Some(ItemReason::DeployRisk),
            Self::P1DirectMention | Self::P1DeployOwnedComment | Self::P2OwnedComment | Self::P2MultiMention => {
                Some(ItemReason::ReplyOwed)
            }
            Self::P1ChangesRequested | Self::P1CiFailed => Some(ItemReason::MyPrAttention),
            Self::P1ReviewRerequested | Self::P2ReviewRequested => Some(ItemReason::ReviewOwed),
            Self::P2Assigned => Some(ItemReason::Assigned),
            Self::P1AppReviewMail
            | Self::P1SentryMail
            | Self::P1NotionMention
            | Self::P2FigmaReady
            | Self::P2NotionComment
            | Self::P2NotionUnknown => Some(ItemReason::WorkOwed),
            Self::NoiseBot
            | Self::NoiseSelf
            | Self::NoiseInvolves
            | Self::NoiseNoReply
            | Self::NoiseBuildMail
            | Self::NoiseCalendarInvite
            | Self::NoiseTeamCalendar
            | Self::NoiseNeedsAction
            | Self::NoiseMeetingDeclined
            | Self::NoiseMeetingFree
            | Self::NoiseMeetingUpdate
            | Self::NoiseDmLog
            | Self::NoiseLifecycle
            | Self::NoiseNotionSource
            | Self::NoiseNotionEdit
            | Self::NoiseNotionPromo
            | Self::Unclassified => None,
        }
    }
}

/// `rule_id` 문자열의 등급. 모르는 규칙은 기록만이다.
pub fn tier_of(rule_id: &str) -> Tier {
    rule_id.parse().map_or(Tier::Record, RuleId::tier)
}

/// 판정에 쓰는 이벤트의 값.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleEvent {
    pub event_type: EventType,
    pub actor_kind: ActorKind,
    pub actor_id: Option<String>,
    pub mentions_me: bool,
    pub mention_count: i64,
    pub text_head: Option<String>,
}

/// 판정에 쓰는 객체의 값. 시각은 저장된 원문이다.
#[derive(Clone, Debug, PartialEq)]
pub struct RuleObject {
    pub kind: ObjectKind,
    pub state: Option<String>,
    pub all_day: bool,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub my_response: Option<String>,
    pub busy: Option<bool>,
    pub assigned_to_me: bool,
    pub authored_by_me: bool,
}
