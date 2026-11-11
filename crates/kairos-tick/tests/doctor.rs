//! doctor 시험(R12, R28 포함).
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_agenda::{ControlFile, DisplayStart, EngineReport, StateBuilder};
use kairos_core::model::{QuerySpec, Source, SourceName, SourceResult};
use kairos_core::ports::FixedClock;
use kairos_core::process::ProcessOutput;
use kairos_store::Database;
use kairos_tick::{CollectStatus, Doctor, DoctorConfig, Halt, HaltFlag, SourceFailure, permission_problems};

const DAY_MS: i64 = 86_400_000;

fn days_after(when: &str, days: i64) -> String {
    at(when).plus_ms(days * DAY_MS).to_iso()
}

fn tick_source(name: SourceName) -> Arc<dyn Source> {
    direct(name, |_| Vec::new())
}

fn synthetic_sources() -> Vec<Arc<dyn Source>> {
    vec![tick_source(SourceName::Github), unconnected(SourceName::Notion, vec![QuerySpec::new("mail", 900, 0)])]
}

fn gmail_sources() -> Vec<Arc<dyn Source>> {
    vec![
        direct_with(SourceName::Gmail, vec![QuerySpec::new("recent", 3600, 0)], |_| Vec::new()),
        direct_with(SourceName::Slack, vec![QuerySpec::new("from_me", 180, 600)], |_| Vec::new()),
    ]
}

fn run(source: SourceName, query: &str) -> SourceResult {
    SourceResult { source, query: query.into(), ..result() }
}

#[test]
fn 표시_시작은_t0_이후_1근무일이_끝나는_20시_kst다() {
    let start = |t0: &str, now: &str, off_dates: &kairos_core::OffDates| {
        DisplayStart::compute(Some(t0), at(now), off_dates).unwrap()
    };
    let starts = |t0: &str, off_dates: &kairos_core::OffDates| start(t0, T3, off_dates).starts_at.unwrap().to_iso();
    // 월요일 10:00 이전에 시작 -> 월요일 20:00 KST = 11:00Z
    let monday = start("2026-03-02T00:10:00.000Z", T3, &none());
    assert_eq!(monday.t0.as_deref(), Some("2026-03-02T00:10:00.000Z"));
    assert_eq!(monday.starts_at.unwrap().to_iso(), "2026-03-02T11:00:00.000Z");
    assert!(!monday.ready);
    // 월요일 10:00 이후 시작 -> 화요일
    assert_eq!(starts("2026-03-02T05:00:00.000Z", &none()), "2026-03-03T11:00:00.000Z");
    // 금요일 오후 -> 월요일
    assert_eq!(starts("2026-03-06T05:00:00.000Z", &none()), "2026-03-09T11:00:00.000Z");
    // 다음 월요일이 쉬는 날이면 화요일
    assert_eq!(starts("2026-03-06T05:00:00.000Z", &off(&["2026-03-09"])), "2026-03-10T11:00:00.000Z");
    assert!(start("2026-03-02T00:10:00.000Z", "2026-03-02T11:00:00.000Z", &none()).ready);
    let empty = DisplayStart::compute(None, at(T3), &none()).unwrap();
    assert_eq!((empty.t0, empty.starts_at, empty.ready), (None, None, false));
}

/// 원본은 `kairos doctor`를 띄운다. 그 명령의 몸통은 `diagnose`다.
#[test]
fn doctor는_수집_전의_notion을_기록_없음으로_드러낸다() {
    let (_dir, home) = temp_home();
    let notion = direct_with(SourceName::Notion, vec![QuerySpec::new("mail", 900, 0)], |_| Vec::new());
    let text = diagnose(&home, T1, vec![notion]).lines.join("\n");
    assert!(text.contains("Notion mail: 수집 기록 없음"), "{text}");
    assert!(text.contains("권한 700"));
    assert!(text.contains("스키마 v8, wal, 권한 600"));
    assert!(text.contains("근거 없는 종료 0건"));
}

