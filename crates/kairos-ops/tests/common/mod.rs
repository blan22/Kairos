//! 모든 고정값은 합성 데이터다
#![allow(dead_code, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use kairos_core::model::{
    ActorKind, CollectContext, EventType, McpResponse, NormalizedEvent, NormalizedObject, ObjectKind, Outcome, Source,
    SourceName, SourceResult,
};
use kairos_core::ports::{CancelToken, Clock, FixedClock};
use kairos_core::{Instant, KairosHome, js};
use kairos_ledger::{IngestOptions, IngestReport, Ledger};
use kairos_store::Database;
use serde_json::{Value, json};
use tempfile::TempDir;

pub const T0: &str = "2026-03-02T01:00:00.000Z"; // 월요일 10:00 KST
pub const T1: &str = "2026-03-02T01:03:00.000Z";
pub const T2: &str = "2026-03-02T01:06:00.000Z";
pub const T3: &str = "2026-03-02T01:09:00.000Z";
pub const BEFORE: &str = "2026-03-01T01:00:00.000Z";
pub const DAY_MS: i64 = 86_400_000;
pub const MINUTE_MS: i64 = 60_000;

const ME: &str = "U0SYNTH0001";

pub fn at(text: &str) -> Instant {
    Instant::parse(text).unwrap()
}

pub fn days_after(when: &str, days: i64) -> Instant {
    at(when).plus_ms(days * DAY_MS)
}

/// 임시 데이터 폴더. 값이 사라지면 폴더도 지워진다.
pub struct TestHome {
    dir: TempDir,
    pub home: KairosHome,
}

impl TestHome {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = KairosHome::at(dir.path());
        Self { dir, home }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn join(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub fn open_db(&self) -> Database {
        Database::open(&self.home).unwrap()
    }
}

pub fn ingest(db: &Database, result: SourceResult, now: &str) -> IngestReport {
    Ledger::new(db).ingest(result, &IngestOptions::at(at(now))).unwrap()
}

pub fn result() -> SourceResult {
    SourceResult {
        source: SourceName::Github,
        query: "tick".into(),
        outcome: Outcome::Ok,
        started_at: T0.into(),
        finished_at: T0.into(),
        pages: 1,
        expected_count: None,
        parsed_count: 0,
        identity: None,
        window_from: None,
        window_to: None,
        watermark: Some(T0.into()),
        progress_at: None,
        thread_reads: Vec::new(),
        overlap_seconds: 300,
        snapshot: false,
        error_code: None,
        objects: Vec::new(),
        events: Vec::new(),
    }
}

/// 그 시각의 실행 결과. 시작, 끝, watermark가 모두 그 시각이다.
pub fn result_at(when: &str) -> SourceResult {
    SourceResult { started_at: when.into(), finished_at: when.into(), watermark: Some(when.into()), ..result() }
}

pub fn issue(number: i64) -> NormalizedObject {
    NormalizedObject {
        text_head: Some(format!("합성 이슈 {number}")),
        state: Some("open".into()),
        assigned_to_me: Some(true),
        authored_by_me: Some(false),
        ..NormalizedObject::new(
            format!("gh:acme/widget#{number}"),
            SourceName::Github,
            ObjectKind::Issue,
            number.to_string(),
        )
    }
}

pub fn comment(id: &str, occurred_at: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("synthetic-teammate".into()),
        object_key: Some("gh:acme/widget#1".into()),
        ..NormalizedEvent::new(
            format!("gh:comment:{id}"),
            SourceName::Github,
            EventType::GhComment,
            occurred_at,
            ActorKind::Human,
        )
    }
}

pub fn meeting(id: &str) -> NormalizedObject {
    NormalizedObject {
        text_head: Some(format!("합성 회의 {id}")),
        all_day: Some(false),
        start_at: Some("2026-03-03T10:00:00+09:00".into()),
        end_at: Some("2026-03-03T11:00:00+09:00".into()),
        state: Some("confirmed".into()),
        my_response: Some("accepted".into()),
        busy: Some(true),
        ..NormalizedObject::new(format!("gcal:{id}"), SourceName::Calendar, ObjectKind::Meeting, id)
    }
}

pub fn count(db: &Database, table: &str) -> i64 {
    db.connection().query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0)).unwrap()
}

/// 고정값은 합성 데이터이고 신원은 자리표시자다.
pub fn fixture(path: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(path);
    std::fs::read_to_string(file).unwrap().replace("__ME__", ME)
}

pub fn clock() -> Arc<dyn Clock> {
    Arc::new(FixedClock(at(T3)))
}

pub fn json_response(id: &str, body: &Value) -> McpResponse {
    McpResponse { id: id.to_owned(), is_error: false, content: json!([{ "type": "text", "text": body.to_string() }]) }
}

pub fn text_response(id: &str, text: &str) -> McpResponse {
    McpResponse { id: id.to_owned(), is_error: false, content: json!([{ "type": "text", "text": text }]) }
}

pub fn context_of(source: &dyn Source, now: &str) -> CollectContext {
    let cursors = source.queries().iter().map(|query| (query.id.clone(), None)).collect::<BTreeMap<_, _>>();
    CollectContext { now: at(now), cursors, thread_reads: None, cancel: CancelToken::new() }
}

const WATCHDOG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ops/kairos-watchdog.zsh");

/// 실제 감시자를 임시 홈으로 돌린다. `env`가 비면 알림을 표준 출력으로 돌린다.
pub fn run_watchdog(home: &Path, now_ms: i64, env: &[(&str, &str)]) -> String {
    let dry = [("KAIROS_WATCHDOG_DRY", "1")];
    let extra = if env.is_empty() { &dry[..] } else { env };
    let output = Command::new("/bin/zsh")
        .args(["-f", WATCHDOG])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("KAIROS_HOME", home)
        .env("KAIROS_WATCHDOG_NOW", now_ms.div_euclid(1000).to_string())
        .envs(extra.iter().copied())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    js::trim(&String::from_utf8_lossy(&output.stdout)).to_owned()
}

pub fn watchdog(home: &Path, now_ms: i64) -> String {
    run_watchdog(home, now_ms, &[])
}

/// 같은 판정의 Rust 구현(`kairos watchdog`). zsh 판처럼 dry로 돌리고 찍은 줄을 돌려준다.
pub fn rust_watchdog(home: &Path, now_ms: i64) -> String {
    let home = KairosHome::at(home);
    let watchdog = kairos_ops::Watchdog {
        home: &home,
        now: now_ms.div_euclid(1000),
        zone: kairos_core::profile::current().schedule.zone,
        dry: true,
    };
    watchdog.run(&|_: &str| panic!("dry는 알리지 않는다")).unwrap().join("\n")
}
