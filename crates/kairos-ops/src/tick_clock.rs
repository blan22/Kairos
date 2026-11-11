use kairos_core::{Instant, KairosResult};
use kairos_policy::tick_start_minutes;
use kairos_store::row::sql;
use kairos_store::{Connection, OptionalExtension, params};

use crate::sleep::TICK_GAP_MS;

#[derive(Clone, Copy, Debug)]
pub struct TickClockRepo<'a> {
    connection: &'a Connection,
}

impl<'a> TickClockRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    fn get(&self) -> KairosResult<Option<(String, Option<String>)>> {
        self.connection
            .query_row("SELECT last_tick_at, resumed_at FROM tick_clock WHERE id = 1", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(sql)
    }

    fn save(&self, last_tick_at: &str, resumed_at: Option<&str>) -> KairosResult<()> {
        self.connection
            .execute(
                "INSERT INTO tick_clock (id, last_tick_at, resumed_at) VALUES (1, ?, ?)
                 ON CONFLICT (id) DO UPDATE SET last_tick_at = excluded.last_tick_at, resumed_at = excluded.resumed_at",
                params![last_tick_at, resumed_at],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// 이번 틱을 적고 복귀 시각을 돌려준다. P1 알림 합치기의 기준이다(설계 16절).
    /// 공백은 그날 수집이 시작된 뒤부터 잰다. 밤과 주말은 수집이 쉬는 시간이라 근무일 아침의 첫 틱은 복귀가 아니다.
    /// 얼어붙은 틱은 공백과 무관하게 복귀다. `at`에 깨어난 시각을 넘긴다.
    pub fn note_tick(&self, at: Instant, frozen: bool) -> KairosResult<Option<Instant>> {
        let row = self.get()?;
        let day_start = Instant::at_local(at.local_date(), tick_start_minutes());
        // 저장된 시각을 읽지 못하면 원본의 NaN처럼 공백 비교가 거짓이 된다.
        let floor = match &row {
            Some((last_tick_at, _)) => Instant::parse(last_tick_at).map(|last| last.max(day_start)),
            None => Some(at),
        };
        let gapped = floor.is_some_and(|floor| at.ms_since(floor) > TICK_GAP_MS);
        let resumed_at = if frozen || gapped { Some(at.to_iso()) } else { row.and_then(|(_, resumed)| resumed) };
        self.save(&at.to_iso(), resumed_at.as_deref())?;
        Ok(resumed_at.as_deref().and_then(Instant::parse))
    }
}
