//! kairos doctor. 줄은 여기서 만들고 kairos-cli가 찍는다.

mod lines;
mod permissions;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kairos_agenda::{
    ControlFile, DisplayGate, HealthStatus, INDEFINITE, ItemRepo, KairosControl, SourceHealth, SourceHealthBook,
    StateBuilder,
};
use kairos_core::model::{Outcome, Source, SourceName};
use kairos_core::ports::Clock;
use kairos_core::process::ProcessRunner;
use kairos_core::{Instant, KairosHome, KairosResult};
use kairos_ledger::{OffDateBook, check_invariants};
use kairos_ops::{LABELS, MaintenanceRecord, ResumeFile, SLEEP_INTERRUPTED, ToolResults};
use kairos_store::Database;

use crate::halt::{HaltFlag, js_number_text};
use crate::mode::{ClaudeProbe, SystemClaudeProbe};
use crate::registry::SourceFailure;
use crate::repo::TickRunRepo;
use crate::tick::AuthFile;
use lines::{code_note, health_label};
use permissions::mode_of;
pub use permissions::permission_problems;

/// 설계 9.5: 로그인 만료 7일 전부터 경고한다.
pub const LOGIN_WARN_DAYS: i64 = 7;
/// 신원 단언이 이 기간 동안 한 번도 없으면 문제로 올린다. Slack은 내 글이 없는 날에 값이 없는 것이 정상이다.
pub const IDENTITY_STALE_DAYS: i64 = 3;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug)]
pub struct DoctorConfig {
    pub home: KairosHome,
    /// ~/.claude가 있는 폴더.
    pub user_home: PathBuf,
    /// 프로세스 환경. KAIROS_CLAUDE와 LaunchAgents를 찾을 HOME을 읽는다.
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoctorReport {
    pub lines: Vec<String>,
    /// 문제가 있으면 1. 경고 한 줄이라도 있으면 1이다.
    pub exit_code: i32,
    pub problems: Vec<String>,
}

pub struct Doctor {
    config: DoctorConfig,
    clock: Arc<dyn Clock>,
    runner: Arc<dyn ProcessRunner>,
    sources: Vec<Arc<dyn Source>>,
    failures: Vec<SourceFailure>,
    login: Option<Arc<dyn ClaudeProbe>>,
}

struct Findings {
    lines: Vec<String>,
    problems: Vec<String>,
}

fn identity_line(runs: &TickRunRepo<'_>, row: &SourceHealth) -> KairosResult<String> {
    let Some(expected) = row.source.self_identity() else {
        return Ok(if row.status == HealthStatus::NotConnected {
            String::new()
        } else if row.source.asserts_identity() {
            " 본인 식별자 설정 없음".to_owned()
        } else {
            " 신원 단언 없음".to_owned()
        });
    };
    Ok(match runs.last_ok_identity(row.source, &row.query)? {
        None => String::new(),
        Some(None) => " 신원 확인 기록 없음".to_owned(),
        Some(Some(identity)) if identity == expected => format!(" 신원 일치({expected})"),
        Some(Some(_)) => " 신원 불일치".to_owned(),
    })
}

/// ok 수집이 이어지는데 신원을 확인한 기록이 오래되었는가.
fn identity_stale(runs: &TickRunRepo<'_>, source: SourceName, at: Instant) -> KairosResult<bool> {
    if source.self_identity().is_none() {
        return Ok(false);
    }
    let (first, asserted) = runs.identity_span(source)?;
    let since = asserted.or(first).as_deref().and_then(Instant::parse);
    Ok(since.is_some_and(|since| at.ms_since(since) > IDENTITY_STALE_DAYS * DAY_MS))
}

fn display_line(control: &KairosControl, gate: &DisplayGate, at: Instant) -> String {
    if control.is_paused(at) {
        return if control.paused_until() == Some(INDEFINITE) { "일시정지" } else { "일시정지(기한 있음)" }.to_owned();
    }
    if !gate.open {
        return format!("꺼짐({})", gate.message.as_deref().unwrap_or("undefined"));
    }
    if gate.mode == kairos_agenda::DisplayMode::On {
        "켜짐(사용자 지정)"
    } else {
        "켜짐(기록 1근무일 지남)"
    }
    .to_owned()
}

fn health_row(runs: &TickRunRepo<'_>, row: &SourceHealth, found: &mut Findings) -> KairosResult<()> {
    let label = row.source.label();
    let mut line = format!("  {label} {}: {}", row.query, health_label(row.status));
    if row.status == HealthStatus::NotConnected {
        line.push_str(" (not_connected)");
    }
    if let Some(delay) = row.delay_minutes {
        let _ = write!(line, " {delay}분");
    }
    if let Some(outcome) = row.last_outcome {
        let _ = write!(line, " 마지막 결과 {outcome}{}", code_note(row));
    }
    if let Some(last_ok) = row.last_ok_at.as_deref().filter(|at| !at.is_empty()) {
        let _ = write!(line, " 마지막 ok {last_ok}");
    }
    let identity = identity_line(runs, row)?;
    line.push_str(&identity);
    if let Some(note) = row.note.as_deref().filter(|note| !note.is_empty()) {
        let _ = write!(line, " {note}");
    }
    found.lines.push(line);
    if row.status == HealthStatus::Delayed {
        found.problems.push(SourceHealthBook::delay_note(label, row.delay_minutes));
    }
    let failed = row.last_outcome.is_some_and(|outcome| outcome != Outcome::Ok && outcome != Outcome::NotConnected);
    if failed && row.last_error_code.as_deref() != Some(SLEEP_INTERRUPTED) {
        let outcome = row.last_outcome.map_or("", Outcome::as_str);
        found.problems.push(format!("{label} 마지막 수집 {outcome}"));
    }
    if identity.contains("불일치") {
        found.problems.push(format!("{label} 신원 불일치"));
    }
    Ok(())
}

impl Doctor {
    pub fn new(
        config: DoctorConfig,
        clock: Arc<dyn Clock>,
        runner: Arc<dyn ProcessRunner>,
        sources: Vec<Arc<dyn Source>>,
    ) -> Self {
        Self { config, clock, runner, sources, failures: Vec::new(), login: None }
    }

