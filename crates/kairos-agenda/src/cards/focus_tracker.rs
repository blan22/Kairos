use kairos_core::{Instant, KairosError, KairosResult};
use kairos_ledger::ObjectRepo;
use kairos_policy::{FOCUS_MAX_MINUTES, FOCUS_MEETING_MARGIN_MS, FOCUS_NOTICE_MS};
use kairos_store::Connection;
use serde::Serialize;

use crate::repo::ItemRepo;
use crate::state::KairosControl;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FocusView {
    pub active: bool,
    pub started_at: Option<String>,
    /// 회의를 반영한 실제 끝.
    pub until: Option<String>,
    pub held_p1: i64,
    /// 끝난 뒤 한 시간 동안만 있다.
    pub held_line: Option<String>,
}

/// 포커스의 실제 끝과 그동안 쌓인 건수를 계산한다.
#[derive(Clone, Copy, Debug)]
pub struct FocusTracker<'a> {
    connection: &'a Connection,
}

impl<'a> FocusTracker<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 다음 회의 때문에 포커스가 끝나는 시각. max(발견 시각, 회의 시작 - 5분)이다. 일정 빈 구간으로 포커스를 만들지는 않는다.
    fn meeting_stop(&self, started_at: &str) -> KairosResult<Option<String>> {
        let started = Instant::parse(started_at);
        let mut earliest = None;
        for (start_at, first_seen_at) in ObjectRepo::new(self.connection).focus_stopping_meetings()? {
            // 해석할 수 없는 시작 시각은 NaN이라 비교가 거짓이고 후보가 아니다.
            let start = match (Instant::parse(&start_at), started) {
                (Some(start), Some(started)) if start > started => start,
                _ => continue,
            };
            // 발견 시각이 NaN이면 Math.max가 NaN이고 toISOString이 던진다.
            let seen = Instant::parse(&first_seen_at).ok_or_else(|| KairosError::new("invalid_time"))?;
            let stop = seen.max(start.minus_ms(FOCUS_MEETING_MARGIN_MS));
            earliest = Some(earliest.map_or(stop, |current: Instant| current.min(stop)));
        }
        Ok(earliest.map(Instant::to_iso))
    }

    pub fn view(&self, control: &KairosControl, now: Instant) -> KairosResult<FocusView> {
        let Some(focus) = control.focus()? else {
            return Ok(FocusView { active: false, started_at: None, until: None, held_p1: 0, held_line: None });
        };
        let stop = self.meeting_stop(&focus.started_at)?;
        let planned = [Some(focus.until), stop, focus.ended_at].into_iter().flatten();
        // 해석할 수 없는 값은 NaN이라 `<=`가 거짓이고 뒤의 값이 남는다.
        let until = planned
            .reduce(|a, b| match (Instant::parse(&a), Instant::parse(&b)) {
                (Some(left), Some(right)) if left <= right => a,
                _ => b,
            })
            .unwrap_or_default();
        let end = Instant::parse(&until);
        let active = end.is_some_and(|end| now < end);
        let held_to = if active { now.to_iso() } else { until.clone() };
        let held = ItemRepo::new(self.connection).count_focus_held(&focus.started_at, &held_to)?;
        let noticed = !active && held > 0 && end.is_some_and(|end| now.ms_since(end) < FOCUS_NOTICE_MS);
        Ok(FocusView {
            active,
            started_at: Some(focus.started_at),
            until: Some(until),
            held_p1: held,
            held_line: noticed.then(|| format!("포커스 중 쌓인 P1 {held}건")),
        })
    }

    /// 사용자가 시작한 포커스만 있다. 끝은 90분을 넘지 않는다.
    #[must_use]
    pub fn start(control: KairosControl, minutes: i64, now: Instant) -> KairosControl {
        let span = minutes.min(FOCUS_MAX_MINUTES);
        control.with_focus(&now.to_iso(), &now.plus_ms(span * 60_000).to_iso())
    }

    #[must_use]
    pub fn end(control: KairosControl, now: Instant) -> KairosControl {
        control.with_focus_ended(&now.to_iso())
    }
}
