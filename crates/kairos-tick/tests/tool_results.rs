//! 정리와 되찾기 자체는 kairos-ops의 시험이 맡는다
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_core::KairosHome;
use kairos_core::model::{
    ActorKind, EventType, McpResponse, NormalizedEvent, Outcome, QuerySpec, Source, SourceName, SourceResult,
};
use kairos_core::ports::{Clock, FixedClock};
use kairos_core::process::ProcessOutput;
use kairos_ops::{ToolResults, ToolResultsSweep, collect_cwd, collector_project_dir};
use kairos_source_mcp::{GmailSource, NotionSource};
use kairos_store::Database;
use kairos_tick::{CollectStatus, Doctor, DoctorConfig, DoctorReport, IngestPayload, ResponseIngestor, page_info_of};
use serde_json::{Value, json};
use tempfile::TempDir;

const SESSION: &str = "4f2a1c3e-0b6d-4e8f-9a7b-1c2d3e4f5a6b";
const FILE: &str = "toolu_plugin_0a1b2c3d.txt";

struct Paths {
    _dir: TempDir,
    home: KairosHome,
    user_home: PathBuf,
    project: PathBuf,
    results: PathBuf,
    file: PathBuf,
}

fn paths() -> Paths {
    let (dir, home) = temp_home();
    home.ensure().unwrap();
    let user_home = home.file("user");
    fs::create_dir_all(collect_cwd(&home)).unwrap();
    let project = collector_project_dir(&home, &user_home);
    let results = project.join(SESSION).join("tool-results");
    fs::create_dir_all(&results).unwrap();
    let file = results.join(FILE);
    Paths { _dir: dir, home, user_home, project, results, file }
}

fn marker(path: &str) -> String {
    format!(
        "<persisted-output>\nOutput too large (60.3KB). Full output saved to: {path}\n\nPreview (first 2KB):\n{{\"threads\":[{{\"id\":\"t\n...\n</persisted-output>"
    )
}

fn persisted(id: &str, path: &str) -> McpResponse {
    McpResponse { id: id.to_owned(), is_error: false, content: json!([{ "type": "text", "text": marker(path) }]) }
}

fn notion_mcp() -> Arc<dyn Source> {
    mcp_fake(SourceName::Notion, vec![QuerySpec::new("mail", 900, 0)])
}

fn gmail_mcp() -> Arc<dyn Source> {
    mcp_fake(SourceName::Gmail, vec![QuerySpec::new("recent", 3600, 0)])
}

fn notion_sources() -> Vec<Arc<dyn Source>> {
    vec![notion_mcp()]
}

fn notion_ok() -> SourceResult {
    SourceResult { source: SourceName::Notion, query: "mail".into(), overlap_seconds: 0, ..result() }
}

fn unknown_events(ids: &[&str]) -> Vec<NormalizedEvent> {
    ids.iter()
        .map(|id| NormalizedEvent {
            object_key: Some(format!("notion-mail:{id}")),
            ..NormalizedEvent::new(
                format!("notion-mail:{id}:0"),
                SourceName::Notion,
                EventType::NotionUnknown,
                T0,
                ActorKind::Human,
            )
        })
        .collect()
}

fn store(home: &KairosHome, run: SourceResult) {
    ingest(&Database::open(home).unwrap(), run, T0);
}

fn doctor(paths: &Paths, now: &str) -> DoctorReport {
    let config = DoctorConfig {
        home: paths.home.clone(),
        user_home: paths.user_home.clone(),
        env: std::collections::BTreeMap::new(),
    };
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(at(now)));
    Doctor::new(config, clock, missing_runner(), notion_sources())
        .with_login_probe(Arc::new(Quiet::default()))
        .diagnose()
        .unwrap()
}

fn collected(paths: &Paths, now: &str, sources: Vec<Arc<dyn Source>>) -> kairos_tick::CollectReport {
    tick(&paths.home, now, sources).user_home(&paths.user_home).run()
}

