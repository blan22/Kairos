use kairos_core::local_date::MAX_OFF_SPAN_DAYS;
use kairos_core::{Instant, KairosResult, LocalDate, OffDates};
use kairos_store::Connection;

use crate::repo::{DayOffScope, ObjectRepo};

/// day_off 일정에서 읽는 쉬는 날.
#[derive(Clone, Copy, Debug)]
pub struct OffDateBook<'a> {
    objects: ObjectRepo<'a>,
}

impl<'a> OffDateBook<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { objects: ObjectRepo::new(connection) }
    }

    /// day_off 일정(시작 포함, 끝 제외)을 날짜로 펼친다.
    fn dates(&self, scope: DayOffScope) -> KairosResult<Vec<LocalDate>> {
        let mut dates = Vec::new();
        for (start, end) in self.objects.day_off_spans(scope)? {
            let (Some(start), Some(end)) = (start, end) else {
                continue;
            };
            let Some(mut date) = LocalDate::parse(&start) else {
                continue;
            };
            // 둘 다 YYYY-MM-DD다. 끝은 달력에 없는 날이어도 TS처럼 문자열로 비교한다.
            let mut span = 0;
            while date.to_string() < end && span < MAX_OFF_SPAN_DAYS {
                dates.push(date);
                date = date.add_days(1);
                span += 1;
            }
        }
        Ok(dates)
    }

    /// 근무일 판정에 쓰는 쉬는 날 전체. 지정된 쉬는 날도 합친다.
    pub fn load_off_dates(&self, extra: &OffDates) -> KairosResult<OffDates> {
        Ok(extra.iter().chain(self.dates(DayOffScope::All)?).collect())
    }

    /// 수집 주기와 건강 판정에 쓰는 쉬는 날. 공유 캘린더의 일정은 뺀다.
    /// 동료가 그 캘린더에 '<내 이름> 연차'를 넣는 것만으로 3분 수집과 감시자가 꺼지면 안 된다.
    pub fn load_watch_off_dates(&self, extra: &OffDates) -> KairosResult<OffDates> {
        Ok(extra.iter().chain(self.dates(DayOffScope::ExceptShared)?).collect())
    }

    /// 공유 캘린더에서 온 쉬는 날 중 오늘 이후. 수집을 바꾸지 않으므로 사용자에게 보여 주기만 한다.
    pub fn shared_off_dates(&self, now: Instant) -> KairosResult<Vec<LocalDate>> {
        let today = now.local_date();
        let dates: OffDates = self.dates(DayOffScope::OnlyShared)?.into_iter().collect();
        Ok(dates.iter().filter(|date| *date >= today).collect())
    }
}
