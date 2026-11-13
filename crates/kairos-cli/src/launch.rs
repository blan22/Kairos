//! `kairos-launch`. Windows 작업 스케줄러가 부르는 창 없는 실행기다.
//!
//! 작업 스케줄러는 작업마다 환경 값을 줄 수 없고, 콘솔 프로그램을 부르면 창이 잠깐 뜬다.
//! 실행기는 GUI 하위 시스템으로 빌드해 창을 만들지 않는다. 설치가 쓴 명세(`<label>.json`)를 읽고
//! 옆의 `kairos.exe`를 창 없이 부른 뒤 출력을 로그에 붙인다. 종료 코드는 그대로 넘긴다.
//!
//! 명세는 좁게 읽는다. 인자는 `collect`나 `watchdog` 하나, 환경 키는 정해 둔 것만, 로그는 절대 경로다.
//! 띄우는 것은 늘 실행기 옆의 kairos다. 다만 명세의 `PATH`, `KAIROS_CLAUDE`, `KAIROS_GH`는 수집이 부를 프로그램을 정한다.
//! 그래서 명세는 본인 전용 자리(`%LOCALAPPDATA%`) 아래에 있을 때만 읽는다. kairos도 같은 자리의 데이터 폴더만 받는다.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kairos_core::platform::{self, CreateMode, Os};
use kairos_core::process::{ProcessEnd, ProcessRequest, ProcessRunner};
use serde_json::Value;

/// 명세가 줄 수 있는 환경 키. 설치기의 `COLLECT_ENV`와 같다.
const ALLOWED_ENV: [&str; 6] = ["PATH", "LANG", "KAIROS_HOME", "KAIROS_CLAUDE", "KAIROS_GH", "KAIROS_PLUGIN_DIR"];
/// 작업의 실행 제한(`PT15M`)보다 조금 짧다. 넘기면 실행기가 먼저 끝내고 로그를 남긴다.
const COLLECT_TIMEOUT: Duration = Duration::from_secs(14 * 60);
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(2 * 60);
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
const MAX_SPEC: usize = 64 * 1024;
/// 명세가 틀렸을 때의 종료 코드.
const BAD_SPEC: i32 = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchSpec {
    pub command: &'static str,
    pub env: BTreeMap<String, String>,
    pub log: PathBuf,
}

fn plain(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

/// 명세를 읽는다. 모르는 키나 값이 있으면 거절한다.
pub fn parse_spec(text: &str, os: Os) -> Result<LaunchSpec, String> {
    if text.len() > MAX_SPEC {
        return Err("명세가 너무 큽니다".to_owned());
    }
    let value: Value = serde_json::from_str(text).map_err(|_| "명세가 JSON이 아닙니다".to_owned())?;
    let object = value.as_object().ok_or("명세가 객체가 아닙니다")?;
    if let Some(key) = object.keys().find(|key| !matches!(key.as_str(), "schema" | "args" | "env" | "log")) {
        return Err(format!("모르는 키: {key}"));
    }
    if object.get("schema").and_then(Value::as_i64) != Some(1) {
        return Err("schema는 1이어야 합니다".to_owned());
    }
    let args: Vec<&str> = object
        .get("args")
        .and_then(Value::as_array)
        .ok_or("args가 없습니다")?
        .iter()
        .map(|arg| arg.as_str().ok_or("args는 문자열입니다"))
        .collect::<Result<_, _>>()?;
    let command = match args.as_slice() {
        ["collect"] => "collect",
        ["watchdog"] => "watchdog",
        _ => return Err("args는 collect나 watchdog 하나입니다".to_owned()),
    };
    let mut env = BTreeMap::new();
    for (key, value) in object.get("env").and_then(Value::as_object).ok_or("env가 없습니다")? {
        if !ALLOWED_ENV.contains(&key.as_str()) {
            return Err(format!("허용하지 않는 환경 키: {key}"));
        }
        let text = value.as_str().filter(|text| plain(text)).ok_or_else(|| format!("{key} 값이 틀렸습니다"))?;
        env.insert(key.clone(), text.to_owned());
    }
    let log = object.get("log").and_then(Value::as_str).ok_or("log가 없습니다")?;
    if !plain(log) || !platform::is_absolute(os, log) {
        return Err("log는 절대 경로입니다".to_owned());
    }
    Ok(LaunchSpec { command, env, log: PathBuf::from(log) })
}

/// 실행기 옆의 kairos. 명세에서 읽지 않는다.
pub fn sibling_kairos(launcher: &Path, os: Os) -> Option<PathBuf> {
    Some(launcher.parent()?.join(os.exe("kairos")))
}

/// 한 번 부른다. 출력은 로그에 붙인다. 로그를 열지 못해도 종료 코드는 넘긴다.
pub fn launch(runner: &dyn ProcessRunner, kairos: &Path, spec: &LaunchSpec, base_env: BTreeMap<String, String>) -> i32 {
    // Windows는 SystemRoot, USERPROFILE, LOCALAPPDATA 같은 값이 있어야 프로그램이 뜬다. 사용자 환경 위에 명세의 값을 얹는다.
    let mut env = base_env;
    env.extend(spec.env.clone());
    let timeout = if spec.command == "collect" { COLLECT_TIMEOUT } else { WATCHDOG_TIMEOUT };
    let request = ProcessRequest::new(kairos.to_string_lossy().into_owned(), [spec.command], timeout)
        .env(env)
        .max_output(MAX_OUTPUT);
    let output = runner.run(&request);
    let code = match output.end {
        ProcessEnd::Exited(code) => code,
        _ => 1,
    };
    let mut text = output.stdout;
    text.push_str(&output.stderr);
    if !matches!(output.end, ProcessEnd::Exited(_)) {
        text.push_str(&format!("kairos-launch: {} {:?}\n", spec.command, output.end));
    }
    append(&spec.log, &text);
    code
}

fn append(log: &Path, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).create_mode(0o600).open(log) {
        let _ = file.write_all(text.as_bytes());
    }
}

