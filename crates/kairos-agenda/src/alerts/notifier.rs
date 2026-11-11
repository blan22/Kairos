use std::sync::Arc;
use std::time::Duration;

use kairos_core::ports::{Alert, AlertTier, Delivery, Notifier, PanelOpener};
use kairos_core::process::{ProcessEnd, ProcessRequest, ProcessRunner};
use serde_json::json;

const NOTIFY_TIMEOUT: Duration = Duration::from_secs(5);
/// `execFile`의 기본 maxBuffer.
const MAX_OUTPUT: usize = 1024 * 1024;

pub const ORCA_BIN: &str = "/Applications/Orca.app/Contents/Resources/bin/orca";
/// 패널 명령의 고정 인자. 외부 값이 없다. Orca는 이 명령으로 창을 앞으로 끌어오거나 포커스를 가져가지 않는다(orca plugin open 도움말).
pub const PANEL_OPEN_ARGS: [&str; 6] = ["plugin", "open", "local.kairos", "--panel", "kairos", "--json"];
/// `open -b`로 Orca를 앞으로 끌어올 때 쓰는 번들 ID.
pub const ORCA_BUNDLE_ID: &str = "com.stablyai.orca";
/// 알림 헬퍼의 실행 파일. 설치 자료 폴더 기준이다. 설치는 `ops`를 복사하고 이 폴더에는 번들만 놓는다.
pub const NOTIFIER_APP_REL: &str = "ops/notifier/KairosNotifier.app/Contents/MacOS/KairosNotifier";

/// 헬퍼의 종료 코드. 2는 입력 오류, 3은 권한 없음이라 게시하지 않았다. 4는 게시 실패이거나 알림 센터의 응답을 기다리다 끝난 것이다.
const EXIT_ADD_FAILED: i32 = 4;

/// 헬퍼가 알림을 묶는 스레드. 급마다 따로 쌓인다.
const fn thread_of(tier: AlertTier) -> &'static str {
    match tier {
        AlertTier::P0 => "kairos.p0",
        AlertTier::P1 => "kairos.p1",
    }
}

/// 알림 헬퍼(KairosNotifier.app). 누르면 담긴 항목을 패널에 보여 준다.
/// 내용은 JSON 한 덩어리를 argv로 넘긴다. 셸을 거치지 않는다. 종료 0만 게시로 본다. 권한이 없으면 대화상자 없이 실패한다.
pub struct NativeNotifier {
    runner: Arc<dyn ProcessRunner>,
    bin: String,
}

impl NativeNotifier {
    pub fn new(runner: Arc<dyn ProcessRunner>, bin: impl Into<String>) -> Self {
        Self { runner, bin: bin.into() }
    }

    /// `post`에 넘기는 JSON.
    pub fn payload(alert: &Alert) -> String {
        json!({
            "title": alert.title,
            "body": alert.body,
            "items": alert.items,
            "tier": alert.tier.as_str(),
            "thread": thread_of(alert.tier),
        })
        .to_string()
    }
}

impl Notifier for NativeNotifier {
    fn notify(&self, alert: &Alert) -> bool {
        self.deliver(alert) == Delivery::Posted
    }

    /// 4와 시간 초과는 요청이 알림 센터에 이미 들어갔을 수 있어 `Unknown`이다. 5(알림 센터가 거절), 2, 3, 헬퍼가 없거나 스스로 죽었으면 게시하지 않았다.
    fn deliver(&self, alert: &Alert) -> Delivery {
        let payload = Self::payload(alert);
        let request =
            ProcessRequest::new(self.bin.clone(), ["post", payload.as_str()], NOTIFY_TIMEOUT).max_output(MAX_OUTPUT);
        match self.runner.run(&request).end {
            ProcessEnd::Exited(0) => Delivery::Posted,
            ProcessEnd::Exited(EXIT_ADD_FAILED)
            | ProcessEnd::TimedOut
            | ProcessEnd::Cancelled
            | ProcessEnd::OutputTooLarge => Delivery::Unknown,
            _ => Delivery::NotPosted,
        }
    }
}

/// Orca 패널을 연다.
pub struct OrcaPanelOpener {
    runner: Arc<dyn ProcessRunner>,
    bin: String,
}

impl OrcaPanelOpener {
    pub fn new(runner: Arc<dyn ProcessRunner>) -> Self {
        Self::with_bin(runner, ORCA_BIN)
    }

    pub fn with_bin(runner: Arc<dyn ProcessRunner>, bin: impl Into<String>) -> Self {
        Self { runner, bin: bin.into() }
    }
}

impl PanelOpener for OrcaPanelOpener {
    fn open(&self) -> bool {
        let request =
            ProcessRequest::new(self.bin.clone(), PANEL_OPEN_ARGS, NOTIFY_TIMEOUT).cwd("/").max_output(MAX_OUTPUT);
        self.runner.run(&request).success()
    }
}