/// 원본은 `kairos ingest` 프로세스를 띄운다. 그 명령의 몸통인 되찾기와 저장을 같은 순서로 부른다.
#[test]
fn kairos_ingest가_표식을_되찾아_저장하고_파일을_지운다() {
    let paths = paths();
    fs::write(&paths.file, fixture("gmail/threads.json")).unwrap();
    let path = paths.file.to_string_lossy().into_owned();
    let tools = ToolResults::for_collector(&paths.home, &paths.user_home);
    let recovered = tools.recover_responses(vec![persisted("gmail:recent", &path)]).unwrap();
    let db = Database::open(&paths.home).unwrap();
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(at(T3)));
    let source = GmailSource::new(clock.clone());
    let payload = IngestPayload { now: at(T3), source: SourceName::Gmail, responses: recovered.responses };
    let summary = ResponseIngestor::new(&db, clock).ingest_responses(&source, &payload, &none()).unwrap();
    assert_eq!(summary.results[0].outcome, Outcome::Ok);
    assert!(!paths.file.exists());
    assert!(!serde_json::to_string(&summary).unwrap().contains("저장되면 안 되는"));
}

#[test]
fn 정리는_토큰_상한_초과분의_mcp_서버_도구_ms_txt도_지우고_그_밖의_이름은_남긴다() {
    let paths = paths();
    fs::write(&paths.file, "a").unwrap();
    fs::write(paths.results.join("mcp-synth_server-get_thing-1730700000000.txt"), "b").unwrap();
    fs::write(paths.results.join("notes.txt"), "c").unwrap();
    let swept = ToolResults::new(&paths.project).sweep().unwrap();
    assert_eq!(swept, ToolResultsSweep { removed: 2, remaining: 1 });
    assert!(paths.results.join("notes.txt").exists());
}

#[test]
fn collect_끝에서_남은_파일을_지우고_건수를_결과에_남긴다_실패한_틱도_마찬가지다() {
    let paths = paths();
    let plugin = plugin_dir(&paths.home);
    let leaving = paths.file.clone();
    let mode = runner(move |_| {
        fs::write(&leaving, "{\"synthetic\":true}").unwrap();
        ProcessOutput::exited(1, "not json")
    });
    let report = tick(&paths.home, T1, vec![gmail_mcp()]).user_home(&paths.user_home).claude(&plugin, mode).run();
    assert_eq!(report.tool_results, Some(ToolResultsSweep { removed: 1, remaining: 0 }));
    assert!(!paths.file.exists());
    assert!(!paths.project.exists());
}

#[test]
fn 훅이_죽어_남긴_파일은_다음_틱이_지운다_모드를_띄우지_않는_틱도_지운다() {
    let paths = paths();
    fs::write(&paths.file, "{\"synthetic\":true}").unwrap();
    let report = collected(&paths, T1, Vec::new());
    assert_eq!(report.tool_results, Some(ToolResultsSweep { removed: 1, remaining: 0 }));
}

#[test]
fn collect가_예외로_끝나도_정리한다() {
    let paths = paths();
    fs::write(&paths.file, "{\"synthetic\":true}").unwrap();
    let probe = Quiet { fail_fingerprint: true, ..Quiet::default() };
    let outcome = tick(&paths.home, T1, Vec::new()).user_home(&paths.user_home).probe(probe).try_run(false);
    assert!(outcome.is_err());
    assert!(!paths.file.exists());
    assert!(!paths.home.file("collect.lock").exists());
}

