//! Mac 알림 권한. 설치본의 알림 헬퍼(KairosNotifier.app)로 권한 상태를 읽고 macOS 권한 창을 띄운다.
//!
//! 다른 Mac 앱과 같다. 묻는 쪽은 macOS이고 kairos는 터미널에서 따로 묻지 않는다. 허용하면 그때부터 알림이 간다.
//! macOS는 한 번 답한 앱에 권한 창을 다시 띄우지 않는다. 거절된 뒤에는 시스템 설정의 알림 화면에서 켠다.
//! `kairos setup`과 `kairos notify setup|test`가 이 길을 쓴다. 백그라운드 틱은 권한 창을 띄우지 않는다.
//! 헬퍼에 넘기는 인자와 시스템 설정 주소는 모두 상수다. 외부 값이 들어가지 않는다.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kairos_core::process::{ProcessOutput, ProcessRequest};

use crate::app::{AppContext, Console};

/// LaunchServices 등록기. 알림을 누르면 macOS가 등록된 번들을 띄운다. 시험은 `KAIROS_LSREGISTER`로 가짜를 준다.
const LSREGISTER_BIN: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
/// 설치본 안의 알림 헬퍼 번들.
const NOTIFIER_APP: [&str; 3] = ["ops", "notifier", "KairosNotifier.app"];
/// 번들 안의 실행 파일.
const NOTIFIER_EXEC: [&str; 3] = ["Contents", "MacOS", "KairosNotifier"];
/// 시스템 설정의 알림 화면. `id`는 헬퍼의 번들 ID(`ops/notifier/Info.plist`)다. macOS가 `id`를 모르면 알림 화면만 연다.
const SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=dev.kairos.notifier";
/// 헬퍼의 `setup`은 권한 창에서 최대 120초 기다린다.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(150);
const SHORT_TIMEOUT: Duration = Duration::from_secs(30);
/// 헬퍼의 `post`는 3.5초 안에 끝난다.
const POST_TIMEOUT: Duration = Duration::from_secs(5);
/// 헬퍼가 권한이 없을 때 내는 종료 코드.
const EXIT_NOT_AUTHORIZED: i32 = 3;
/// 시험 알림. 고정 문구이고 항목이 없어 눌러도 아무 일도 하지 않는다.
const TEST_PAYLOAD: &str = r#"{"title":"kairos","body":"시험 알림입니다. 이 알림이 보이면 Mac 알림이 켜져 있습니다.","items":[],"tier":"P1","thread":"kairos.setup"}"#;

/// 헬퍼가 `status`로 답한 권한 상태.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Permission {
    /// 알림을 보낼 수 있다(`authorized`, `provisional`, `ephemeral`).
    Authorized,
    /// 사용자가 거절했거나 시스템 설정에서 껐다. 권한 창이 다시 뜨지 않는다.
    Denied,
    /// 아직 묻지 않았다. 권한 창을 띄울 수 있다.
    NotDetermined,
    /// 헬퍼가 답하지 않았다.
    Unknown,
}

impl Permission {
    fn parse(text: &str) -> Self {
        match text.trim() {
            "authorized" | "provisional" | "ephemeral" => Self::Authorized,
            "denied" => Self::Denied,
            "notDetermined" => Self::NotDetermined,
            _ => Self::Unknown,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Authorized => "authorized",
            Self::Denied => "denied",
            Self::NotDetermined => "notDetermined",
            Self::Unknown => "unknown",
        }
    }
}

/// 권한을 받으려 한 결과.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Grant {
    /// 이미 있었거나 방금 받았다.
    Authorized,
    /// 전에 거절했다. 권한 창을 띄우지 못했다.
    DeniedBefore,
    /// 방금 권한 창에서 거절했다.
    DeniedNow,
    /// 권한 창에 답이 없었다.
    Unanswered,
}

/// 시험 알림을 보낸 결과.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TestPost {
    Posted,
    NotAuthorized,
    /// 헬퍼의 종료 코드. 신호나 시간 초과면 None이다.
    Failed(Option<i32>),
}

/// 헬퍼를 찾지 못한 까닭.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Missing {
    /// 설치본(`<데이터 폴더>/current`)이 없다.
    Runtime,
    /// 설치본에 헬퍼 번들이 없다.
    Helper,
}

impl Missing {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Runtime => "설치본이 없습니다. scripts/install.sh로 설치한 뒤 다시 실행하세요",
            Self::Helper => "알림 헬퍼가 설치본에 없습니다. scripts/install.sh로 다시 설치하세요",
        }
    }
}

fn join(base: &Path, parts: &[&str]) -> PathBuf {
    parts.iter().fold(base.to_path_buf(), |path, part| path.join(part))
}

fn lsregister_bin(context: &AppContext) -> &str {
    context.env("KAIROS_LSREGISTER").map(str::trim).filter(|value| !value.is_empty()).unwrap_or(LSREGISTER_BIN)
}

/// 번들을 LaunchServices에서 뺀다. 설치본을 지울 때 쓴다. 뺐으면 true다.
pub(crate) fn unregister(context: &AppContext, app: &Path) -> bool {
    let request = ProcessRequest::new(lsregister_bin(context), ["-u", &app.to_string_lossy()], SHORT_TIMEOUT);
    context.runner().run(&request).success()
}

/// 설치본의 알림 헬퍼. 개발 트리의 번들은 권한을 따로 받으므로 쓰지 않는다.
pub(crate) struct NotifierHelper<'a> {
    context: &'a AppContext,
    app: PathBuf,
    bin: PathBuf,
}