#[test]
fn doctor_notion_미연결_신원_로그인_만료_d_7_권한_문제를_드러낸다() {
    let (_dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    ingest(&db, SourceResult { identity: Some("kairos-dev".into()), ..result() }, T0);
    drop(db);
    let healthy = diagnose_login(&home, T1, synthetic_sources(), "2026-04-30T00:00:00.000Z");
    let text = healthy.lines.join("\n");
    assert!(text.contains("Notion mail: 미연결 (not_connected)"), "{text}");
    let github = healthy.lines.iter().find(|line| line.contains("GitHub tick:")).unwrap();
    assert!(github.contains("마지막 ok 2026-03-02T01:00:00.000Z 신원 일치(kairos-dev)"), "{github}");
    assert!(text.contains("로그인 만료 D-"));
    assert!(text.contains("권한: 폴더 700, 파일 600 확인"), "{text}");

    let soon = diagnose_login(&home, T1, synthetic_sources(), "2026-03-08T00:00:00.000Z");
    assert_eq!(soon.exit_code, 1);
    assert!(soon.problems.iter().any(|problem| problem.starts_with("로그인 만료 D-")));

    fs::write(home.state_path(), "{}").unwrap();
    fs::set_permissions(home.state_path(), fs::Permissions::from_mode(0o644)).unwrap();
    let loose = diagnose(&home, T1, synthetic_sources());
    assert_eq!(loose.exit_code, 1);
    assert!(loose.lines.join("\n").contains("권한 644 (기대 600)"));
}

#[test]
fn doctor_덮지_못한_구간을_드러내고_기한이_지나면_문제에서_내린다() {
    let (_dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    let quarantined = SourceResult {
        error_code: Some("record_quarantined".into()),
        window_from: Some(T0.into()),
        window_to: Some(T1.into()),
        ..run(SourceName::Slack, "from_me")
    };
    ingest(&db, quarantined, T0);
    let capped = SourceResult {
        error_code: Some("window_capped".into()),
        window_from: Some(BEFORE.into()),
        window_to: Some(T1.into()),
        ..run(SourceName::Gmail, "recent")
    };
    ingest(&db, capped, T0);
    drop(db);
    let report = diagnose(&home, T1, gmail_sources());
    let has = |prefix: &str, end: &str| report.lines.iter().any(|line| line.starts_with(prefix) && line.ends_with(end));
    assert!(has("알림: Slack from_me: ", "메시지 격리, 원본 앱에서 확인 필요"), "{:?}", report.lines);
    assert!(has("알림: Gmail recent: ", "이전을 읽지 못함"), "{:?}", report.lines);
    assert_eq!(report.exit_code, 1);
    assert!(report.problems.iter().any(|problem| problem.starts_with("덮지 못한 구간 Slack")));

    let later = diagnose(&home, &days_after(T1, 8), gmail_sources());
    assert!(!later.problems.iter().any(|problem| problem.starts_with("덮지 못한 구간")));
}

#[test]
fn doctor_신원_단언이_없는_소스를_표시하고_오래_없으면_문제로_올린다() {
    let (_dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    ingest(&db, run(SourceName::Gmail, "recent"), T0);
    ingest(&db, run(SourceName::Slack, "from_me"), T0);
    let recent = days_after(T0, 4);
    let stamped = |identity: Option<&str>| SourceResult {
        identity: identity.map(str::to_owned),
        started_at: recent.clone(),
        finished_at: recent.clone(),
        watermark: Some(recent.clone()),
        ..run(SourceName::Slack, "from_me")
    };
    ingest(&db, stamped(None), &recent);
    drop(db);
    let report = diagnose(&home, &recent, gmail_sources());
    let row = report.lines.iter().find(|line| line.contains("Gmail recent:")).unwrap();
    assert!(row.contains("신원 단언 없음"), "{row}");
    assert!(report.problems.iter().any(|problem| problem == "Slack 신원 단언 3일 넘게 없음"));

    ingest(&Database::open(&home).unwrap(), stamped(Some("U0SYNTH0001")), &recent);
    let fresh = diagnose(&home, &recent, gmail_sources());
    assert!(!fresh.problems.iter().any(|problem| problem.starts_with("Slack 신원")));
}

#[test]
fn doctor_신원이_다른_응답은_contract_break로_기록되어_문제로_드러난다() {
    let (_dir, home) = temp_home();
    ingest(&Database::open(&home).unwrap(), SourceResult { identity: Some("someone-else".into()), ..result() }, T0);
    let report = diagnose(&home, T1, synthetic_sources());
    assert!(report.problems.iter().any(|problem| problem == "GitHub 마지막 수집 contract_break"));
    assert!(report.lines.join("\n").contains("마지막 결과 contract_break"));
}

fn imminent(when: &str) -> SourceResult {
    let object = meeting_at("imminent", "2026-03-02T10:30:00+09:00", "2026-03-02T11:00:00+09:00");
    SourceResult {
        source: SourceName::Calendar,
        started_at: when.into(),
        finished_at: when.into(),
        watermark: Some(when.into()),
        objects: vec![object],
        ..result()
    }
}

#[test]
fn doctor_스키마_게이트_항목_건수_미전달_p0를_드러낸다() {
    let (_dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    prime(&db, SourceName::Calendar, "tick");
    ingest(&db, imminent(T1), T1);
    refresh(&db, T1);
    drop(db);
    ControlFile::new(&home).write(&control_on()).unwrap();
    let report = diagnose(&home, T2, Vec::new());
    let text = report.lines.join("\n");
    assert!(text.contains("DB 스키마 v8, wal, 권한 600"), "{text}");
    assert!(text.contains("항목: 열림 P0 1건, P1 0건, P2 0건"));
    assert!(text.contains("표시: 켜짐(사용자 지정)"));
    assert!(report.problems.iter().any(|line| line.contains("P0 알림 미전달 1건")));
    assert!(!text.contains("합성 회의"));
}

#[test]
fn doctor_게이트가_닫혀_있으면_사유를_보여_주고_문제로_올리지_않는다() {
    let (_dir, home) = temp_home();
    drop(Database::open(&home).unwrap());
    let report = diagnose(&home, &kst(MON, "11:00"), Vec::new());
    assert!(report.lines.join("\n").contains("표시: 꺼짐("));
    assert!(!report.problems.iter().any(|line| line.contains("표시")));
}

#[test]
fn r12_건너뛴_이벤트는_engine_json과_doctor에_코드로_남고_수집은_끝까지_간다() {
    let (_dir, home) = temp_home();
    let report = tick(&home, T1, Vec::new())
        .sleep(frozen(0))
        .items(|| Ok(EngineReport { skipped: 2, ..EngineReport::default() }))
        .run();
    assert_eq!(report.status, CollectStatus::Ran);
    let status = StateBuilder::read_engine_status(&home).unwrap();
    assert_eq!((status.ok, status.error_code.as_deref()), (false, Some("event_skipped")));
    let doctor = diagnose(&home, T2, Vec::new());
    assert!(doctor.problems.iter().any(|problem| problem.contains("event_skipped")));
}

/// 원본은 CLI를 띄운다. 그 명령의 몸통은 `diagnose`다.
#[test]
fn r28_doctor가_세션_잠금_상태를_보여_준다() {
    let (_dir, home) = temp_home();
    drop(Database::open(&home).unwrap());
    let locked = diagnose(&home, T1, Vec::new());
    assert_eq!(locked.lines.last().unwrap(), "세션: 잠김. 차단 시험 뒤 kairos sessions on");
    ControlFile::new(&home).write(&control_on().with_sessions(Some(T0))).unwrap();
    // 켠 때의 설정 지문이 없으면 설정이 바뀐 것과 같다.
    let unapproved = diagnose(&home, T1, Vec::new());
    assert_eq!(unapproved.lines.last().unwrap(), "세션: 잠김. 설정이 바뀌어 차단 시험을 다시 본 뒤 kairos sessions on");
    kairos_agenda::SessionApproval::new(&home).record().unwrap();
    let opened = diagnose(&home, T1, Vec::new());
    assert_eq!(opened.lines.last().unwrap(), &format!("세션: 열림({T0}에 켬)"));
    std::fs::write(home.file(kairos_agenda::SESSION_APPROVAL_FILE), format!("{}\n", "0".repeat(64))).unwrap();
    let changed = diagnose(&home, T1, Vec::new());
    assert!(changed.lines.last().unwrap().starts_with("세션: 잠김. 설정이 바뀌어"));
}

#[test]
fn 수집_중단은_문제로_올리고_모르는_값은_알_수_없음으로_적는다() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    let flag = HaltFlag::new(&home, Arc::new(FixedClock(at(T0))));
    let halt = Halt { at: Some(T0.into()), reason: "model_called".into(), num_turns: Some(1.0), cost_usd: Some(0.36) };
    flag.write(&halt).unwrap();
    let report = diagnose(&home, T1, Vec::new());
    let line = format!("수집 중단: model_called ({T0}, num_turns 1, 비용 0.36). 원인 확인 뒤 kairos collect --resume");
    assert!(report.lines.contains(&line), "{:?}", report.lines);
    assert!(report.problems.iter().any(|problem| problem == "수집 중단"));
    fs::write(home.file("halt.json"), r#"{"reason":"합성"}"#).unwrap();
    let bare = diagnose(&home, T1, Vec::new());
    let line =
        "수집 중단: 합성 (undefined, num_turns 알 수 없음, 비용 알 수 없음). 원인 확인 뒤 kairos collect --resume";
    assert!(bare.lines.iter().any(|row| row == line), "{:?}", bare.lines);
    assert!(flag.clear().unwrap());
    assert!(!flag.clear().unwrap());
}

fn doctor_with(home: &kairos_core::KairosHome, env: BTreeMap<String, String>, answer: &'static str) -> Doctor {
    let config = DoctorConfig { home: home.clone(), user_home: home.file("user"), env };
    let claude = runner(move |_| ProcessOutput::exited(0, answer));
    Doctor::new(config, Arc::new(FixedClock(at(T1))), claude, Vec::new())
}

#[test]
fn 로그인은_저장된_값을_먼저_읽고_없으면_claude에_묻는다() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    let env = BTreeMap::from([("KAIROS_CLAUDE".to_owned(), "/x/claude".to_owned())]);
    let line = |report: &kairos_tick::DoctorReport| {
        report.lines.iter().find(|line| line.starts_with("로그인")).unwrap().clone()
    };
    let asked = doctor_with(&home, env.clone(), r#"{"loginExpiresAt":"2026-04-30T00:00:00.000Z"}"#).diagnose().unwrap();
    assert_eq!(line(&asked), "로그인 만료 D-59 (2026-04-30T00:00:00.000Z)");
    fs::write(home.file("auth.json"), r#"{"checkedAt":"x","loginExpiresAt":null}"#).unwrap();
    let stored =
        doctor_with(&home, env.clone(), r#"{"loginExpiresAt":"2026-04-30T00:00:00.000Z"}"#).diagnose().unwrap();
    assert_eq!(line(&stored), "로그인 만료 시각 모름 (마지막 확인 실패)");
    fs::write(home.file("auth.json"), r#"{"loginExpiresAt":"2026-03-01T00:00:00.000Z"}"#).unwrap();
    let expired = doctor_with(&home, env.clone(), "{}").diagnose().unwrap();
    assert_eq!(line(&expired), "로그인 만료됨(2026-03-01T00:00:00.000Z). claude.ai 재로그인이 필요합니다");
    assert!(expired.problems.iter().any(|problem| problem == "로그인 만료"));
    fs::write(home.file("auth.json"), r#"{"loginExpiresAt":"합성"}"#).unwrap();
    assert_eq!(line(&doctor_with(&home, env, "{}").diagnose().unwrap()), "로그인 만료 시각 모름 (합성)");
}

#[test]
fn 소스_파일_오류와_launch_agent_수를_드러낸다() {
    let (_dir, home) = temp_home();
    let agents = home.file("Library/LaunchAgents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(agents.join("dev.kairos.collect.plist"), "").unwrap();
    let env = BTreeMap::from([("HOME".to_owned(), home.root().to_string_lossy().into_owned())]);
    let failures = vec![SourceFailure { file: "slack.ts".into(), error_code: "source_load_failed".into() }];
    let report = doctor_with(&home, env, "{}").with_failures(failures).diagnose().unwrap();
    assert!(report.lines.iter().any(|line| line == "  소스 파일 오류 slack.ts: source_load_failed"));
    assert!(report.problems.iter().any(|problem| problem == "소스 파일 오류 slack.ts"));
    assert!(report.lines.iter().any(|line| line == "LaunchAgent: 1개 설치됨"));
}

#[test]
fn 권한_문제는_폴더와_파일을_이름순으로_본다() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    assert!(permission_problems(&home).unwrap().is_empty());
    let logs = home.file("logs");
    fs::create_dir(&logs).unwrap();
    fs::set_permissions(&logs, fs::Permissions::from_mode(0o700)).unwrap();
    for name in ["b.log", "a.log"] {
        fs::write(logs.join(name), "").unwrap();
        fs::set_permissions(logs.join(name), fs::Permissions::from_mode(0o644)).unwrap();
    }
    let problems = permission_problems(&home).unwrap();
    assert_eq!(problems.len(), 2);
    assert!(problems[0].ends_with("a.log") && problems[1].ends_with("b.log"), "{problems:?}");
}
