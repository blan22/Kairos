//! 잠금 시험은 .rs, 모드 판정은 tests/mode.rs에 있다
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::fakes::*;
use common::*;
use kairos_agenda::StateBuilder;
use kairos_core::Instant;
use kairos_core::model::{QuerySpec, SourceName, SourceResult};
use kairos_core::ports::FixedClock;
use kairos_core::process::ProcessOutput;
use kairos_store::Database;
use kairos_tick::{CollectLock, CollectStatus, Halt, HaltFlag};
use serde_json::{Value, json};

fn ok_result(source: SourceName) -> SourceResult {
    let objects = if source == SourceName::Github { vec![issue(1)] } else { Vec::new() };
    SourceResult { source, parsed_count: 1, objects, ..result() }
}

fn query() -> Vec<QuerySpec> {
    tick_query()
}

fn slack_mcp() -> Arc<dyn kairos_core::model::Source> {
    mcp_fake(SourceName::Slack, query())
}

fn halt_of(home: &kairos_core::KairosHome) -> Option<Halt> {
    HaltFlag::new(home, Arc::new(FixedClock(at(T3)))).read()
}

#[test]
fn 소스_하나가_던지거나_멈춰도_나머지는_저장된다() {
    let (_dir, home) = temp_home();
    let sources = vec![direct(SourceName::Github, |_| vec![ok_result(SourceName::Github)]), hung(SourceName::Slack)];
    let report = tick(&home, T1, sources).limit(50).run();
    assert_eq!(report.status, CollectStatus::Ran);
    let rows = run_rows(&home);
    let of = |name: &str| rows.iter().filter(|row| row.0 == name).cloned().collect::<Vec<_>>();
    assert_eq!(of("github").iter().map(|row| row.1.as_str()).collect::<Vec<_>>(), ["ok"]);
    assert_eq!(of("slack")[0].1, "timeout");
    assert_eq!(of("slack")[0].2.as_deref(), Some("source_timeout"));

    let throwing = vec![direct(SourceName::Slack, |_| panic!("외부 문자열 노출 금지"))];
    tick(&home, T2, throwing).run();
    let last = run_rows(&home).pop().unwrap();
    assert_eq!((last.1.as_str(), last.2.as_deref()), ("parse_error", Some("source_threw")));
}

#[test]
fn 시간_초과한_소스는_watermark를_움직이지_않는다() {
    let (_dir, home) = temp_home();
    tick(&home, T1, vec![hung(SourceName::Github)]).limit(30).run();
    assert_eq!(count(&Database::open(&home).unwrap(), "cursor"), 0);
}

#[test]
fn 수집_중_두_번째_실행은_busy로_돌아가고_잠금은_끝나면_풀린다() {
    let (_dir, home) = temp_home();
    let (release, gate) = mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let (started, entered) = mpsc::channel::<()>();
    let started = Mutex::new(started);
    let slow = direct(SourceName::Github, move |_| {
        started.lock().unwrap().send(()).unwrap();
        gate.lock().unwrap().recv().unwrap();
        vec![ok_result(SourceName::Github)]
    });
    let first = {
        let (home, slow) = (home.clone(), slow.clone());
        thread::spawn(move || tick(&home, T1, vec![slow]).run())
    };
    entered.recv().unwrap();
    let second = tick(&home, T1, vec![slow]).run();
    assert_eq!(second.status, CollectStatus::Busy);
    release.send(()).unwrap();
    assert_eq!(first.join().unwrap().status, CollectStatus::Ran);
    assert!(CollectLock::new(&home, Arc::new(FixedClock(at(T1)))).acquire().unwrap());
}

#[test]
fn 근무_시간_밖과_주말에는_수집하지_않고_force는_건너뛴다() {
    let (_dir, home) = temp_home();
    let sources = || vec![direct(SourceName::Github, |_| vec![ok_result(SourceName::Github)])];
    let night = tick(&home, "2026-03-02T13:00:00.000Z", sources()).run();
    assert_eq!(night.status, CollectStatus::Skipped);
    let weekend = tick(&home, "2026-03-07T01:00:00.000Z", sources()).run();
    assert_eq!(weekend.status, CollectStatus::Skipped);
    assert_eq!(run_rows(&home).len(), 0);
    let forced = tick(&home, "2026-03-07T01:00:00.000Z", sources()).forced();
    assert_eq!(forced.status, CollectStatus::Ran);
}

