use kairos_core::{Instant, OffDates};

use crate::budget::{
    bundle_slot_minutes, lunch_end_minutes, lunch_start_minutes, work_end_minutes, work_start_minutes,
};

/// 근무 시간과 묶음 슬롯의 시각 판단. 시스템 시간대와 무관하게 KST로 본다.
#[derive(Clone, Copy, Debug)]
pub struct WorkSchedule<'a> {
    off_dates: &'a OffDates,
}

impl<'a> WorkSchedule<'a> {
    pub const fn new(off_dates: &'a OffDates) -> Self {
        Self { off_dates }
    }

    /// now 이후의 첫 묶음 시각. 오늘이 쉬는 날이면 다음 근무일이다.
    pub fn next_bundle_at(&self, now: Instant) -> Instant {
        let today = now.local_date();
        let minutes = now.local_minutes();
        if self.off_dates.is_workday(today)
            && let Some(slot) = bundle_slot_minutes().iter().find(|slot| **slot > minutes)
        {
            return Instant::at_local(today, *slot);
        }
        Instant::at_local(self.off_dates.next_workday(today), bundle_slot_minutes()[0])
    }

    /// 지금 속한 묶음 슬롯의 시작. 첫 슬롯 전이면 None이다.
    pub fn bundle_slot(&self, now: Instant) -> Option<Instant> {
        let minutes = now.local_minutes();
        let slot = bundle_slot_minutes().iter().rev().find(|slot| **slot <= minutes)?;
        Some(Instant::at_local(now.local_date(), *slot))
    }

    pub fn is_lunch(&self, now: Instant) -> bool {
        (lunch_start_minutes()..lunch_end_minutes()).contains(&now.local_minutes())
    }

    pub fn is_work_hours(&self, now: Instant) -> bool {
        (work_start_minutes()..work_end_minutes()).contains(&now.local_minutes())
    }

    /// 다음 근무 시작. 오늘 시작 전이면 오늘이다.
    pub fn next_work_start(&self, now: Instant) -> Instant {
        let today = now.local_date();
        if self.off_dates.is_workday(today) && now.local_minutes() < work_start_minutes() {
            return Instant::at_local(today, work_start_minutes());
        }
        Instant::at_local(self.off_dates.next_workday(today), work_start_minutes())
    }
}
