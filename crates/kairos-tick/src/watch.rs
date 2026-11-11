//! watch.tsv. 감시자가 sqlite3만으로 판정할 수 있게 소스별 허용 지연을 적어 둔다.

use std::path::PathBuf;

use kairos_agenda::{ControlFile, SourceHealthBook};
use kairos_core::home::write_file_atomic;
use kairos_core::model::{Source, SourceMode};
use kairos_core::{Instant, KairosHome, KairosResult};
use kairos_ledger::OffDateBook;
use kairos_ops::MaintenanceRecord;
use kairos_store::Connection;

const MIN_ALLOWED_DELAY_SECONDS: i64 = 600;

pub fn watch_path(home: &KairosHome) -> PathBuf {
    home.file("watch.tsv")
}

/// 감시자가 판정할 요일과 시간. `시작분\t끝분\tISO 요일 숫자` 한 줄이다. 설정의 schedule.collect, schedule.days다.
pub fn watch_window_path(home: &KairosHome) -> PathBuf {
    home.file("watch-window.tsv")
}

/// 프로필의 수집 시간과 근무 요일을 감시자 모양으로 쓴다.
pub fn watch_window_line() -> String {
    let schedule = &kairos_core::profile::current().schedule;
    format!("{}\t{}\t{}\n", schedule.tick.start, schedule.tick.end, schedule.days.iso_digits())
}

#[derive(Clone, Copy, Debug)]
pub struct WatchFile<'a> {
    connection: &'a Connection,
    home: &'a KairosHome,
}

impl<'a> WatchFile<'a> {
    pub const fn new(connection: &'a Connection, home: &'a KairosHome) -> Self {
        Self { connection, home }
    }

    fn alerts(&self, now: Instant, leftover: usize) -> KairosResult<Vec<&'static str>> {
        let book = SourceHealthBook::new(self.connection);
        let invariants = MaintenanceRecord::read(self.home).map(|record| record.invariants);
        let violated = invariants.is_some_and(|found| {
            found.events_without_disposition > 0
                || found.closures_without_evidence > 0
                || found.item_closures_without_evidence > 0
        });
        let mut alerts = Vec::new();
        if violated {
            alerts.push("invariant violated");
        }
        if !book.coverage_gaps(now)?.is_empty() {
            alerts.push("coverage gap");
        }
        if book.notion_format_suspects(now)? > 0 {
            alerts.push("notion format");
        }
        if leftover > 0 {
            alerts.push("large response files");
        }
        Ok(alerts)
    }

    /// `leftover`는 지우지 못한 큰 응답 파일 수다.
    pub fn write(&self, sources: &[&dyn Source], now: Instant, leftover: usize) -> KairosResult<()> {
        let alerts = self.alerts(now, leftover)?;
        // 공휴일과 kairos off도 판정에서 뺀다. 이날은 plan-tick이 호출을 시간당 1회로 줄여 지연처럼 보인다.
        // 공유 캘린더의 연차는 빼지 않는다. plan-tick도 같은 기준으로 호출을 유지한다.
        let control = ControlFile::new(self.home).read();
        let off_dates = OffDateBook::new(self.connection).load_watch_off_dates(&control.off_date_set())?;
        let active = i32::from(off_dates.is_workday(now.local_date()));
        let mut lines = vec![format!("active\t{active}")];
        lines.extend(alerts.iter().map(|alert| format!("alert\t{alert}")));
        for source in sources {
            if matches!(source.mode(), SourceMode::NotConnected { .. }) {
                continue;
            }
            for query in source.queries() {
                let allowed = (query.interval_seconds * 3).max(MIN_ALLOWED_DELAY_SECONDS);
                lines.push(format!("source\t{}\t{}\t{allowed}", source.name(), query.id));
            }
        }
        write_file_atomic(&watch_window_path(self.home), &watch_window_line())?;
        write_file_atomic(&watch_path(self.home), &format!("{}\n", lines.join("\n")))
    }
}