impl<'a> NotifierHelper<'a> {
    /// 설치본(`<데이터 폴더>/current`)의 헬퍼.
    pub(crate) fn installed(context: &'a AppContext) -> Result<Self, Missing> {
        let runtime = fs::canonicalize(context.home().root().join("current")).map_err(|_| Missing::Runtime)?;
        Self::in_runtime(context, &runtime).ok_or(Missing::Helper)
    }

    /// `runtime` 안의 헬퍼. 실행 파일이 없으면 None이다. 시험은 `KAIROS_NOTIFIER`로 가짜를 준다.
    pub(crate) fn in_runtime(context: &'a AppContext, runtime: &Path) -> Option<Self> {
        let (app, bin) = match context.env("KAIROS_NOTIFIER").map(str::trim).filter(|value| !value.is_empty()) {
            // 번들은 실행 파일의 세 단계 위다.
            Some(bin) => {
                let bin = PathBuf::from(bin);
                let app = bin.ancestors().nth(3).map_or_else(|| bin.clone(), Path::to_path_buf);
                (app, bin)
            }
            None => {
                let app = join(runtime, &NOTIFIER_APP);
                let bin = join(&app, &NOTIFIER_EXEC);
                (app, bin)
            }
        };
        bin.is_file().then_some(Self { context, app, bin })
    }

    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> ProcessOutput {
        self.context.runner().run(&ProcessRequest::new(program, args.iter().copied(), timeout))
    }

    fn helper(&self, args: &[&str], timeout: Duration) -> ProcessOutput {
        self.run(&self.bin.to_string_lossy(), args, timeout)
    }

    pub(crate) fn permission(&self) -> Permission {
        Permission::parse(&self.helper(&["status"], SHORT_TIMEOUT).stdout)
    }

    /// 권한을 받는다. 터미널에서 묻지 않고 macOS 권한 창을 띄운다. 하는 일을 `console`에 알린다.
    pub(crate) fn grant(&self, console: &Console) -> Grant {
        match self.permission() {
            Permission::Authorized => {
                console.print("Mac 알림: 권한이 이미 있습니다");
                return Grant::Authorized;
            }
            Permission::Denied => {
                console.print("Mac 알림이 꺼져 있습니다. macOS는 한 번 거절한 앱에 권한 창을 다시 띄우지 않습니다");
                console.print("시스템 설정 > 알림 > kairos에서 '알림 허용'을 켜면 그때부터 알림이 갑니다");
                return Grant::DeniedBefore;
            }
            Permission::NotDetermined | Permission::Unknown => {}
        }
        console.print("macOS가 kairos 알림을 허용할지 묻습니다. 화면 오른쪽 위의 권한 창에서 '허용'을 누르세요");
        self.register(console);
        if self.helper(&["setup"], REQUEST_TIMEOUT).success() {
            console.print("Mac 알림 권한을 받았습니다. 확인 알림을 하나 보냈습니다");
            return Grant::Authorized;
        }
        if self.permission() == Permission::Denied {
            console.print("알림을 허용하지 않았습니다. 바꾸려면 시스템 설정 > 알림 > kairos에서 켜세요");
            Grant::DeniedNow
        } else {
            console.print("권한 창에 답이 없어 넘어갑니다. 나중에: kairos notify setup");
            Grant::Unanswered
        }
    }

    /// 번들을 LaunchServices에 등록한다. 실패해도 권한은 받을 수 있다. 알림을 눌렀을 때 패널이 열리지 않을 뿐이다.
    fn register(&self, console: &Console) {
        let lsregister = lsregister_bin(self.context);
        if !self.run(lsregister, &["-f", &self.app.to_string_lossy()], SHORT_TIMEOUT).success() {
            console.print("LaunchServices 등록에 실패했습니다. 알림을 눌러도 패널이 열리지 않을 수 있습니다");
        }
    }

    /// 시스템 설정의 알림 화면을 연다.
    pub(crate) fn open_settings(&self) -> bool {
        self.run(&self.context.open_bin(), &[SETTINGS_URL], SHORT_TIMEOUT).success()
    }

    /// 고정 문구의 시험 알림 하나를 보낸다. 권한을 묻지 않는다.
    pub(crate) fn post_test(&self) -> TestPost {
        let done = self.helper(&["post", TEST_PAYLOAD], POST_TIMEOUT);
        match done.exit_code() {
            Some(0) => TestPost::Posted,
            Some(EXIT_NOT_AUTHORIZED) => TestPost::NotAuthorized,
            code => TestPost::Failed(code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 헬퍼의_상태_낱말을_권한으로_읽는다() {
        for allowed in ["authorized\n", "provisional", " ephemeral "] {
            assert_eq!(Permission::parse(allowed), Permission::Authorized, "{allowed}");
        }
        assert_eq!(Permission::parse("denied\n"), Permission::Denied);
        assert_eq!(Permission::parse("notDetermined\n"), Permission::NotDetermined);
        for unknown in ["", "unknown", "authorized extra"] {
            assert_eq!(Permission::parse(unknown), Permission::Unknown, "{unknown}");
        }
    }

    #[test]
    fn 시험_알림은_헬퍼가_받는_모양이다() {
        let payload: serde_json::Value = serde_json::from_str(TEST_PAYLOAD).unwrap_or_default();
        for key in ["title", "body", "thread"] {
            assert!(payload[key].as_str().is_some_and(|text| !text.is_empty()), "{key}");
        }
        assert_eq!(payload["tier"], "P1");
        assert_eq!(payload["items"], serde_json::json!([]));
    }
}
