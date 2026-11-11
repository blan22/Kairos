//! 시험. 틱은 프로세스를 끝내므로 시험 바이너리를 자식으로 다시 띄운다
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant as Monotonic};

use common::fakes::*;
use common::*;
use kairos_core::KairosHome;
use kairos_core::model::{QuerySpec, SourceName};
use kairos_ops::{collect_cwd, collector_project_dir};
use kairos_tick::SleepProbe;
use serde_json::json;

const CHILD_HOME: &str = "KAIROS_SIGTERM_HOME";
const CHILD_MODE: &str = "KAIROS_SIGTERM_MODE";
const SESSION: &str = "4f2a1c3e-0b6d-4e8f-9a7b-1c2d3e4f5a6b";

fn result_file(home: &KairosHome) -> PathBuf {
    let project = collector_project_dir(home, &home.file("user"));
    project.join(SESSION).join("tool-results").join("toolu_plugin_0a1b2c3d.txt")
}

fn wait_for(path: &Path) {
    let deadline = Monotonic::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Monotonic::now() < deadline, "자식이 준비하지 못했다");
        thread::sleep(Duration::from_millis(5));
    }
}

/// 첫 측정은 지나가고, 기록 뒤의 두 번째 측정에서 멈춘다.
struct HangsAfterMode {
    ready: PathBuf,
}

impl SleepProbe for HangsAfterMode {
    fn gap_ms(&self) -> i64 {
        0
    }

    fn count(&self) -> Option<i64> {
        if self.ready.parent().unwrap().join("kairos.db").exists()
            && self.ready.parent().unwrap().join("halt.json").exists()
        {
            fs::write(&self.ready, "").unwrap();
            loop {
                thread::park();
            }
        }
        None
    }
}

/// 자식 역할. 수집 모드가 끝나지 않는 틱을 돌린다.
#[test]
fn 자식_역할_끝나지_않는_틱() {
    let Ok(root) = std::env::var(CHILD_HOME) else {
        return;
    };
    let home = KairosHome::at(root);
    let plugin = plugin_dir(&home);
    let file = result_file(&home);
    let ready = home.file("ready");
    let source = mcp_fake(SourceName::Gmail, vec![QuerySpec::new("recent", 3600, 0)]);
    if std::env::var(CHILD_MODE).as_deref() == Ok("spend") {
        let mode = runner(|_| dropped(json!({ "num_turns": 1, "total_cost_usd": 0.36 })));
        tick(&home, T1, vec![source]).claude(&plugin, mode).sleep(Arc::new(HangsAfterMode { ready })).forced();
        return;
    }
    let mode = runner(move |_| {
        fs::write(&file, "{}").unwrap();
        fs::write(&ready, "").unwrap();
        loop {
            thread::park();
        }
    });
    tick(&home, T1, vec![source]).claude(&plugin, mode).forced();
}

fn spawn_child(home: &KairosHome, mode: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "자식_역할_끝나지_않는_틱", "--nocapture", "--test-threads=1"])
        .env(CHILD_HOME, home.root())
        .env(CHILD_MODE, mode)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// 자식이 끝나지 않으면 시험이 멈추지 않게 죽인다.
fn wait_limited(child: &mut std::process::Child) -> std::process::ExitStatus {
    let deadline = Monotonic::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Monotonic::now() > deadline {
            child.kill().unwrap();
            panic!("자식이 신호로 끝나지 않았다");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn terminate(child: &std::process::Child) {
    let status = Command::new("/bin/kill").args(["-TERM", &child.id().to_string()]).status().unwrap();
    assert!(status.success());
}

fn home_with_results() -> (tempfile::TempDir, KairosHome) {
    let (dir, home) = temp_home();
    home.ensure().unwrap();
    fs::create_dir_all(collect_cwd(&home)).unwrap();
    fs::create_dir_all(result_file(&home).parent().unwrap()).unwrap();
    (dir, home)
}

#[test]
fn sigterm으로_죽어도_파일을_지우고_잠금을_푼다() {
    let (_dir, home) = home_with_results();
    let mut child = spawn_child(&home, "wait");
    wait_for(&home.file("ready"));
    assert!(result_file(&home).exists());
    assert!(home.file("collect.lock").exists());
    terminate(&child);
    let status = wait_limited(&mut child);
    let mut stderr = String::new();
    child.stderr.take().unwrap().read_to_string(&mut stderr).unwrap();
    assert_eq!(status.code(), Some(143), "{stderr}");
    assert!(!result_file(&home).exists());
    assert!(!home.file("collect.lock").exists());
}

/// 기록 뒤의 기다림에서 온 신호도 같게 끝난다. 정지 표식은 이미 남아 있다.
#[test]
fn 정지_표식을_쓴_뒤의_sigterm도_잠금을_풀고_143으로_끝난다() {
    let (_dir, home) = home_with_results();
    let mut child = spawn_child(&home, "spend");
    wait_for(&home.file("ready"));
    terminate(&child);
    let status = wait_limited(&mut child);
    assert_eq!(status.code(), Some(143));
    assert!(home.file("halt.json").exists());
    assert!(!home.file("collect.lock").exists());
}
