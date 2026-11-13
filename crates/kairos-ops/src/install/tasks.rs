//! Windows 작업 스케줄러(`schtasks.exe`)와 알림 등록(`reg.exe`). 종료 코드만 쓴다. 코드 없이 끝나면 1이다.
//!
//! 알림을 누르면 Windows가 `kairos-reveal:` 링크를 연다. 등록한 명령은 `kairos reveal --link <링크>`이고,
//! reveal은 링크에서 항목 ID만 골라 낸다. 토스트의 보낸 이름은 사용자 범위 AppUserModelId(`dev.kairos.notifier`)다.

use std::sync::Arc;
use std::time::Duration;

use kairos_core::platform::{self, Os};
use kairos_core::process::{ProcessRequest, ProcessRunner};

const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT: usize = 1024 * 1024;
/// 작업 폴더. 작업 스케줄러 라이브러리의 `\kairos\` 아래에 둔다.
const TASK_FOLDER: &str = r"\kairos\";
/// `schtasks /Query`가 그 이름의 작업이 없을 때 내는 코드.
pub(super) const NO_TASK: i32 = 1;

/// 알림을 누르면 여는 링크의 이름. `kairos reveal --link`가 같은 값을 읽는다.
pub const REVEAL_SCHEME: &str = "kairos-reveal";
/// 토스트를 보내는 앱 ID. 알림 헬퍼(`kairos notifier`)가 같은 값을 쓴다.
pub const NOTIFIER_APP_ID: &str = "dev.kairos.notifier";
const CLASSES: &str = r"HKCU\Software\Classes";

pub(super) fn task_name(label: &str) -> String {
    format!("{TASK_FOLDER}{label}")
}

/// `%SystemRoot%\System32` 아래의 도구. 값이 없거나 절대 경로가 아니면 `C:\Windows`로 본다.
/// `%SystemRoot%\System32` 아래의 도구. PATH를 보지 않는다.
pub fn system_tool(name: &str) -> String {
    let root = std::env::var("SystemRoot")
        .ok()
        .filter(|value| platform::is_absolute(Os::Windows, value))
        .unwrap_or_else(|| r"C:\Windows".to_owned());
    format!(r"{}\System32\{name}", root.trim_end_matches('\\'))
}

pub(super) struct Schtasks<'a> {
    runner: &'a Arc<dyn ProcessRunner>,
}

impl<'a> Schtasks<'a> {
    pub const fn new(runner: &'a Arc<dyn ProcessRunner>) -> Self {
        Self { runner }
    }

    fn call(&self, program: &str, args: &[&str]) -> i32 {
        let request = ProcessRequest::new(program, args.iter().copied(), TIMEOUT).max_output(MAX_OUTPUT);
        self.runner.run(&request).exit_code().unwrap_or(1)
    }

    pub fn run(&self, args: &[&str]) -> i32 {
        self.call(&system_tool("schtasks.exe"), args)
    }

    fn reg(&self, args: &[&str]) -> i32 {
        self.call(&system_tool("reg.exe"), args)
    }

    /// 링크 처리기와 토스트 앱 ID를 사용자 범위(HKCU)에 쓴다. 관리자 권한이 필요 없다. 실패한 키의 수를 돌려준다.
    /// `kairos`는 판이 바뀌어도 그대로인 `<데이터 폴더>\current\bin\kairos.exe`다.
    pub fn register_notifier(&self, kairos: &str) -> usize {
        let scheme = format!(r"{CLASSES}\{REVEAL_SCHEME}");
        let command = format!(r"{scheme}\shell\open\command");
        let app = format!(r"{CLASSES}\AppUserModelId\{NOTIFIER_APP_ID}");
        let open = format!("\"{kairos}\" reveal --link \"%1\"");
        let writes: [&[&str]; 5] = [
            &["add", &scheme, "/ve", "/d", "URL:kairos reveal", "/f"],
            &["add", &scheme, "/v", "URL Protocol", "/d", "", "/f"],
            &["add", &command, "/ve", "/d", &open, "/f"],
            &["add", &app, "/v", "DisplayName", "/d", "kairos", "/f"],
            &["add", &app, "/v", "ShowInSettings", "/t", "REG_DWORD", "/d", "1", "/f"],
        ];
        writes.iter().filter(|args| self.reg(args) != 0).count()
    }

    /// 링크 처리기와 앱 ID를 지운다. 없던 키는 실패로 세지 않는다.
    pub fn unregister_notifier(&self) {
        for key in [format!(r"{CLASSES}\{REVEAL_SCHEME}"), format!(r"{CLASSES}\AppUserModelId\{NOTIFIER_APP_ID}")] {
            self.reg(&["delete", &key, "/f"]);
        }
    }
}
