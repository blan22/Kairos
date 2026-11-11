use kairos_core::ports::{Alert, AlertTier, Notifier};
use kairos_core::{Instant, KairosResult};
use kairos_store::Database;
use serde::Serialize;

use crate::repo::{ItemRepo, SuggestionRepo};
use crate::state::DisplayGate;

/// P0 알림의 제목.
pub const P0_TITLE: &str = "긴급";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct NotifyReport {
    pub sent: usize,
    pub failed: usize,
}

/// 살아 있는 P0를 알린다.
pub struct P0Alerts<'a> {
    database: &'a Database,
    notifier: &'a dyn Notifier,
}

impl<'a> P0Alerts<'a> {
    pub fn new(database: &'a Database, notifier: &'a dyn Notifier) -> Self {
        Self { database, notifier }
    }

    pub fn message(meeting: usize, deploy: usize) -> String {
        let mut parts = Vec::new();
        if meeting > 0 {
            parts.push(format!("회의 임박 {meeting}건"));
        }
        if deploy > 0 {
            parts.push(format!("배포 당일 위험 {deploy}건"));
        }
        parts.join(", ")
    }

    /// 아직 알리지 않은 살아 있는 P0를 1회 알린다. 포커스는 무시하지만 표시 게이트는 따른다.
    /// 게이트가 닫혀 있으면 표시를 남기지 않아서, 열린 뒤에도 그 항목이 살아 있으면 그때 알린다.
    /// 알림이 나간 뒤에만 기록한다. 먼저 쓰면 실패한 알림이 재시도 없이 사라진다.
    /// 수집 잠금 안에서 불리므로 같은 항목을 두 프로세스가 동시에 알리지 않는다.
    pub fn notify(&self, now: Instant, gate: &DisplayGate) -> KairosResult<NotifyReport> {
        if !gate.open {
            return Ok(NotifyReport::default());
        }
        let rows = ItemRepo::new(self.database.connection()).unnotified_p0()?;
        if rows.is_empty() {
            return Ok(NotifyReport::default());
        }
        let meeting = rows.iter().filter(|(_, reason)| reason == "meeting_new").count();
        let alert = Alert {
            tier: AlertTier::P0,
            title: P0_TITLE.to_owned(),
            body: Self::message(meeting, rows.len() - meeting),
            items: rows.iter().map(|(id, _)| *id).collect(),
        };
        if !self.notifier.notify(&alert) {
            return Ok(NotifyReport { sent: 0, failed: rows.len() });
        }
        let at = now.to_iso();
        self.database.transaction(|connection| {
            let items = ItemRepo::new(connection);
            let suggestions = SuggestionRepo::new(connection);
            for (id, _) in &rows {
                items.mark_p0_notified(*id, &at)?;
                suggestions.insert_p0_alert(*id, &at)?;
            }
            Ok(())
        })?;
        Ok(NotifyReport { sent: rows.len(), failed: 0 })
    }
}
