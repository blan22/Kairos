use std::collections::HashSet;

use kairos_core::model::SourceName;
use kairos_core::ports::{Alert, AlertTier, Notifier};
use kairos_core::{Instant, KairosResult};
use kairos_policy::{ItemReason, MERGE_WINDOW_MS, P1_NOTIFY_TICK_TRIES, P1_NOTIFY_TRIES, P1_RETRY_GAP_MS};
use kairos_store::Database;
use serde::Serialize;

use crate::cards::{P1AlertMode, label_of, reason_label};
use crate::item::P1Notice;
use crate::repo::{ItemRepo, P1Waiting, SuggestionRepo};

const P1_CARD: &str = "p1-alert";
const P1_CATCHUP_CARD: &str = "p1-catchup";
/// P1 알림의 제목.
pub const P1_TITLE: &str = "새로 온 일";
/// 복귀 묶음 알림의 제목.
pub const P1_CATCHUP_TITLE: &str = "자리를 비운 사이 온 일";

/// 이 값은 DB에서 읽은 규칙 ID로 찾기만 한다. 규칙 ID와 소스 값을 문구에 그대로 넣지 않는다.
fn rule_label(rule_id: &str) -> Option<&'static str> {
    match rule_id {
        "p1.direct_mention" | "p1.notion_mention" => Some("멘션"),
        "p1.review_rerequested" => Some("리뷰 요청"),
        "p1.deploy_owned_comment" => Some("답변 대기"),
        "p1.changes_requested" => Some("변경 요청"),
        "p1.ci_failed" => Some("CI 실패"),
        "p1.deploy_d1" => Some("배포 위험"),
        "p1.app_review_mail" => Some("App Review 메일"),
        "p1.sentry_mail" => Some("Sentry 메일"),
        _ => None,
    }
}

fn source_rank(source: Option<&str>) -> usize {
    let order = SourceName::ALL;
    source.and_then(|source| order.iter().position(|name| name.as_str() == source)).unwrap_or(order.len())
}

/// 문구의 재료 한 줄.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P1Line {
    pub source: Option<String>,
    pub reason: ItemReason,
    /// 항목에 붙은 마지막 P1 사건의 규칙. 배포 때 다시로 깨어난 항목처럼 사건이 없으면 None이다.
    pub rule_id: Option<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct P1NotifyInput<'a> {
    pub now: Instant,
    pub mode: P1AlertMode,
    /// 마지막 잠자기나 재부팅 복귀 시각. 없으면 밀린 건을 묶지 않는다.
    pub resumed_at: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct P1NotifyReport {
    pub sent: usize,
    /// 앞선 알림에 묶여 따로 울리지 않은 건수.
    pub merged: usize,
    /// 알리지 않기로 해 표시만 남긴 건수.
    pub dropped: usize,
    pub failed: usize,
}

/// 30분 합치기의 단위. 채널이 없는 GitHub는 같은 객체다. 항목이 닫혔다 다시 열려도 값이 같다.
fn merge_key<'a>(channel_id: Option<&'a str>, object_key: &'a str) -> &'a str {
    channel_id.unwrap_or(object_key)
}

/// 앞의 몇 번은 틱마다 시도하고 그 뒤는 간격을 둔다. 상한을 넘으면 doctor의 미전달로만 남는다.
fn retry_due(row: &P1Waiting, now: Instant) -> bool {
    if row.p1_notify_attempts < P1_NOTIFY_TICK_TRIES {
        return true;
    }
    if row.p1_notify_attempts >= P1_NOTIFY_TRIES {
        return false;
    }
    // 실패 시각이 해석되지 않으면 NaN이라 비교가 거짓이다.
    let failed = row.p1_notify_failed_at.as_deref().and_then(Instant::parse);
    failed.is_some_and(|failed| now.ms_since(failed) >= P1_RETRY_GAP_MS)
}

