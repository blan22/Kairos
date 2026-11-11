//! 항목의 타입. 등급은 규칙에서만 오고 사용자 입력으로 P0를 만들 수 없다.

use kairos_core::{Instant, string_enum};
use kairos_policy::{CloseReason, ItemReason, Mergeable, RuleId, Tier};

string_enum! {
    /// 항목의 등급. 기록만인 `Tier::Record`는 항목이 되지 않는다.
    pub enum ItemTier {
        P0 => "P0",
        P1 => "P1",
        P2 => "P2",
    }
}

impl ItemTier {
    pub const fn from_tier(tier: Tier) -> Option<Self> {
        match tier {
            Tier::P0 => Some(Self::P0),
            Tier::P1 => Some(Self::P1),
            Tier::P2 => Some(Self::P2),
            Tier::Record => None,
        }
    }

    /// 둘 중 높은 등급. 변형 순서가 곧 높은 순서다.
    #[must_use]
    pub fn higher(self, other: Self) -> Self {
        self.min(other)
    }
}

string_enum! {
    pub enum ItemState {
        Open => "open",
        Parked => "parked",
        Accepted => "accepted",
        Closed => "closed",
    }
}

string_enum! {
    pub enum ParkTrigger {
        NextBundle => "next_bundle",
        Deploy => "deploy",
        Kept => "kept",
        Triage => "triage",
    }
}

string_enum! {
    pub enum ItemCloseReason {
        DoneEvidence => "done_evidence",
        Withdrawn => "withdrawn",
        Expired => "expired",
        UserDone => "user_done",
        UserDropped => "user_dropped",
    }
}

impl From<CloseReason> for ItemCloseReason {
    fn from(reason: CloseReason) -> Self {
        match reason {
            CloseReason::DoneEvidence => Self::DoneEvidence,
            CloseReason::Withdrawn => Self::Withdrawn,
            CloseReason::Expired => Self::Expired,
        }
    }
}

string_enum! {
    /// p1_notified_at이 찍힌 이유. 전달, 앞선 알림에 묶임, 알리지 않기로 함, 사용자가 이미 다룸이다.
    pub enum P1Notice {
        Sent => "sent",
        Merged => "merged",
        Dropped => "dropped",
        Handled => "handled",
    }
}

/// items 표의 한 행. 시각 열은 SQL이 문자열로 비교하는 저장 원문이다.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Item {
    pub id: i64,
    pub object_key: String,
    pub reason: ItemReason,
    pub tier: ItemTier,
    pub state: ItemState,
    pub baseline: bool,
    pub source: Option<String>,
    pub channel_id: Option<String>,
    /// manual 항목의 사용자 입력만 들어간다.
    pub title: Option<String>,
    pub opened_by_event: Option<String>,
    pub opened_at: String,
    pub due_at: Option<String>,
    pub first_surfaced_at: Option<String>,
    pub push_count: i64,
    pub bundle_queued: bool,
    pub parked_until: Option<String>,
    pub park_trigger: Option<ParkTrigger>,
    pub p0_notified_at: Option<String>,
    /// 알렸거나, 합쳤거나, 알리지 않기로 했거나, 사용자가 이미 다뤘다. 더 알릴 일이 없다. 이유는 p1_notice다.
    pub p1_notified_at: Option<String>,
    pub p1_notify_failed_at: Option<String>,
    pub p1_notify_attempts: i64,
    /// 보류로 처음 본 시각. 닫힌 틱에 버리지 않고 30분 구간에 묶지 않는다.
    pub p1_held_at: Option<String>,
    pub p1_notice: Option<P1Notice>,
    pub closed_at: Option<String>,
    pub close_reason: Option<ItemCloseReason>,
    pub close_evidence_event: Option<String>,
}

impl Item {
    /// 어제 이전에 열린 P1이다. 저장 형식이 고정이라 문자열로 비교한다.
    pub fn is_carried(&self, now: Instant) -> bool {
        self.tier == ItemTier::P1 && self.opened_at < now.start_of_local_day().to_iso()
    }

    /// 조용히 대기 중이다. 새 사건이 오면 깨어난다.
    pub fn is_parked_quietly(&self) -> bool {
        matches!(self.park_trigger, Some(ParkTrigger::Kept | ParkTrigger::Triage))
    }

    /// 항목을 만드는 키. 객체가 없으면 스레드가 키다.
    pub fn key_of<'a>(object_key: Option<&'a str>, thread_key: &'a str) -> &'a str {
        object_key.unwrap_or(thread_key)
    }
}

impl Mergeable for Item {
    fn channel_id(&self) -> Option<&str> {
        self.channel_id.as_deref()
    }

    fn opened_at(&self) -> Option<Instant> {
        Instant::parse(&self.opened_at)
    }
}

/// 규칙 ID의 사유와 등급. 항목이 되지 않는 규칙이나 모르는 규칙은 None이다.
pub(crate) fn rule_target(rule_id: &str) -> Option<(ItemReason, ItemTier)> {
    let rule: RuleId = rule_id.parse().ok()?;
    Some((rule.reason()?, ItemTier::from_tier(rule.tier())?))
}
