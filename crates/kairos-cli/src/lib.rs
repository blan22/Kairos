//! kairos, kairos-run 실행 파일과 조립.
//!
//! - `AppContext` 조립 지점. 시스템 구현은 여기서만 고른다.
//! - `Command`, `CommandRegistry` 명령의 얼굴과 등록부. 새 명령은 구현과 등록 한 줄이다.
//! - `ArgParser` `node:util.parseArgs`의 동작을 옮긴 인자 해석.
//! - `Reply` `--json` 응답 틀과 종료 코드.

mod app;
mod args;
mod command;
mod commands;
mod js_number;
mod reply;
mod session;

use kairos_sessions::SystemSessionSpawner;

pub use app::{AppContext, Console, InstallLayout};
pub use command::{Command, CommandRegistry};
pub use commands::registry;
pub use session::run_session;

/// 프로그램 이름을 뺀 인자. 깨진 바이트는 U+FFFD가 된다. `std::env::args`는 패닉한다.
fn process_args() -> Vec<String> {
    std::env::args_os().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect()
}

/// 설정을 읽어 이 프로세스의 프로필로 정한다. 소스 목록과 시간대가 프로필을 보므로 `AppContext`보다 먼저다.
/// 설정이 깨졌으면 멈춘다. 신뢰 범위를 모른 채 수집하거나 세션을 띄우지 않는다.
fn install_profile() -> Result<(), kairos_config::ConfigError> {
    let home = kairos_core::KairosHome::from_env();
    let loaded = kairos_config::load(home.root())?;
    // 이미 정했으면(라이브러리로 부른 시험) 그대로 둔다.
    let _ = kairos_core::profile::install(loaded.profile);
    Ok(())
}

/// 데이터 폴더가 git 작업 트리 안이면 그 까닭. 수집 기록과 설정이 커밋되어 새지 않게 어느 명령도 돌지 않는다.
fn home_in_worktree() -> Option<String> {
    let home = kairos_core::KairosHome::from_env();
    let worktree = home.enclosing_git_worktree(&kairos_core::home::user_home())?;
    Some(format!(
        "데이터 폴더({})가 git 작업 트리({}) 안에 있습니다. 수집 기록과 설정이 커밋될 수 있어 멈춥니다. KAIROS_HOME을 저장소 밖으로 옮기세요",
        home.root().display(),
        worktree.display()
    ))
}

/// 시작 전에 멈춘다. `--json`이면 계약의 오류 모양으로 낸다.
fn refuse(args: &[String], code: &str, message: &str) -> i32 {
    if args.iter().any(|arg| arg == "--json") {
        let body = serde_json::json!({
            "schema": kairos_agenda::CONTRACT_VERSION,
            "ok": false,
            "error": { "code": code, "message": message },
        });
        println!("{body}");
    } else {
        eprintln!("{message}");
    }
    1
}

/// `kairos`의 몸통. `config`, `connect`, `sources`, `setup`, `help`는 설정이 깨져도 돈다. 깨진 설정을 고치는 길이다.
/// 데이터 폴더가 git 작업 트리 안이면 `help`만 돈다.
pub fn kairos_main() -> i32 {
    let args = process_args();
    let help = matches!(args.first().map(String::as_str), Some("help" | "--help") | None);
    if !help && let Some(message) = home_in_worktree() {
        return refuse(&args, "home_in_git_worktree", &message);
    }
    if let Err(error) = install_profile() {
        let diagnostic = matches!(
            args.first().map(String::as_str),
            Some("config" | "connect" | "sources" | "setup" | "help" | "--help") | None
        );
        if !diagnostic {
            let message = format!("kairos 설정 오류: {error} (kairos config validate로 확인)");
            return refuse(&args, error.code, &message);
        }
    }
    registry().dispatch(&AppContext::system(), &args)
}

/// `kairos-run`의 몸통.
pub fn kairos_run_main() -> i32 {
    if let Some(message) = home_in_worktree() {
        eprintln!("kairos-run: {message}");
        return 1;
    }
    if let Err(error) = install_profile() {
        eprintln!("kairos-run 설정 오류: {error}");
        return 1;
    }
    run_session(&AppContext::system(), &process_args(), &SystemSessionSpawner)
}