#[test]
fn 연결_없는_소스는_틱마다_not_connected로_기록되고_state_json이_쓰인다() {
    let (_dir, home) = temp_home();
    let sources = vec![
        direct(SourceName::Github, |_| vec![ok_result(SourceName::Github)]),
        unconnected(SourceName::Notion, vec![QuerySpec::new("mail", 900, 0)]),
    ];
    tick(&home, T1, sources).run();
    assert!(run_rows(&home).iter().any(|row| row.0 == "notion" && row.1 == "not_connected"));
    let state = StateBuilder::read_state(&home).unwrap();
    assert!(!state["display"]["t0"].is_null());
    assert_eq!(fs::metadata(home.state_path()).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn 모델이_불리면_contract_break를_기록하고_다음_틱부터_멈춘다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let first = tick(&home, T1, vec![slack_mcp()])
        .claude(&plugin, drop_runner(json!({ "num_turns": 1, "total_cost_usd": 0.36 })))
        .fingerprint("v1")
        .run();
    assert!(first.halted);
    let rows = run_rows(&home);
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].1.as_str(), rows[0].2.as_deref()), ("contract_break", Some("model_called")));
    assert_eq!(halt_of(&home).unwrap().cost_usd, Some(0.36));
    assert_eq!(
        fs::read_to_string(home.file("halt.json")).unwrap(),
        format!(
            "{{\n  \"at\": \"{T1}\",\n  \"reason\": \"model_called\",\n  \"numTurns\": 1,\n  \"costUsd\": 0.36\n}}\n"
        )
    );

    let called = Arc::new(Mutex::new(false));
    let seen = called.clone();
    let second = tick(&home, T2, vec![slack_mcp()])
        .claude(
            &plugin,
            runner(move |_| {
                *seen.lock().unwrap() = true;
                dropped(json!({}))
            }),
        )
        .fingerprint("v1")
        .run();
    assert_eq!(second.status, CollectStatus::Halted);
    assert!(!*called.lock().unwrap());
    assert_eq!(run_rows(&home).len(), 1);
}

#[test]
fn 결과_json이_정상이어도_소스_기록이_없으면_parse_error() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let report = tick(&home, T1, vec![slack_mcp()]).claude(&plugin, drop_runner(json!({}))).run();
    assert!(!report.halted);
    let rows = run_rows(&home);
    assert_eq!((rows[0].1.as_str(), rows[0].2.as_deref()), ("parse_error", Some("no_result_recorded")));
}

#[test]
fn 버전이_바뀐_첫_틱은_계약_시험이고_기록이_없으면_contract_break() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    let plugin = plugin_dir(&home);
    fs::write(home.file("collector.json"), r#"{"fingerprint":"old"}"#).unwrap();
    let report = tick(&home, T1, vec![slack_mcp()]).claude(&plugin, drop_runner(json!({}))).fingerprint("new").run();
    assert!(report.contract_test);
    assert_eq!(run_rows(&home)[0].1, "contract_break");
    let saved: Value = serde_json::from_str(&fs::read_to_string(home.file("collector.json")).unwrap()).unwrap();
    assert_eq!(saved["fingerprint"], "old");
}

#[test]
fn 모드_플러그인이나_claude가_없으면_tool_missing으로_드러난다() {
    let (_dir, home) = temp_home();
    let none = home.file("none").to_string_lossy().into_owned();
    tick(&home, T1, vec![slack_mcp()]).claude_bin(Some("/x/claude"), Some(&none)).run();
    tick(&home, T2, vec![slack_mcp()]).run();
    let rows = run_rows(&home);
    assert_eq!((rows[0].1.as_str(), rows[0].2.as_deref()), ("tool_missing", Some("mode_plugin_missing")));
    assert_eq!((rows[1].1.as_str(), rows[1].2.as_deref()), ("tool_missing", Some("claude_missing")));
}

#[test]
fn 모드가_기록을_남기면_통과로_보고_지문을_저장한다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let inner = home.clone();
    let mode = runner(move |_| {
        let db = Database::open(&inner).unwrap();
        ingest(&db, SourceResult { source: SourceName::Slack, ..result() }, T1);
        dropped(json!({}))
    });
    let report = tick(&home, T1, vec![slack_mcp()]).claude(&plugin, mode).fingerprint("v2").run();
    assert!(report.contract_test);
    assert!(!home.file("halt.json").exists());
    let text = fs::read_to_string(home.file("collector.json")).unwrap();
    assert_eq!(text, format!("{{\n  \"fingerprint\": \"v2\",\n  \"passedAt\": \"{T1}\"\n}}\n"));
}