/// 새 P1을 한 번의 알림으로 묶어 1회 알린다.
pub struct P1Alerts<'a> {
    database: &'a Database,
    notifier: &'a dyn Notifier,
}

impl<'a> P1Alerts<'a> {
    pub fn new(database: &'a Database, notifier: &'a dyn Notifier) -> Self {
        Self { database, notifier }
    }

    /// 'Slack 멘션 2건, GitHub 리뷰 요청 1건'.
    pub fn message(rows: &[P1Line]) -> String {
        let mut sorted: Vec<&P1Line> = rows.iter().collect();
        sorted.sort_by_key(|row| source_rank(row.source.as_deref()));
        let mut counts: Vec<(String, usize)> = Vec::new();
        for row in sorted {
            let label = row.rule_id.as_deref().and_then(rule_label).unwrap_or(reason_label(row.reason));
            let line = format!("{} {label}", label_of(row.source.as_deref()));
            match counts.iter_mut().find(|(known, _)| *known == line) {
                Some((_, count)) => *count += 1,
                None => counts.push((line, 1)),
            }
        }
        let parts: Vec<String> = counts.iter().map(|(line, count)| format!("{line} {count}건")).collect();
        parts.join(", ")
    }

    fn settle(&self, rows: &[&P1Waiting], at: &str, notice: P1Notice) -> KairosResult<()> {
        if rows.is_empty() {
            return Ok(());
        }
        self.database.transaction(|connection| {
            let items = ItemRepo::new(connection);
            rows.iter().try_for_each(|row| items.settle_notice(row.id, at, notice))
        })
    }

    fn mark_held(&self, rows: &[P1Waiting], at: &str) -> KairosResult<()> {
        let fresh: Vec<&P1Waiting> = rows.iter().filter(|row| row.p1_held_at.is_none()).collect();
        if fresh.is_empty() {
            return Ok(());
        }
        self.database.transaction(|connection| {
            let items = ItemRepo::new(connection);
            fresh.iter().try_for_each(|row| items.mark_held(row.id, at))
        })
    }

    fn mark_failed(&self, rows: &[&P1Waiting], at: &str) -> KairosResult<()> {
        self.database.transaction(|connection| {
            let items = ItemRepo::new(connection);
            rows.iter().try_for_each(|row| items.mark_failed(row.id, at))
        })
    }

    fn record_sent(&self, rows: &[&P1Waiting], at: &str, card: &str) -> KairosResult<()> {
        self.database.transaction(|connection| {
            let items = ItemRepo::new(connection);
            let suggestions = SuggestionRepo::new(connection);
            rows.iter().try_for_each(|row| {
                items.settle_notice(row.id, at, P1Notice::Sent)?;
                suggestions.insert_p1_alert(row.id, card, at)
            })
        })
    }

    /// 30분 안에 나간 알림의 합치기 키와, 그중 복귀 첫 알림이 있는가.
    fn recent_alerts(&self, now: Instant) -> KairosResult<(HashSet<String>, bool)> {
        let rows =
            SuggestionRepo::new(self.database.connection()).recent_alerts(&now.minus_ms(MERGE_WINDOW_MS).to_iso())?;
        let keys = rows.iter().map(|row| merge_key(row.channel_id.as_deref(), &row.object_key).to_owned()).collect();
        let catchup = format!("{P1_CATCHUP_CARD}@");
        Ok((keys, rows.iter().any(|row| row.card_id.starts_with(&catchup))))
    }