    #[must_use]
    pub fn with_failures(mut self, failures: Vec<SourceFailure>) -> Self {
        self.failures = failures;
        self
    }

    /// 로그인 만료 시각을 새로 읽는 쪽. 기본은 저장된 값이다.
    #[must_use]
    pub fn with_login_probe(mut self, login: Arc<dyn ClaudeProbe>) -> Self {
        self.login = Some(login);
        self
    }

    fn login_expiry(&self) -> KairosResult<Option<String>> {
        if let Some(login) = &self.login {
            return login.login_expiry();
        }
        if let Some(record) = AuthFile::new(&self.config.home).read() {
            return Ok(record.login_expires_at);
        }
        let bin = self.config.env.get("KAIROS_CLAUDE").cloned();
        SystemClaudeProbe::new(self.runner.clone(), bin, self.config.env.clone()).login_expiry()
    }

    fn login_lines(&self, at: Instant, found: &mut Findings) -> KairosResult<()> {
        let Some(expires) = self.login_expiry()?.filter(|expires| !expires.is_empty()) else {
            let failed = if AuthFile::new(&self.config.home).exists() { " (마지막 확인 실패)" } else { "" };
            found.lines.push(format!("로그인 만료 시각 모름{failed}"));
            return Ok(());
        };
        let Some(end) = Instant::parse(&expires) else {
            found.lines.push(format!("로그인 만료 시각 모름 ({expires})"));
            return Ok(());
        };
        let days = (end.ms_since(at) as f64 / DAY_MS as f64).ceil() as i64;
        if days <= 0 {
            found.lines.push(format!("로그인 만료됨({expires}). claude.ai 재로그인이 필요합니다"));
            found.problems.push("로그인 만료".to_owned());
            return Ok(());
        }
        let warn = days <= LOGIN_WARN_DAYS;
        let hint = if warn { " 곧 만료. claude.ai 재로그인 필요" } else { "" };
        found.lines.push(format!("로그인 만료 D-{days} ({expires}){hint}"));
        if warn {
            found.problems.push(format!("로그인 만료 D-{days}"));
        }
        Ok(())
    }

    fn halt_lines(&self, found: &mut Findings) {
        let Some(halt) = HaltFlag::new(&self.config.home, self.clock.clone()).read() else {
            return;
        };
        let known = |value: Option<f64>| value.map_or_else(|| "알 수 없음".to_owned(), js_number_text);
        found.lines.push(format!(
            "수집 중단: {} ({}, num_turns {}, 비용 {}). 원인 확인 뒤 kairos collect --resume",
            halt.reason,
            halt.at.as_deref().unwrap_or("undefined"),
            known(halt.num_turns),
            known(halt.cost_usd),
        ));
        found.problems.push("수집 중단".to_owned());
    }

