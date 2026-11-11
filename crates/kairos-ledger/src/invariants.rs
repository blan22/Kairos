use kairos_core::KairosResult;
use kairos_store::Connection;
use serde::{Deserialize, Serialize};

use crate::repo::EventRepo;

/// maintenance.json에 이 키로 저장된다.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invariants {
    /// 처분 없는 이벤트. 0이어야 한다.
    pub events_without_disposition: i64,
    /// 근거 없는 종료. 0이어야 한다.
    pub closures_without_evidence: i64,
    /// 규칙이 판정하지 못한 이벤트. 위반은 아니지만 규칙 공백이므로 드러낸다.
    pub unclassified: i64,
    /// 있지도 않은 이벤트를 근거로 닫힌 항목. 0이어야 한다.
    pub item_closures_without_evidence: i64,
}

pub fn check_invariants(connection: &Connection) -> KairosResult<Invariants> {
    let events = EventRepo::new(connection);
    Ok(Invariants {
        events_without_disposition: events.count_without_disposition()?,
        closures_without_evidence: events.count_closures_without_evidence()?,
        unclassified: events.count_unclassified()?,
        item_closures_without_evidence: events.count_item_closures_without_evidence()?,
    })
}
