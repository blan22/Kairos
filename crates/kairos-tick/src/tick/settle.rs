//! 기록 뒤의 항목 갱신과 알림. 어떤 실패도 수집 기록과 state.json을 막지 않는다.

use std::collections::BTreeSet;

use kairos_agenda::{
    CardChooser, ControlFile, DisplayGate, EngineReport, EngineStatus, KairosControl, NextContext, P0Alerts, P1Alerts,
    P1NotifyInput, P1NotifyReport, RefreshInput, StateBuilder,
};
use kairos_core::model::Source;
use kairos_core::ports::Notifier;
use kairos_core::{Instant, KairosError, KairosHome, KairosResult};
use kairos_ledger::OffDateBook;
use kairos_ops::TickClockRepo;
use kairos_store::Database;

use super::ItemRefresher;
use super::guard::caught;
use crate::repo::TickRunRepo;

pub(super) struct Settler<'a> {
    pub database: &'a Database,
    pub home: &'a KairosHome,
    pub sources: &'a [&'a dyn Source],
    pub now: Instant,
}

impl Settler<'_> {
    /// 패닉도 던진 것과 같게 본다. 빠져나온 트랜잭션을 닫아야 뒤의 기록이 산다.
    fn guarded<T>(&self, work: impl FnOnce() -> KairosResult<T>) -> KairosResult<T> {
        let outcome = caught(work);
        if outcome.is_none() {
            TickRunRepo::new(self.database.connection()).rollback_abandoned();
        }
        outcome.unwrap_or_else(|| Err(KairosError::new("engine_failed")))
    }

    fn write_status(&self, ok: bool, error_code: Option<String>) -> KairosResult<()> {
        StateBuilder::write_engine_status(&EngineStatus { at: self.now.to_iso(), ok, error_code }, self.home)
    }

    fn refresh(
        &self,
        control: &KairosControl,
        ok_sources: &BTreeSet<String>,
        items: &dyn ItemRefresher,
    ) -> KairosResult<EngineReport> {
        let now = self.now;
        let off_dates = OffDateBook::new(self.database.connection()).load_off_dates(&control.off_date_set())?;
        let gate = DisplayGate::current(self.database.connection(), self.sources, now, control)?;
        let context = NextContext { now, control: control.clone(), off_dates: off_dates.clone(), gate, health: None };
        let cards_held = CardChooser::new(self.database.connection()).cards_hidden(&context)?;
        let input = RefreshInput { now, off_dates: &off_dates, ok_sources, cards_held };
        let report = items.refresh(self.database, &input)?;
        // 한 이벤트를 건너뛴 것도 사람이 볼 일이다. 나머지는 이미 갱신됐다.
        let skipped = report.skipped > 0;
        self.write_status(!skipped, skipped.then(|| "event_skipped".to_owned()))?;
        Ok(report)
    }

    /// 수집 기록 뒤에 항목을 갱신한다. 원인 코드는 engine.json에 남고 doctor가 드러낸다.
    /// 다음 틱은 저장된 이벤트에서 다시 시작한다.
    pub(super) fn settle_items(
        &self,
        control: &KairosControl,
        ok_sources: &BTreeSet<String>,
        items: &dyn ItemRefresher,
    ) -> Option<EngineReport> {
        match self.guarded(|| self.refresh(control, ok_sources, items)) {
            Ok(report) => Some(report),
            Err(error) => {
                // 기록하지 못해도 수집은 끝까지 간다.
                let _ = self.write_status(false, Some(error.code().to_owned()));
                None
            }
        }
    }

    /// 알림 실패는 다음 틱에 다시 시도한다. doctor의 미전달 건수에 드러난다.
    pub(super) fn alert_p0(&self, control: &KairosControl, notifier: Option<&dyn Notifier>) -> usize {
        let Some(notifier) = notifier else {
            return 0;
        };
        self.guarded(|| {
            let gate = DisplayGate::current(self.database.connection(), self.sources, self.now, control)?;
            Ok(P0Alerts::new(self.database, notifier).notify(self.now, &gate)?.sent)
        })
        .unwrap_or(0)
    }

    fn p1_context(&self) -> KairosResult<NextContext> {
        let control = ControlFile::new(self.home).read();
        let off_dates = OffDateBook::new(self.database.connection()).load_off_dates(&control.off_date_set())?;
        let gate = DisplayGate::current(self.database.connection(), self.sources, self.now, &control)?;
        Ok(NextContext { now: self.now, control, off_dates, gate, health: None })
    }

    /// `alert_p0`와 같이 틱을 실패시키지 않는다. 틱 동안 사용자가 바꾼 일시정지, 포커스, 끄기를 따르려고 설정을 다시 읽는다.
    /// 얼어붙은 틱의 now는 깨어난 시각보다 뒤처져 점심이나 근무 시간 밖에도 send로 판단한다. 표시를 남기지 않고 다음 틱에 맡긴다.
    pub(super) fn alert_p1(&self, notifier: Option<&dyn Notifier>, woke_at: Option<Instant>) -> P1NotifyReport {
        self.guarded(|| {
            let clock = TickClockRepo::new(self.database.connection());
            let resumed_at = clock.note_tick(woke_at.unwrap_or(self.now), woke_at.is_some())?;
            let Some(notifier) = notifier.filter(|_| woke_at.is_none()) else {
                return Ok(P1NotifyReport::default());
            };
            let context = self.p1_context()?;
            let mode = CardChooser::new(self.database.connection()).p1_alert_mode(&context)?;
            let resumed_at = resumed_at.map(Instant::to_iso);
            let input = P1NotifyInput { now: self.now, mode, resumed_at: resumed_at.as_deref() };
            P1Alerts::new(self.database, notifier).notify(&input)
        })
        .unwrap_or_default()
    }
}
