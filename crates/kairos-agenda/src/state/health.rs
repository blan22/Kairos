//! 소스 건강과 사용자가 알아야 할 수집 사실.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use kairos_core::model::{Outcome, QuerySpec, Source, SourceMode, SourceName};
use kairos_core::{Instant, KairosResult, OffDates, string_enum};
use kairos_ledger::{CoverageGapRow, EventRepo, MANUAL_SKIP, OffDateBook, RunSummary, SourceRunRepo};
use kairos_policy::{tick_end_minutes, tick_start_minutes};
use kairos_store::Connection;
use serde::Serialize;

pub const MIN_ALERT_SECONDS: i64 = 600;
/// 덮지 못한 구간을 알리는 기간. 복구할 방법이 없는 사실이라 이 기간이 지나면 기록으로만 남는다.
pub const GAP_NOTICE_DAYS: i64 = 7;
/// 최근 이 시간 안의 건수만 본다. 한 건이 30일 내내 경고로 남지 않게 한다.
pub const NOTION_FORMAT_WINDOW_MS: i64 = 24 * 3_600_000;
pub const NOTION_FORMAT_THRESHOLD: i64 = 3;

const DAY_MS: i64 = 86_400_000;
const GAP_CODES: [&str; 8] = [
    "window_capped",
    "record_quarantined",
    MANUAL_SKIP,
    "notion_sender_mismatch",
    "notion_quarantined",
    "notion_block_cap",
    "page_cap_stalled",
    "node_quarantined",
];

string_enum! {
    /// ok 기준 안에 정상 수집이 있다. delayed 마지막 ok가 max(주기 x 3, 10분)을 넘었다.
    /// never 아직 ok가 없다. not_connected 연결이 없다. quiet 수집 시간 밖이거나 쉬는 날이라 판정하지 않는다.
    pub enum HealthStatus {
        Ok => "ok",
        Delayed => "delayed",
        Never => "never",
        NotConnected => "not_connected",
        Quiet => "quiet",
    }
}

/// 질의 하나의 건강. 필드 순서가 state.json의 키 순서다. status는 TS가 펼친 뒤에 붙여 맨 뒤다.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceHealth {
    pub source: SourceName,
    pub query: String,
    pub last_outcome: Option<Outcome>,
    pub last_error_code: Option<String>,
    pub last_ok_at: Option<String>,
    pub delay_minutes: Option<i64>,
    pub note: Option<String>,
    pub status: HealthStatus,
}

/// 소스 하나의 건강. 질의가 여럿이면 가장 나쁜 상태의 질의를 쓴다. 호스트가 수집 상태로 그리는 값이라 고정 낱말과 시각뿐이다.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceStatus {
    pub source: SourceName,
    pub label: String,
    pub status: HealthStatus,
    /// 상태를 정한 질의의 마지막 정상 수집 시각. 주기가 다른 질의의 시각은 섞지 않는다.
    pub last_ok_at: Option<String>,
    pub delay_minutes: Option<i64>,
}

/// 사용자가 먼저 알아야 하는 순서. 지금 늦는 것이 맨 앞이다.
const fn severity(status: HealthStatus) -> u8 {
    match status {
        HealthStatus::Delayed => 4,
        HealthStatus::Never => 3,
        HealthStatus::NotConnected => 2,
        HealthStatus::Quiet => 1,
        HealthStatus::Ok => 0,
    }
}

/// 같은 상태의 질의 둘에서 보여 줄 시각. 늦는 중이면 가장 오래 밀린 것을, 아니면 가장 최근에 성공한 것을 쓴다.
/// 주기가 하루인 질의가 정상인 소스를 '23시간 전 수집'으로 보이게 하지 않기 위해서다.
fn shown_last_ok(status: HealthStatus, left: Option<String>, right: Option<&str>) -> Option<String> {
    let wanted = if status == HealthStatus::Delayed { Ordering::Less } else { Ordering::Greater };
    match (left, right) {
        (Some(left), Some(right)) => {
            let take_right = Instant::compare_texts(right, &left) == Some(wanted);
            Some(if take_right { right.to_owned() } else { left })
        }
        (left, right) => left.or_else(|| right.map(str::to_owned)),
    }
}

/// 최근의 덮지 못한 구간.
pub type CoverageGap = CoverageGapRow;

/// 그날 수집이 시작된 순간. 밤사이 공백을 지연으로 세지 않기 위해 쓴다.
fn tick_start(now: Instant) -> Instant {
    now.minus_ms((i64::from(now.local_minutes()) - i64::from(tick_start_minutes())) * 60_000)
}