#[test]
fn doctor_남은_파일은_problems에_오르고_없으면_0건으로_나온다() {
    let paths = paths();
    let clean = doctor(&paths, T1);
    assert!(clean.lines.iter().any(|line| line == "큰 응답 임시 파일: 0건"));
    assert!(!clean.problems.iter().any(|problem| problem.contains("임시 파일")));

    fs::write(&paths.file, "{\"synthetic\":true}").unwrap();
    let dirty = doctor(&paths, T1);
    assert!(dirty.problems.iter().any(|problem| problem == "큰 응답 임시 파일 1건 남음"));
    assert_eq!(dirty.exit_code, 1);
    assert_eq!(fs::read_to_string(&paths.file).unwrap(), "{\"synthetic\":true}", "doctor는 읽기만 한다");
}

#[test]
fn doctor_notion은_gmail과_같은_신원_공백을_드러내고_미분류_건수를_보인다() {
    let paths = paths();
    store(&paths.home, SourceResult { events: unknown_events(&["t1"]), ..notion_ok() });
    let report = doctor(&paths, T1);
    let text = report.lines.join("\n");
    let row = report.lines.iter().find(|line| line.contains("Notion mail: 정상")).unwrap();
    assert!(row.contains("신원 단언 없음"), "{row}");
    assert!(text.contains("미분류 1건"));
    assert!(text.contains("Notion 미분류 1건"));
    assert!(
        !report.problems.iter().any(|problem| problem.contains("Notion 신원")),
        "단언 수단이 없는 소스를 신원 문제로 올리지 않는다"
    );
}

#[test]
fn doctor와_첫_줄_고지_발신자_불일치는_덮지_못한_구간으로_드러난다() {
    let paths = paths();
    let run = SourceResult {
        error_code: Some("notion_sender_mismatch".into()),
        window_from: Some(T0.into()),
        window_to: Some(T0.into()),
        ..notion_ok()
    };
    store(&paths.home, run);
    let report = doctor(&paths, T1);
    assert!(report.problems.iter().any(|problem| problem.contains("발신자 형식을 읽지 못한")));
}

fn notion_stub() -> Arc<dyn Source> {
    notion_mcp()
}

#[test]
fn watch_tsv에_notion_질의가_주기의_3배로_들어간다() {
    let paths = paths();
    collected(&paths, T1, vec![notion_stub()]);
    let text = fs::read_to_string(paths.home.file("watch.tsv")).unwrap();
    assert!(text.lines().any(|line| line == "source\tnotion\tmail\t2700"), "{text}");
}

/// 원본은 `kairos page-info` 프로세스를 띄운다. 그 명령의 몸통을 같은 순서로 부른다.
#[test]
fn kairos_page_info는_다음_쪽_값만_내고_파일을_지우지_않는다_뒤의_ingest가_쓴다() {
    let paths = paths();
    let body = json!({
        "threads": [{ "id": "t1", "subject": "합성 제목 저장금지" }],
        "nextPageToken": "next-token",
        "pagination_info": "For the next page of results use cursor `C`",
    });
    fs::write(&paths.file, body.to_string()).unwrap();
    let tools = ToolResults::for_collector(&paths.home, &paths.user_home);
    let info = |content: &Value| page_info_of(tools.peek_response(content).unwrap().as_deref());
    let path = paths.file.to_string_lossy().into_owned();
    assert_eq!(
        Value::Object(info(&persisted("x", &path).content)),
        json!({ "nextPageToken": "next-token", "pagination_info": "For the next page of results use cursor `C`" })
    );
    assert!(paths.file.exists());
    assert!(info(&persisted("x", "/etc/hosts").content).is_empty(), "폴더 밖은 읽지 않는다");
    let plain = json!([{ "type": "text", "text": "{\"nextPageToken\":\"plain\"}" }]);
    assert!(info(&plain).is_empty(), "표식이 아니면 아무것도 내지 않는다");
}

