use kairos_core::{Instant, KairosResult};
use kairos_policy::{P1_NOTIFY_TRIES, PUSH_LIMIT};
use kairos_store::params;
use kairos_store::row::scalar;
use kairos_store::rusqlite::Params;
use serde::Serialize;

use super::ItemRepo;
use crate::item::{ItemTier, P1Notice};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct TierCounts {
    #[serde(rename = "P0")]
    pub p0: i64,
    #[serde(rename = "P1")]
    pub p1: i64,
    #[serde(rename = "P2")]
    pub p2: i64,
}

/// 건수만 있고 제목은 없다. 필드 순서가 JSON 키 순서다.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemCounts {
    /// 기준선과 직접 추가를 뺀 열린 항목. 상태 표시줄의 제안 건수는 이 합이다.
    pub open: TierCounts,
    /// kairos add로 만든 열린 할 일. 제안이 아니므로 open에 넣지 않는다.
    pub manual: i64,
    pub parked: i64,
    pub accepted: i64,
    /// 유지로 고른 기준선 항목.
    pub baseline: i64,
    /// 어제 이전에 열렸고 아직 응답이 없는 P1.
    pub carried_p1: i64,
    /// due_at이 지났는데 한 번도 표면에 오르지 않은 P1. 측정 지표다.
    pub overdue_p1: i64,
    /// 밀어내기 한도를 다 쓴 P1.
    pub pushed_out_p1: i64,
    /// 닫히지 않은 P0 중 알림이 나가지 않은 건수.
    pub p0_unnotified: i64,
    /// 알릴 차례를 기다리는 P1. 포커스, 점심, 근무 시간 밖의 보류다. 고장이 아니다.
    pub p1_held: i64,
    /// 전달을 시도했지만 실패한 P1.
    pub p1_undelivered: i64,
    /// 미전달 중 시도 상한을 다 써서 더 시도하지 않는 건수.
    pub p1_gave_up: i64,
    /// 오늘 전달한 P1.
    pub p1_sent_today: i64,
    /// 오늘 앞선 알림에 묶여 따로 울리지 않은 P1.
    pub p1_merged_today: i64,
    /// 오늘 알리지 않기로 한 P1. 게이트, 일시정지, 쉬는 날, 알림 끔이다.
    pub p1_dropped_today: i64,
}

impl ItemCounts {
    /// 표시 게이트가 열렸을 때의 제안 건수.
    pub const fn open_total(&self) -> i64 {
        self.open.p0 + self.open.p1 + self.open.p2
    }
}

const P1: &str = "state = 'open' AND baseline = 0 AND tier = 'P1'";

impl ItemRepo<'_> {
    fn one(&self, condition: &str, values: impl Params) -> KairosResult<i64> {
        scalar(self.connection(), &format!("SELECT count(*) FROM items WHERE {condition}"), values)
    }

    fn open_tier(&self, tier: ItemTier) -> KairosResult<i64> {
        self.one("state = 'open' AND baseline = 0 AND reason != 'manual' AND tier = ?", params![tier.as_str()])
    }

    fn noticed(&self, notice: P1Notice, today: &str) -> KairosResult<i64> {
        self.one("p1_notice = ? AND p1_notified_at >= ?", params![notice.as_str(), today])
    }

    pub fn counts(&self, now: Instant) -> KairosResult<ItemCounts> {
        let today = now.start_of_local_day().to_iso();
        let at = now.to_iso();
        let waiting = format!("{P1} AND p1_notified_at IS NULL AND p1_notify_failed_at IS");
        Ok(ItemCounts {
            open: TierCounts {
                p0: self.open_tier(ItemTier::P0)?,
                p1: self.open_tier(ItemTier::P1)?,
                p2: self.open_tier(ItemTier::P2)?,
            },
            manual: self.one("state = 'open' AND reason = 'manual'", [])?,
            parked: self.one("state = 'parked' AND park_trigger != 'triage'", [])?,
            accepted: self.one("state = 'accepted'", [])?,
            baseline: self.one("state = 'open' AND baseline = 1", [])?,
            carried_p1: self.one(&format!("{P1} AND opened_at < ?"), params![today])?,
            overdue_p1: self.one(&format!("{P1} AND first_surfaced_at IS NULL AND due_at < ?"), params![at])?,
            pushed_out_p1: self.one(&format!("{P1} AND push_count >= ?"), params![PUSH_LIMIT])?,
            p0_unnotified: self.one("state IN ('open', 'accepted') AND tier = 'P0' AND p0_notified_at IS NULL", [])?,
            p1_held: self.one(&format!("{waiting} NULL"), [])?,
            p1_undelivered: self.one(&format!("{waiting} NOT NULL"), [])?,
            p1_gave_up: self
                .one(&format!("{waiting} NOT NULL AND p1_notify_attempts >= ?"), params![P1_NOTIFY_TRIES])?,
            p1_sent_today: self.noticed(P1Notice::Sent, &today)?,
            p1_merged_today: self.noticed(P1Notice::Merged, &today)?,
            p1_dropped_today: self.noticed(P1Notice::Dropped, &today)?,
        })
    }
}