    /// 새 P1을 한 번의 알림으로 묶어 1회 알린다. 예산은 쓰지 않는다. 알림 전달은 사용자가 봤다는 증거가 아니므로 seen은 0이다.
    /// 같은 채널이나 같은 객체의 30분 안 건은 앞선 알림에 묶는다.
    /// 복귀 뒤 30분 안에 나간 알림은 밀린 수집의 첫 알림이다. 그 뒤 30분 동안 공백 안에서 일어난 건을 소스와 무관하게 묶는다. 복귀 뒤에 일어난 건은 새 사건이라 묶지 않는다.
    /// 보류했던 건은 끝날 때 알리기로 한 건이라 어디에도 묶지 않는다.
    /// 알림이 나간 뒤에만 기록한다. 실패하면 표시를 남기지 않아 다시 시도하고 doctor의 미전달에 나온다.
    /// 수집 잠금 안에서 불리므로 같은 항목을 두 프로세스가 동시에 알리지 않는다.
    pub fn notify(&self, input: &P1NotifyInput<'_>) -> KairosResult<P1NotifyReport> {
        let P1NotifyInput { now, mode, resumed_at } = *input;
        let at = now.to_iso();
        let mut report = P1NotifyReport::default();
        let rows = ItemRepo::new(self.database.connection()).waiting_p1()?;
        if mode == P1AlertMode::Hold {
            self.mark_held(&rows, &at)?;
            return Ok(report);
        }
        if mode != P1AlertMode::Send {
            // 앞서 보류했거나 전달에 실패한 건은 이 틱에 잡힌 건이 아니다. 끝날 때나 열린 뒤에 알린다.
            let gone: Vec<&P1Waiting> = rows
                .iter()
                .filter(|row| {
                    mode == P1AlertMode::Off || (row.p1_held_at.is_none() && row.p1_notify_failed_at.is_none())
                })
                .collect();
            self.settle(&gone, &at, P1Notice::Dropped)?;
            report.dropped = gone.len();
            return Ok(report);
        }
        let (keys, recovery) = self.recent_alerts(now)?;
        // 해석되지 않는 복귀 시각은 NaN이라 아래의 비교가 모두 거짓이다. 없는 것과 같다.
        let resumed = resumed_at.and_then(Instant::parse);
        let backlog = |row: &P1Waiting| {
            let occurred = row.occurred_at.as_deref().and_then(Instant::parse);
            matches!((occurred, resumed), (Some(occurred), Some(resumed)) if occurred < resumed)
        };
        let covered = |row: &P1Waiting| {
            row.p1_held_at.is_none()
                && (keys.contains(merge_key(row.channel_id.as_deref(), &row.object_key)) || (recovery && backlog(row)))
        };
        let merged: Vec<&P1Waiting> = rows.iter().filter(|row| covered(row)).collect();
        let fresh: Vec<&P1Waiting> = rows.iter().filter(|row| !covered(row) && retry_due(row, now)).collect();
        self.settle(&merged, &at, P1Notice::Merged)?;
        report.merged = merged.len();
        if fresh.is_empty() {
            return Ok(report);
        }
        let lines: Vec<P1Line> = fresh
            .iter()
            .map(|row| P1Line { source: row.source.clone(), reason: row.reason, rule_id: row.rule_id.clone() })
            .collect();
        let recovering = resumed.is_some_and(|resumed| now.ms_since(resumed) <= MERGE_WINDOW_MS);
        let alert = Alert {
            tier: AlertTier::P1,
            title: if recovering { P1_CATCHUP_TITLE } else { P1_TITLE }.to_owned(),
            body: Self::message(&lines),
            items: fresh.iter().map(|row| row.id).collect(),
        };
        if !self.notifier.notify(&alert) {
            self.mark_failed(&fresh, &at)?;
            report.failed = fresh.len();
            return Ok(report);
        }
        let card = format!("{}@{}", if recovering { P1_CATCHUP_CARD } else { P1_CARD }, now.millis());
        if self.record_sent(&fresh, &at, &card).is_err() && self.record_sent(&fresh, &at, &card).is_err() {
            // 나간 알림이지만 기록이 없으면 다시 울린다. 실패로 세어 상한 안에 둔다.
            self.mark_failed(&fresh, &at)?;
            report.failed = fresh.len();
            return Ok(report);
        }
        report.sent = fresh.len();
        Ok(report)
    }
}
