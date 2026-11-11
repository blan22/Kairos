//! 진짜 `SystemProcessRunner`로 도는 시험. gh 자리에 합성 셸 스크립트를 둔다. 실제 gh는 호출하지 않는다.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant as MonotonicInstant};

use kairos_core::model::{DirectSource, Outcome};
use kairos_core::ports::FixedClock;
use kairos_core::process::SystemProcessRunner;
use tempfile::TempDir;

use super::fake::{context, now, query};
use crate::{GithubConfig, GithubSource};

fn source_at(program: &Path) -> GithubSource {
    let program = program.to_string_lossy().into_owned();
    let config = GithubConfig::from_vars(|key| match key {
        "KAIROS_GH" => Some(program.clone()),
        "PATH" => Some("/usr/bin:/bin".to_owned()),
        _ => None,
    });
    GithubSource::new(Arc::new(SystemProcessRunner), Arc::new(FixedClock(now())), config)
}

fn stub(dir: &TempDir, script: &str) -> PathBuf {
    let path = dir.path().join("gh");
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn 합성_gh로_토큰_환경과_stdin이_실제로_넘어간다() {
    let dir = tempfile::tempdir().unwrap();
    let seen = dir.path().display();
    let script = format!(
        r#"case "$1 $2" in
  "auth token") echo synthetic-token ;;
  "api rate_limit") echo '{{"resources":{{"search":{{"remaining":30}}}}}}' ;;
  "api graphql") cat > "{seen}/stdin"; env > "{seen}/env"
    echo '{{"data":{{"viewer":{{"login":"kairos-dev"}},"nodes":[]}}}}' ;;
  *) echo '{{"total_count":0,"incomplete_results":false,"items":[]}}' ;;
esac"#
    );
    let results = source_at(&stub(&dir, &script)).collect(&context(Vec::new()));
    assert!(results.iter().all(|result| result.outcome == Outcome::Ok), "{results:?}");
    let env = fs::read_to_string(dir.path().join("env")).unwrap();
    assert!(env.lines().any(|line| line == "GH_TOKEN=synthetic-token"));
    assert!(env.lines().any(|line| line == "GH_PROMPT_DISABLED=1"));
    // cargo가 시험 프로세스에 넣은 값이다. 부모 환경을 물려받았다면 여기 있다.
    assert!(!env.contains("CARGO_PKG_NAME"));
    let stdin = fs::read_to_string(dir.path().join("stdin")).unwrap();
    assert!(stdin.starts_with(r#"{"query":""#));
    assert!(!stdin.contains("synthetic-token"));
}

#[test]
fn 취소하면_도는_프로세스를_죽이고_gh_timeout으로_끝난다() {
    let dir = tempfile::tempdir().unwrap();
    let program = stub(&dir, "exec /bin/sleep 30");
    let context = context(Vec::new());
    let cancel = context.cancel.clone();
    let started = MonotonicInstant::now();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancel.cancel();
    });
    let results = source_at(&program).collect(&context);
    // 취소가 듣지 않으면 토큰 호출의 시간 제한 8초에 걸린다.
    assert!(started.elapsed() < Duration::from_secs(7));
    for result in &results {
        assert_eq!(result.outcome, Outcome::Timeout);
        assert_eq!(result.error_code.as_deref(), Some("gh_timeout"));
    }
}

#[test]
fn 검색_도중_취소하면_신원을_확인한_뒤의_gh_timeout이다() {
    let dir = tempfile::tempdir().unwrap();
    let marker = dir.path().join("searching");
    let script = format!(
        r#"case "$1 $2" in
  "auth token") echo synthetic-token ;;
  "api rate_limit") echo '{{"resources":{{"search":{{"remaining":30}}}}}}' ;;
  "api graphql") cat > /dev/null; echo '{{"data":{{"viewer":{{"login":"kairos-dev"}},"nodes":[]}}}}' ;;
  *) touch "{}"; exec /bin/sleep 30 ;;
esac"#,
        marker.display()
    );
    let program = stub(&dir, &script);
    let context = context(Vec::new());
    let cancel = context.cancel.clone();
    // 시각이 아니라 표식으로 기다린다. 기계가 바빠도 검색이 시작된 뒤에 취소한다.
    thread::spawn(move || {
        while !marker.exists() {
            thread::sleep(Duration::from_millis(5));
        }
        cancel.cancel();
    });
    let results = source_at(&program).collect(&context);
    assert_eq!(query(&results, "mentions").outcome, Outcome::Timeout);
    assert_eq!(query(&results, "mentions").error_code.as_deref(), Some("gh_timeout"));
    assert_eq!(query(&results, "mentions").identity.as_deref(), Some("kairos-dev"));
}

#[test]
fn gh가_없으면_tool_missing이다() {
    let results = source_at(Path::new("/nonexistent/kairos-gh")).collect(&context(Vec::new()));
    assert!(results.iter().all(|result| result.outcome == Outcome::ToolMissing));
    assert!(results.iter().all(|result| result.error_code.as_deref() == Some("gh_missing")));
}
