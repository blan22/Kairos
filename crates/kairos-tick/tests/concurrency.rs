//! TS의 Promise가 스레드로 바뀐 자리의 약속. 원본에 없던 시험이다.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant as Monotonic};

use common::fakes::*;
use common::*;
use kairos_core::model::{Source, SourceName, SourceResult};
use kairos_core::ports::{Clock, FixedClock};
use kairos_core::process::{ProcessEnd, ProcessOutput, ProcessRequest, ProcessRunner};
use kairos_source_github::{GithubConfig, GithubSource};
use kairos_tick::CollectStatus;
use serde_json::json;

/// 취소될 때까지 기다리는 gh. 취소되면 실제 러너처럼 Cancelled로 돌아온다.
struct WaitingGh {
    cancelled: Arc<AtomicBool>,
}

impl ProcessRunner for WaitingGh {
    fn run(&self, request: &ProcessRequest) -> ProcessOutput {
        if request.args.first().is_some_and(|arg| arg == "auth") {
            return ProcessOutput::exited(0, "synthetic-token\n");
        }
        loop {
            if request.cancel.as_ref().is_some_and(kairos_core::ports::CancelToken::is_cancelled) {
                self.cancelled.store(true, Ordering::SeqCst);
                return ProcessOutput::ended(ProcessEnd::Cancelled);
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}

/// GitHub 소스는 취소되면 gh_timeout을 낸다. 원본의 withLimit는 그보다 먼저 'timeout'으로 끝나 호출자가 source_timeout을 남긴다.
#[test]
fn github_소스가_취소로_gh_timeout을_내도_기록은_source_timeout이다() {
    let (_dir, home) = temp_home();
    let cancelled = Arc::new(AtomicBool::new(false));
    let gh: Arc<dyn ProcessRunner> = Arc::new(WaitingGh { cancelled: cancelled.clone() });
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(at(T1)));
    let source: Arc<dyn Source> = Arc::new(GithubSource::new(gh, clock, GithubConfig::from_vars(|_| None)));
    let queries = source.queries().len();
    tick(&home, T1, vec![source]).limit(40).run();
    let rows = run_rows(&home);
    assert_eq!(rows.len(), queries);
    assert!(rows.iter().all(|row| row.1 == "timeout" && row.2.as_deref() == Some("source_timeout")), "{rows:?}");
    let deadline = Monotonic::now() + Duration::from_secs(5);
    while !cancelled.load(Ordering::SeqCst) && Monotonic::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(cancelled.load(Ordering::SeqCst), "한도에 걸리면 취소 신호가 소스에 닿는다");
}

#[test]
fn 소스가_신호를_무시해도_약속한_시간에_돌려준다() {
    let (_dir, home) = temp_home();
    let started = Monotonic::now();
    tick(&home, T1, vec![hung(SourceName::Github), hung(SourceName::Slack)]).limit(50).run();
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(run_rows(&home).len(), 2);
}

#[test]
fn 직접_소스와_수집_모드는_동시에_돈다() {
    let (_dir, home) = temp_home();
    let plugin = plugin_dir(&home);
    // 서로 상대가 시작해야 끝난다. 차례로 돌면 둘 다 한도에 걸린다.
    let (source_started, source_seen) = mpsc::channel::<()>();
    let (mode_started, mode_seen) = mpsc::channel::<()>();
    let source_started = Mutex::new(source_started);
    let mode_seen = Mutex::new(mode_seen);
    let source = direct(SourceName::Github, move |_| {
        source_started.lock().unwrap().send(()).unwrap();
        mode_seen.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
        vec![SourceResult { source: SourceName::Github, ..result() }]
    });
    let source_seen = Mutex::new(source_seen);
    let mode_started = Mutex::new(mode_started);
    let mode = runner(move |_| {
        source_seen.lock().unwrap().recv_timeout(Duration::from_secs(5)).unwrap();
        mode_started.lock().unwrap().send(()).unwrap();
        dropped(json!({}))
    });
    let mcp = mcp_fake(SourceName::Slack, tick_query());
    let report = tick(&home, T1, vec![source, mcp]).claude(&plugin, mode).run();
    assert_eq!(report.status, CollectStatus::Ran);
    let github = report.outcomes.iter().find(|row| row.source == SourceName::Github).unwrap();
    assert_eq!(github.outcome.as_str(), "ok");
}

#[test]
fn 소스_순서대로_기록한다() {
    let (_dir, home) = temp_home();
    let slow = direct(SourceName::Github, |_| {
        thread::sleep(Duration::from_millis(30));
        vec![SourceResult { source: SourceName::Github, ..result() }]
    });
    let fast = direct(SourceName::Slack, |_| vec![SourceResult { source: SourceName::Slack, ..result() }]);
    tick(&home, T1, vec![slow, fast]).run();
    let order: Vec<String> = run_rows(&home).into_iter().map(|row| row.0).collect();
    assert_eq!(order, ["github", "slack"]);
}

#[test]
fn 소스는_커서를_미리_읽은_맥락을_받는다() {
    let (_dir, home) = temp_home();
    let first = direct(SourceName::Github, |context| {
        assert!(context.cursor("tick").is_none());
        vec![SourceResult { started_at: T1.into(), finished_at: T1.into(), watermark: Some(T1.into()), ..result() }]
    });
    tick(&home, T1, vec![first]).run();
    let second = direct(SourceName::Github, |context| {
        assert_eq!(context.cursor("tick").unwrap().watermark.as_deref(), Some(T1));
        assert_eq!(context.now, at(T2));
        vec![SourceResult { started_at: T2.into(), finished_at: T2.into(), watermark: Some(T2.into()), ..result() }]
    });
    tick(&home, T2, vec![second]).run();
    assert!(run_rows(&home).iter().all(|row| row.1 == "ok"));
}

/// 벽시계가 한도를 넘기면 monotonic 시계가 멈춰 있어도 끝난다. macOS 잠자기 뒤의 모양이고 심장박동 공백으로 잠자기가 된다.
#[test]
fn 벽시계가_한도를_넘기면_기다림이_끝난다() {
    struct Jumping(Mutex<i64>);
    impl Clock for Jumping {
        fn now(&self) -> kairos_core::Instant {
            let mut now = self.0.lock().unwrap();
            *now += 30_000;
            kairos_core::Instant::from_millis(*now)
        }
    }
    let (_dir, home) = temp_home();
    let config = kairos_tick::TickConfig::new(home.clone(), home.file("user"));
    let clock = Arc::new(Jumping(Mutex::new(at(T1).millis())));
    let started = Monotonic::now();
    kairos_tick::TickRunner::new(config, clock, missing_runner(), None, None, vec![hung(SourceName::Slack)])
        .with_claude_probe(Arc::new(Quiet::default()))
        .run(true)
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
    let rows = run_rows(&home);
    assert_eq!((rows[0].1.as_str(), rows[0].2.as_deref()), ("timeout", Some("sleep_interrupted")));
}