struct HealthInput<'a> {
    source: &'a dyn Source,
    query: &'a QuerySpec,
    last: Option<RunSummary>,
    last_ok: Option<RunSummary>,
    now: Instant,
    collecting: bool,
    resumed_at: Option<&'a str>,
}

fn health_of(input: HealthInput<'_>) -> SourceHealth {
    let HealthInput { source, query, last, last_ok, now, collecting, resumed_at } = input;
    let finished_at = last_ok.as_ref().map(|run| run.finished_at.clone());
    let base = |status: HealthStatus, delay_minutes: Option<i64>, note: Option<String>| SourceHealth {
        source: source.name(),
        query: query.id.clone(),
        last_outcome: last.as_ref().map(|run| run.outcome),
        last_error_code: last.as_ref().and_then(|run| run.error_code.clone()),
        last_ok_at: finished_at.clone(),
        delay_minutes,
        note,
        status,
    };
    if let SourceMode::NotConnected { reason } = source.mode() {
        return base(HealthStatus::NotConnected, None, Some(reason.to_owned()));
    }
    let Some(last_ok) = last_ok.as_ref() else {
        return base(HealthStatus::Never, None, None);
    };
    if !collecting {
        return base(HealthStatus::Quiet, None, None);
    }
    // 잠자기로 쉰 시간은 지연이 아니다. 깨어난 시각부터 다시 재므로 깨어난 뒤에도 ok가 없으면 그대로 경고가 된다.
    let resumed =
        resumed_at.and_then(Instant::parse).filter(|resumed| *resumed <= now).unwrap_or(Instant::from_millis(0));
    let threshold = (query.interval_seconds * 3).max(MIN_ALERT_SECONDS) * 1000;
    // 해석할 수 없는 시각은 NaN이라 `age <= threshold`가 거짓이고 지연 분은 null로 나간다.
    let age = Instant::parse(&last_ok.finished_at).map(|ok| now.ms_since(ok.max(tick_start(now)).max(resumed)));
    match age {
        Some(age) if age <= threshold => base(HealthStatus::Ok, None, None),
        Some(age) => base(HealthStatus::Delayed, Some(age.div_euclid(60_000)), None),
        None => base(HealthStatus::Delayed, None, None),
    }
}

/// 건강 판정과 그 문구.
#[derive(Clone, Copy, Debug)]
pub struct SourceHealthBook<'a> {
    connection: &'a Connection,
}

