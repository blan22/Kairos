//! 표시 게이트. 항목 엔진과 기록은 이 값과 무관하게 돈다. 표시만 막는다.

use kairos_core::model::{Source, SourceMode};
use kairos_core::{Instant, KairosError, KairosResult, OffDates, string_enum};
use kairos_ledger::{CursorRepo, OffDateBook};
use kairos_policy::tick_end_minutes;
use kairos_store::Connection;
use serde::Serialize;

use super::{ControlFile, DisplayMode, KairosControl};

/// 기록 첫날로 치는 마감. 이 시각 이전에 T0가 서야 그날을 한 근무일로 센다.
const FIRST_DAY_GRACE_MINUTES: u32 = 10 * 60;

string_enum! {
    /// paused kairos pause 중이다. display_off kairos display off다.
    /// no_collection auto인데 정상 수집이 아직 없다. recording auto인데 기록 1근무일이 지나지 않았다.
    pub enum GateReason {
        Paused => "paused",
        DisplayOff => "display_off",
        NoCollection => "no_collection",
        Recording => "recording",
    }
}

impl GateReason {
    const fn message(self) -> &'static str {
        match self {
            Self::Paused => "일시정지 중입니다. 해제: kairos pause --resume",
            Self::DisplayOff => "표시를 껐습니다. 켜기: kairos display auto",
            Self::NoCollection => "아직 정상 수집이 없습니다",
            Self::Recording => "기록 1근무일이 끝나면 표시를 시작합니다",
        }
    }
}

/// 표시 시작 시점의 재료. 표면은 gate를 읽는다.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayStart {
    /// 연결된 소스 중 가장 이른 T0. 정상 수집이 없으면 None.
    pub t0: Option<String>,
    /// T0 이후 1근무일이 끝나는 시각(그날 20:00 KST). T0가 없으면 None.
    pub starts_at: Option<Instant>,
    /// now가 starts_at 이후인가.
    pub ready: bool,
}

impl DisplayStart {
    /// T0가 근무일 10:00 이전에 섰으면 그날, 아니면 다음 근무일을 기록 1근무일로 센다.
    /// 그 근무일의 수집이 끝나는 20:00 KST가 표시 시작 시점이다.
    pub fn compute(t0: Option<&str>, now: Instant, off_dates: &OffDates) -> KairosResult<Self> {
        let Some(t0) = t0 else {
            return Ok(Self { t0: None, starts_at: None, ready: false });
        };
        let at = Instant::parse(t0).ok_or_else(|| KairosError::new("invalid_time"))?;
        let mut day = at.local_date();
        if !off_dates.is_workday(day) || at.local_minutes() > FIRST_DAY_GRACE_MINUTES {
            day = off_dates.next_workday(day);
        }
        let starts_at = Instant::at_local(day, tick_end_minutes());
        Ok(Self { t0: Some(t0.to_owned()), starts_at: Some(starts_at), ready: now >= starts_at })
    }

    /// 지금의 재료. 쉬는 날은 DB의 일정과 사용자가 지정한 날을 합친다.
    pub fn load(
        connection: &Connection,
        sources: &[&dyn Source],
        now: Instant,
        control: &KairosControl,
    ) -> KairosResult<Self> {
        let off_dates = OffDateBook::new(connection).load_off_dates(&control.off_date_set())?;
        Self::compute(earliest_t0(connection, sources)?.as_deref(), now, &off_dates)
    }
}

/// 연결된 소스 중 가장 이른 T0.
pub fn earliest_t0(connection: &Connection, sources: &[&dyn Source]) -> KairosResult<Option<String>> {
    let connected: Vec<_> = sources
        .iter()
        .filter(|source| !matches!(source.mode(), SourceMode::NotConnected { .. }))
        .map(|source| source.name().as_str())
        .collect();
    let rows = CursorRepo::new(connection).established_t0s()?;
    let times = rows.into_iter().filter(|(source, _)| connected.contains(&source.as_str())).map(|(_, t0)| t0);
    // 해석할 수 없는 값은 NaN이라 `<=`가 거짓이고 뒤의 값이 남는다.
    Ok(times.reduce(|a, b| match (Instant::parse(&a), Instant::parse(&b)) {
        (Some(left), Some(right)) if left <= right => a,
        _ => b,
    }))
}

/// 지금 표시해도 되는가. 패널의 값은 next가 주고 state.json의 값은 감시와 조회용이다.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayGate {
    pub open: bool,
    pub mode: DisplayMode,
    pub reason: Option<GateReason>,
    pub message: Option<String>,
    /// auto에서 표시가 시작되는 시각. 모르면 None이다.
    pub starts_at: Option<Instant>,
}

impl DisplayGate {
    pub fn evaluate(start: &DisplayStart, control: &KairosControl, now: Instant) -> Self {
        let mode = control.display_mode();
        let opened = Self { open: true, mode, reason: None, message: None, starts_at: start.starts_at };
        let closed = |reason: GateReason| Self {
            open: false,
            mode,
            reason: Some(reason),
            message: Some(reason.message().to_owned()),
            starts_at: start.starts_at,
        };
        if control.is_paused(now) {
            return closed(GateReason::Paused);
        }
        match mode {
            DisplayMode::Off => closed(GateReason::DisplayOff),
            DisplayMode::On => opened,
            DisplayMode::Auto if control.display_opened() => opened,
            DisplayMode::Auto if start.t0.is_none() => closed(GateReason::NoCollection),
            DisplayMode::Auto if start.ready => opened,
            DisplayMode::Auto => closed(GateReason::Recording),
        }
    }

    /// 지금의 표시 게이트.
    pub fn current(
        connection: &Connection,
        sources: &[&dyn Source],
        now: Instant,
        control: &KairosControl,
    ) -> KairosResult<Self> {
        Ok(Self::evaluate(&DisplayStart::load(connection, sources, now, control)?, control, now))
    }
}

impl ControlFile<'_> {
    /// auto의 표시가 처음 열리는 순간을 남긴다. 저장은 tick과 next가 부를 때 한 번뿐이고, 켠 뒤로는 값을 다시 계산하지 않는다.
    /// 틱 동안 사용자가 바꾼 값을 덮지 않게 저장 직전에 다시 읽는다.
    pub fn latch_display(
        &self,
        connection: &Connection,
        sources: &[&dyn Source],
        now: Instant,
        control: KairosControl,
    ) -> KairosResult<KairosControl> {
        if control.display_opened() || control.display_mode() != DisplayMode::Auto {
            return Ok(control);
        }
        if !DisplayStart::load(connection, sources, now, &control)?.ready {
            return Ok(control);
        }
        let opened = self.read().with_display_opened_at(&now.to_iso());
        self.write(&opened)?;
        Ok(opened)
    }
}