#[test]
fn 수집_시간_밖_주말_중단_상태의_틱도_남은_파일을_지운다() {
    let setups: [(&str, &str, bool); 3] = [
        ("야간", "2026-03-02T12:00:00.000Z", false),
        ("토요일", "2026-03-07T02:00:00.000Z", false),
        ("중단", T1, true),
    ];
    for (name, now, halted) in setups {
        let paths = paths();
        if halted {
            fs::write(paths.home.file("halt.json"), r#"{"reason":"model_called"}"#).unwrap();
        }
        fs::write(&paths.file, "{\"synthetic\":true}").unwrap();
        let report = collected(&paths, now, Vec::new());
        assert_ne!(report.status, CollectStatus::Ran, "{name}");
        assert_eq!(report.tool_results, Some(ToolResultsSweep { removed: 1, remaining: 0 }), "{name}");
        assert!(!paths.file.exists(), "{name}");
        assert!(!paths.home.file("collect.lock").exists(), "{name}");
    }
}

#[test]
fn 잠금이_있으면_쉬는_틱도_진행_중인_수집의_파일을_지우지_않는다() {
    let paths = paths();
    fs::write(&paths.file, "{\"synthetic\":true}").unwrap();
    let held = json!({ "pid": std::process::id(), "at": at(T1).millis() }).to_string();
    fs::write(paths.home.file("collect.lock"), held).unwrap();
    let report = collected(&paths, "2026-03-02T12:00:00.000Z", Vec::new());
    assert_eq!((report.status, report.tool_results), (CollectStatus::Skipped, None));
    assert!(paths.file.exists());
}

#[test]
fn 지우지_못한_파일은_watch_tsv의_경보로_나간다() {
    let paths = paths();
    fs::write(paths.results.join("notes.txt"), "keep").unwrap();
    collected(&paths, T1, Vec::new());
    let text = fs::read_to_string(paths.home.file("watch.tsv")).unwrap();
    assert!(text.lines().any(|line| line == "alert\tlarge response files"), "{text}");
}

#[test]
fn 형식을_모르는_블록이_몰리면_doctor_문제와_watch_경보가_되고_한_건은_되지_않는다() {
    let paths = paths();
    store(&paths.home, SourceResult { events: unknown_events(&["t1"]), ..notion_ok() });
    let quiet = doctor(&paths, T1);
    assert!(!quiet.problems.iter().any(|problem| problem.contains("형식 변경")));

    store(&paths.home, SourceResult { events: unknown_events(&["t2", "t3"]), ..notion_ok() });
    let report = doctor(&paths, T1);
    assert!(report.problems.iter().any(|problem| problem.contains("형식 변경 의심 3건")));
    collected(&paths, T1, vec![notion_stub()]);
    let text = fs::read_to_string(paths.home.file("watch.tsv")).unwrap();
    assert!(text.lines().any(|line| line == "alert\tnotion format"), "{text}");

    let later = at(T1).plus_ms(25 * 3_600_000).to_iso();
    let aged = doctor(&paths, &later);
    assert!(!aged.problems.iter().any(|problem| problem.contains("형식 변경")), "오래된 건은 경보로 남지 않는다");
}

#[test]
fn doctor는_partial의_원인_코드와_복구_안내를_보여_준다() {
    let paths = paths();
    let run = SourceResult {
        outcome: Outcome::Partial,
        error_code: Some("large_response_outside".into()),
        watermark: None,
        ..notion_ok()
    };
    store(&paths.home, run);
    let report = doctor(&paths, T1);
    assert!(
        report
            .lines
            .join("\n")
            .contains("마지막 결과 partial (large_response_outside: 큰 응답 경로 불일치. 확인 필요)")
    );
}

#[test]
fn 실제_notion_소스도_watch_tsv에_같은_줄로_나간다() {
    let paths = paths();
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(at(T1)));
    let notion: Arc<dyn Source> = Arc::new(NotionSource::new(clock));
    collected(&paths, T1, vec![notion]);
    let text = fs::read_to_string(paths.home.file("watch.tsv")).unwrap();
    assert!(text.lines().any(|line| line == "source\tnotion\tmail\t2700"), "{text}");
}