/// `kairos-launch <명세 파일>`.
pub fn launch_main(args: &[String], runner: &dyn ProcessRunner, base_env: BTreeMap<String, String>) -> i32 {
    let os = Os::current();
    let [spec_path] = args else {
        return BAD_SPEC;
    };
    // 다른 사용자가 바꿀 수 있는 자리의 명세는 읽지 않는다. 로그 자리도 모르므로 남기지 않는다.
    if os.private_home_problem(Path::new(spec_path), &kairos_core::home::user_home()).is_some() {
        return BAD_SPEC;
    }
    let Ok(text) = std::fs::read_to_string(spec_path) else {
        return BAD_SPEC;
    };
    let spec = match parse_spec(&text, os) {
        Ok(spec) => spec,
        Err(reason) => {
            // 로그 자리를 모르므로 명세 옆에 남긴다.
            append(&Path::new(spec_path).with_extension("error.log"), &format!("kairos-launch: {reason}\n"));
            return BAD_SPEC;
        }
    };
    let Some(kairos) = std::env::current_exe().ok().and_then(|exe| sibling_kairos(&exe, os)) else {
        return BAD_SPEC;
    };
    launch(runner, &kairos, &spec, base_env)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kairos_core::process::ProcessOutput;

    use super::*;

    const SPEC: &str = r#"{"schema":1,"args":["collect"],"env":{"PATH":"C:\\Windows\\System32","KAIROS_HOME":"C:\\Users\\synth\\AppData\\Local\\kairos"},"log":"C:\\Users\\synth\\AppData\\Local\\kairos\\logs\\collect.log"}"#;

    #[test]
    fn 명세는_정해_둔_인자와_환경_키만_받는다() {
        let spec = parse_spec(SPEC, Os::Windows).unwrap();
        assert_eq!(spec.command, "collect");
        assert_eq!(spec.env["KAIROS_HOME"], r"C:\Users\synth\AppData\Local\kairos");
        for bad in [
            r#"{"schema":1,"args":["sessions","on"],"env":{},"log":"C:\\l.log"}"#,
            r#"{"schema":1,"args":["collect"],"env":{"KAIROS_ORCA":"C:\\x.exe"},"log":"C:\\l.log"}"#,
            r#"{"schema":1,"args":["collect"],"env":{},"log":"l.log"}"#,
            r#"{"schema":2,"args":["collect"],"env":{},"log":"C:\\l.log"}"#,
            r#"{"schema":1,"args":["collect"],"env":{},"log":"C:\\l.log","program":"C:\\x.exe"}"#,
            r#"{"schema":1,"args":["collect"],"env":{"PATH":"a\nb"},"log":"C:\\l.log"}"#,
        ] {
            assert!(parse_spec(bad, Os::Windows).is_err(), "{bad}");
        }
    }

    #[test]
    fn 실행기는_옆의_kairos만_부른다() {
        assert_eq!(
            sibling_kairos(Path::new("/opt/kairos/bin/kairos-launch"), Os::Linux),
            Some(PathBuf::from("/opt/kairos/bin/kairos"))
        );
    }

    struct Recorder(Mutex<Vec<ProcessRequest>>);

    impl ProcessRunner for Recorder {
        fn run(&self, request: &ProcessRequest) -> ProcessOutput {
            self.0.lock().unwrap().push(request.clone());
            ProcessOutput { stderr: "warn\n".into(), ..ProcessOutput::exited(3, "out\n") }
        }
    }

    #[test]
    fn 사용자_환경_위에_명세의_값을_얹고_출력을_로그에_붙인다() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs").join("collect.log");
        let mut spec = parse_spec(SPEC, Os::Windows).unwrap();
        spec.log = log.clone();
        let runner = Recorder(Mutex::new(Vec::new()));
        let base = BTreeMap::from([
            ("PATH".to_owned(), "old".to_owned()),
            ("SystemRoot".to_owned(), r"C:\Windows".to_owned()),
        ]);
        assert_eq!(launch(&runner, Path::new("kairos"), &spec, base), 3);
        let seen = runner.0.lock().unwrap();
        assert_eq!(seen[0].args, ["collect"]);
        let env = seen[0].env.as_ref().unwrap();
        assert_eq!((env["PATH"].as_str(), env["SystemRoot"].as_str()), (r"C:\Windows\System32", r"C:\Windows"));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "out\nwarn\n");
    }
}