#[test]
fn 읽지_못한_정지_표식은_정지로_본다_파일이_없을_때만_정지가_아니다() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    let plugin = plugin_dir(&home);
    assert_eq!(halt_of(&home), None);
    for broken in ["", "{\"깨진", "[]"] {
        fs::write(home.file("halt.json"), broken).unwrap();
        assert_eq!(halt_of(&home).unwrap().reason, "halt_unreadable");
    }
    fs::remove_file(home.file("halt.json")).unwrap();
    fs::create_dir(home.file("halt.json")).unwrap();
    assert_eq!(halt_of(&home).unwrap().reason, "halt_unreadable", "폴더처럼 읽지 못하는 것도 정지다");
    fs::remove_dir(home.file("halt.json")).unwrap();
    fs::write(home.file("halt.json"), "[]").unwrap();

    let called = Arc::new(Mutex::new(false));
    let seen = called.clone();
    let mode = runner(move |_| {
        *seen.lock().unwrap() = true;
        dropped(json!({}))
    });
    let report = tick(&home, T1, vec![slack_mcp()]).claude(&plugin, mode).run();
    assert_eq!(report.status, CollectStatus::Halted);
    assert!(!*called.lock().unwrap());
}

/// 원본은 객체의 getter로 기록을 던지게 했다. 여기서는 표를 막아 ingest의 재시도까지 실패하게 한다.
#[test]
fn 모델이_불린_틱은_기록이_던져도_정지_표식이_먼저_남는다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let inner = home.clone();
    let mode = runner(move |_| {
        let db = Database::open(&inner).unwrap();
        db.connection()
            .execute_batch(
                "CREATE TRIGGER poisoned BEFORE INSERT ON source_run BEGIN SELECT RAISE(ABORT, 'synthetic'); END",
            )
            .unwrap();
        dropped(json!({ "num_turns": 1, "total_cost_usd": 0.36 }))
    });
    let sources = vec![direct(SourceName::Github, |_| vec![ok_result(SourceName::Github)]), slack_mcp()];
    let outcome = tick(&home, T1, sources).claude(&plugin, mode).try_run(false);
    assert!(outcome.is_err());
    assert_eq!(halt_of(&home).unwrap().reason, "model_called");
    assert!(!home.file("collect.lock").exists());
}

#[test]
fn 출력이_json_null이면_다음_틱부터_수집을_멈추고_잠금은_풀린다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    let mode = runner(|_| ProcessOutput::exited(0, "null"));
    let outcome = tick(&home, T1, vec![slack_mcp()]).claude(&plugin, mode).try_run(false);
    assert!(outcome.is_ok());
    assert!(!home.file("collect.lock").exists());
    assert_eq!(halt_of(&home).unwrap().reason, "model_called");
}

/// 원본은 openDb가 try 밖이라 잠금을 남긴다. 프로세스가 끝나면 다음 틱이 죽은 pid로 보고 넘겨받는다.
#[test]
fn db를_열지_못하면_원본처럼_실패로_끝나고_잠금이_남는다() {
    let (_dir, home) = temp_home();
    home.ensure().unwrap();
    fs::create_dir(home.db_path()).unwrap();
    assert!(tick(&home, T1, Vec::new()).try_run(false).is_err());
    assert!(home.file("collect.lock").exists());
}

#[test]
fn 깨어난_시각은_틱의_now에_흐른_시간을_더한_값이다() {
    struct Stepping(Mutex<i64>);
    impl kairos_core::ports::Clock for Stepping {
        fn now(&self) -> Instant {
            let mut at = self.0.lock().unwrap();
            *at += 40_000;
            Instant::from_millis(*at)
        }
    }
    let (_dir, home) = temp_home();
    let clock = Arc::new(Stepping(Mutex::new(at(T1).millis())));
    let config = kairos_tick::TickConfig::new(home.clone(), home.file("user"));
    let tick = kairos_tick::TickRunner::new(config, clock.clone(), missing_runner(), None, None, Vec::new())
        .with_claude_probe(Arc::new(Quiet::default()))
        .with_sleep_probe(frozen(60_000));
    let report = tick.run(true).unwrap();
    assert!(report.slept);
    let first_read = at(T1).plus_ms(40_000);
    let woke = kairos_ops::ResumeFile::new(&home).read().unwrap();
    assert!(woke.ms_since(first_read) >= 40_000, "now는 시계의 첫 읽기이고 그 뒤 흐른 시간을 더한다");
    thread::sleep(Duration::from_millis(1));
}