    fn launch_agents_installed(&self) -> usize {
        let user = self.config.env.get("HOME").map_or("", String::as_str);
        let agents = Path::new(user).join("Library").join("LaunchAgents");
        LABELS.iter().filter(|label| agents.join(format!("{label}.plist")).exists()).count()
    }

    fn sources_section(
        &self,
        database: &Database,
        control: &KairosControl,
        at: Instant,
        found: &mut Findings,
    ) -> KairosResult<()> {
        let sources: Vec<&dyn Source> = self.sources.iter().map(AsRef::as_ref).collect();
        let home = &self.config.home;
        let runs = TickRunRepo::new(database.connection());
        let book = SourceHealthBook::new(database.connection());
        let off_dates = OffDateBook::new(database.connection()).load_watch_off_dates(&control.off_date_set())?;
        let resumed_at = ResumeFile::new(home).read().map(Instant::to_iso);
        let health = book.source_health(&sources, at, &off_dates, resumed_at.as_deref())?;
        let invariants = check_invariants(database.connection())?;
        let version = database.user_version()?;
        let journal = runs.journal_mode()?;

        found.lines.push(format!("데이터 폴더 {} (권한 {})", home.root().display(), mode_of(home.root())?));
        found.lines.push(format!("DB 스키마 v{version}, {journal}, 권한 {}", mode_of(&home.db_path())?));
        found.lines.push("소스".to_owned());
        for row in &health {
            health_row(&runs, row, found)?;
        }
        let mut seen = Vec::new();
        for row in &health {
            if seen.contains(&row.source) {
                continue;
            }
            seen.push(row.source);
            if identity_stale(&runs, row.source, at)? {
                found.problems.push(format!("{} 신원 단언 {IDENTITY_STALE_DAYS}일 넘게 없음", row.source.label()));
            }
        }
        for notice in book.notice_lines(at)? {
            found.lines.push(format!("알림: {notice}"));
        }
        for gap in book.coverage_gaps(at)? {
            found.problems.push(format!("덮지 못한 구간 {}", SourceHealthBook::gap_line(&gap)));
        }
        for failure in &self.failures {
            found.lines.push(format!("  소스 파일 오류 {}: {}", failure.file, failure.error_code));
            found.problems.push(format!("소스 파일 오류 {}", failure.file));
        }
        found.lines.push(format!(
            "불변식: 처분 없는 이벤트 {}건, 근거 없는 종료 {}건, 미분류 {}건, 근거 없는 항목 종료 {}건",
            invariants.events_without_disposition,
            invariants.closures_without_evidence,
            invariants.unclassified,
            invariants.item_closures_without_evidence,
        ));
        let notion_unknown = runs.notion_unknown_count()?;
        if notion_unknown > 0 {
            found
                .lines
                .push(format!("Notion 미분류 {notion_unknown}건: 알림 메일 형식이 바뀌었는지 확인이 필요합니다"));
        }
        let suspects = book.notion_format_suspects(at)?;
        if suspects > 0 {
            found.problems.push(SourceHealthBook::notion_format_line(suspects));
        }
        if invariants.events_without_disposition > 0 {
            found.problems.push("처분 없는 이벤트".to_owned());
        }
        if invariants.closures_without_evidence > 0 {
            found.problems.push("근거 없는 종료".to_owned());
        }
        if invariants.item_closures_without_evidence > 0 {
            found.problems.push("근거 없는 항목 종료".to_owned());
        }
        Ok(())
    }