impl<'a> SourceHealthBook<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    pub fn source_health(
        &self,
        sources: &[&dyn Source],
        now: Instant,
        off_dates: &OffDates,
        resumed_at: Option<&str>,
    ) -> KairosResult<Vec<SourceHealth>> {
        let minutes = now.local_minutes();
        let collecting =
            off_dates.is_workday(now.local_date()) && (tick_start_minutes()..tick_end_minutes()).contains(&minutes);
        let runs = SourceRunRepo::new(self.connection);
        let mut health = Vec::new();
        for source in sources {
            for query in source.queries() {
                let last = runs.latest(source.name(), &query.id, false)?;
                let last_ok = runs.latest(source.name(), &query.id, true)?;
                health.push(health_of(HealthInput {
                    source: *source,
                    query,
                    last,
                    last_ok,
                    now,
                    collecting,
                    resumed_at,
                }));
            }
        }
        Ok(health)
    }

    /// 질의별 건강을 소스별 한 줄로 줄인다. 순서는 처음 나온 소스 순서다.
    pub fn summary(health: &[SourceHealth]) -> Vec<SourceStatus> {
        let mut rows: Vec<SourceStatus> = Vec::new();
        for row in health {
            let Some(found) = rows.iter_mut().find(|status| status.source == row.source) else {
                rows.push(SourceStatus {
                    source: row.source,
                    label: row.source.label().to_owned(),
                    status: row.status,
                    last_ok_at: row.last_ok_at.clone(),
                    delay_minutes: row.delay_minutes,
                });
                continue;
            };
            match severity(row.status).cmp(&severity(found.status)) {
                Ordering::Greater => {
                    found.status = row.status;
                    found.last_ok_at.clone_from(&row.last_ok_at);
                    found.delay_minutes = row.delay_minutes;
                }
                Ordering::Equal => {
                    found.last_ok_at = shown_last_ok(row.status, found.last_ok_at.take(), row.last_ok_at.as_deref());
                    found.delay_minutes = found.delay_minutes.max(row.delay_minutes);
                }
                Ordering::Less => {}
            }
        }
        rows
    }

    /// 지연 문구. 분을 모르면 지연 사실만 쓴다.
    pub fn delay_note(label: &str, minutes: Option<i64>) -> String {
        minutes.map_or_else(|| format!("{label} 지연"), |minutes| format!("{label} {minutes}분 지연"))
    }

    /// 건강 경고 한 줄. 문제가 없으면 None이다. '0건'이 아니라 'Slack 12분 지연'처럼 쓴다.
    pub fn health_line(health: &[SourceHealth]) -> Option<String> {
        let mut seen = BTreeSet::new();
        let mut parts = Vec::new();
        for row in health {
            let label = row.source.label();
            let part = match row.status {
                HealthStatus::Delayed => Self::delay_note(label, row.delay_minutes),
                HealthStatus::Never => format!("{label} 수집 기록 없음"),
                HealthStatus::NotConnected => format!("{label} 미연결"),
                HealthStatus::Ok | HealthStatus::Quiet => continue,
            };
            if seen.insert(part.clone()) {
                parts.push(part);
            }
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }

    /// 최근의 덮지 못한 구간. 겹쳐 읽기로 같은 건이 여러 틱에 찍히므로 질의와 코드별로 묶는다.
    pub fn coverage_gaps(&self, now: Instant) -> KairosResult<Vec<CoverageGap>> {
        let since = now.minus_ms(GAP_NOTICE_DAYS * DAY_MS).to_iso();
        SourceRunRepo::new(self.connection).coverage_gaps(&GAP_CODES, &since)
    }

    pub fn gap_line(gap: &CoverageGap) -> String {
        // 구간 끝이 없으면 TS 템플릿이 null을 글자로 찍는다.
        let from = gap.window_from.as_deref().unwrap_or("null");
        let to = gap.window_to.as_deref().unwrap_or("null");
        let detail = match gap.error_code.as_str() {
            "window_capped" => format!("{from} 이전을 읽지 못함"),
            "record_quarantined" => format!("{from}~{to} 메시지 격리, 원본 앱에서 확인 필요"),
            MANUAL_SKIP => format!("{from}~{to} 건너뜀"),
            "notion_sender_mismatch" => format!("{from}~{to} 발신자 형식을 읽지 못한 알림 메일이 있음"),
            "notion_quarantined" => format!("{from}~{to} 읽지 못한 알림 메일 격리, Gmail에서 확인 필요"),
            "notion_block_cap" => format!("{from}~{to} 메일 한 통의 블록이 상한을 넘어 일부만 저장"),
            "page_cap_stalled" => format!("{from}~{to} 쪽 상한으로 전진 못함"),
            "node_quarantined" => format!("{from}~{to} 끝까지 읽지 못한 항목을 넘김, GitHub에서 확인 필요"),
            code => code.to_owned(),
        };
        format!("{} {}: {detail}", gap.source.label(), gap.query)
    }

    /// 동작 문구를 못 찾은 블록이 몰리면 알림 메일 형식이 바뀐 것이다. 기준 미만이면 0이다.
    pub fn notion_format_suspects(&self, now: Instant) -> KairosResult<i64> {
        let since = now.minus_ms(NOTION_FORMAT_WINDOW_MS).to_iso();
        let count = EventRepo::new(self.connection).count_notion_unknown_since(&since)?;
        Ok(if count >= NOTION_FORMAT_THRESHOLD { count } else { 0 })
    }

    pub fn notion_format_line(count: i64) -> String {
        format!("Notion 알림 메일 형식 변경 의심 {count}건. 파서를 고친 뒤 kairos collect --reread-notion")
    }

    /// 수집은 그대로지만 사용자가 알아야 할 사실. list 첫 줄과 doctor에 나온다.
    pub fn notice_lines(&self, now: Instant) -> KairosResult<Vec<String>> {
        let shared = OffDateBook::new(self.connection).shared_off_dates(now)?;
        let suspects = self.notion_format_suspects(now)?;
        let mut lines: Vec<String> = self.coverage_gaps(now)?.iter().map(Self::gap_line).collect();
        if suspects > 0 {
            lines.push(Self::notion_format_line(suspects));
        }
        if !shared.is_empty() {
            let dates: Vec<String> = shared.iter().map(ToString::to_string).collect();
            lines.push(format!("공유 캘린더의 내 연차 {} (수집과 감시는 계속)", dates.join(", ")));
        }
        Ok(lines)
    }
}
