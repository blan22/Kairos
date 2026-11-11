use kairos_core::{Instant, KairosResult, OffDates};
use kairos_policy::{DAY_TARGET, P1_PER_DAY, P1_PER_HOUR, WorkSchedule};
use kairos_store::Connection;
use serde::Serialize;

use crate::repo::SuggestionRepo;

const HOUR_MS: i64 = 3_600_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetUsage {
    pub p1_hour_used: i64,
    pub p1_hour_limit: i64,
    pub p1_day_used: i64,
    pub p1_day_limit: i64,
    pub bundles_today: i64,
    pub shown_today: i64,
    pub day_target: i64,
    pub next_bundle_at: Instant,
}

/// 표시 예산의 사용량. 카드 수로 센다.
#[derive(Clone, Copy, Debug)]
pub struct BudgetMeter<'a> {
    suggestions: SuggestionRepo<'a>,
}

impl<'a> BudgetMeter<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { suggestions: SuggestionRepo::new(connection) }
    }

    pub fn usage(&self, now: Instant, off_dates: &OffDates) -> KairosResult<BudgetUsage> {
        let day = now.start_of_local_day().to_iso();
        Ok(BudgetUsage {
            p1_hour_used: self.suggestions.p1_cards_after(&now.minus_ms(HOUR_MS).to_iso())?,
            p1_hour_limit: P1_PER_HOUR,
            p1_day_used: self.suggestions.p1_cards_since(&day)?,
            p1_day_limit: P1_PER_DAY,
            bundles_today: self.suggestions.bundles_since(&day)?,
            shown_today: self.suggestions.shown_since(&day)?,
            day_target: DAY_TARGET,
            next_bundle_at: WorkSchedule::new(off_dates).next_bundle_at(now),
        })
    }
}