    fn items_section(
        &self,
        database: &Database,
        control: &KairosControl,
        gate: &DisplayGate,
        at: Instant,
        found: &mut Findings,
    ) -> KairosResult<()> {
        let counts = ItemRepo::new(database.connection()).counts(at)?;
        found.lines.push(format!(
            "항목: 열림 P0 {}건, P1 {}건, P2 {}건, 직접 추가 {}건, 대기 {}건, 수락 {}건, 기준선 유지 {}건, 응답 없이 넘어온 P1 {}건",
            counts.open.p0,
            counts.open.p1,
            counts.open.p2,
            counts.manual,
            counts.parked,
            counts.accepted,
            counts.baseline,
            counts.carried_p1,
        ));
        if let Some(engine) = StateBuilder::read_engine_status(&self.config.home).filter(|engine| !engine.ok) {
            let code = engine.error_code.as_deref().unwrap_or("알 수 없음");
            found.problems.push(format!("항목 엔진 실패 {code} ({})", engine.at));
        }
        if gate.open && counts.p0_unnotified > 0 {
            found.problems.push(format!("P0 알림 미전달 {}건", counts.p0_unnotified));
        }
        let p1_state = if control.p1_notify_on() { "켜짐" } else { "꺼짐(켜기: kairos notify on)" };
        found.lines.push(format!("P1 알림: {p1_state}, 보류 {}건, 미전달 {}건", counts.p1_held, counts.p1_undelivered));
        found.lines.push(format!(
            "P1 알림 오늘: 전달 {}건, 묶음 {}건, 미발송 {}건",
            counts.p1_sent_today, counts.p1_merged_today, counts.p1_dropped_today,
        ));
        if counts.p1_gave_up > 0 {
            found.lines.push(format!("P1 알림 재시도 끝남 {}건: 항목은 패널에 있습니다", counts.p1_gave_up));
        }
        // 보류는 정상이다. 전달을 시도해 실패한 건만 문제다.
        if counts.p1_undelivered > 0 {
            found.problems.push(format!("P1 알림 미전달 {}건", counts.p1_undelivered));
        }
        Ok(())
    }

    fn upkeep_section(
        &self,
        control: &KairosControl,
        gate: &DisplayGate,
        at: Instant,
        found: &mut Findings,
    ) -> KairosResult<()> {
        let home = &self.config.home;
        let permissions = permission_problems(home)?;
        found.lines.push(if permissions.is_empty() { "권한: 폴더 700, 파일 600 확인" } else { "권한 문제" }.to_owned());
        found.lines.extend(permissions.iter().map(|problem| format!("  {problem}")));
        found.problems.extend(permissions.iter().map(|_| "권한".to_owned()));

        found.lines.push(MaintenanceRecord::read(home).map_or_else(
            || "정비: 아직 실행 안 됨".to_owned(),
            |record| {
                format!(
                    "정비: {} (백업 {}, 삭제 이벤트 {}건, 객체 {}건, 실행 기록 {}건)",
                    record.date,
                    if record.backup_file.as_deref().is_some_and(|file| !file.is_empty()) {
                        "생성"
                    } else {
                        "이미 있음"
                    },
                    record.events_deleted,
                    record.objects_deleted,
                    record.runs_deleted,
                )
            },
        ));
        let installed = self.launch_agents_installed();
        found.lines.push(if installed == LABELS.len() {
            "LaunchAgent: 2개 설치됨".to_owned()
        } else {
            format!("LaunchAgent: {installed}개 설치됨")
        });

        found.lines.push(format!("표시: {}", display_line(control, gate, at)));
        let lock = kairos_agenda::SessionApproval::new(&self.config.home).lock(control);
        found.lines.push(match control.session_test_passed_at().filter(|_| control.sessions_allowed()) {
            _ if lock == kairos_agenda::SessionLock::ConfigChanged => {
                "세션: 잠김. 설정이 바뀌어 차단 시험을 다시 본 뒤 kairos sessions on".to_owned()
            }
            Some(passed) => format!("세션: 열림({passed}에 켬)"),
            None if control.sessions_allowed() => "세션: 열림(undefined에 켬)".to_owned(),
            None => "세션: 잠김. 차단 시험 뒤 kairos sessions on".to_owned(),
        });
        Ok(())
    }

    pub fn diagnose(&self) -> KairosResult<DoctorReport> {
        let at = self.clock.now();
        let home = &self.config.home;
        let database = Database::open(home)?;
        let mut found = Findings { lines: Vec::new(), problems: Vec::new() };
        let control = ControlFile::new(home).read();
        self.sources_section(&database, &control, at, &mut found)?;

        let sources: Vec<&dyn Source> = self.sources.iter().map(AsRef::as_ref).collect();
        let gate = DisplayGate::current(database.connection(), &sources, at, &control)?;
        self.items_section(&database, &control, &gate, at, &mut found)?;

        let leftover = ToolResults::for_collector(home, &self.config.user_home).entries()?.len();
        found.lines.push(format!("큰 응답 임시 파일: {leftover}건"));
        if leftover > 0 {
            found.problems.push(format!("큰 응답 임시 파일 {leftover}건 남음"));
        }

        self.halt_lines(&mut found);
        self.login_lines(at, &mut found)?;
        self.upkeep_section(&control, &gate, at, &mut found)?;
        let exit_code = i32::from(!found.problems.is_empty());
        Ok(DoctorReport { lines: found.lines, exit_code, problems: found.problems })
    }
}
