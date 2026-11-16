//! Linux와 Windows의 데스크톱 알림. macOS는 `NativeNotifier`(KairosNotifier.app)를 쓴다.
//!
//! - Linux: `notify-send`. 누르면 열리는 동작은 없다. `--wait`로 기다리면 틱이 멈추므로 쓰지 않는다. 패널의 알림 띠가 같은 항목을 보여 준다.
//! - Windows: PowerShell로 WinRT 토스트를 띄운다. 보낸 이름은 설치가 등록한 `dev.kairos.notifier`이고,
//!   누르면 `kairos-reveal:items=…` 링크로 `kairos reveal --link`가 뜬다.
//!
//! 셸을 거치지 않는다. Windows 스크립트는 상수이고 알림 내용은 16진수로 바꿔 넣는다. 스크립트 안에서 다시 XML 이스케이프한다.

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use kairos_core::ports::{Alert, AlertTier, Delivery, Notifier};
use kairos_core::process::{ProcessEnd, ProcessRequest, ProcessRunner};

/// 알림을 누르면 Windows가 부르는 링크의 앞부분. 설치기의 `REVEAL_SCHEME`과 같다.
pub const REVEAL_LINK_PREFIX: &str = "kairos-reveal:items=";
/// 토스트의 보낸 이름. 설치기가 HKCU에 등록한다.
pub const TOAST_APP_ID: &str = "dev.kairos.notifier";

const LINUX_TIMEOUT: Duration = Duration::from_secs(5);
/// PowerShell은 처음 뜰 때 몇 초 걸린다.
const WINDOWS_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT: usize = 64 * 1024;

/// 토스트를 띄우는 스크립트. `{PAYLOAD}`만 바꾼다. 들어가는 값은 16진수라 따옴표를 깰 수 없다.
const TOAST_SCRIPT: &str = r#"$ErrorActionPreference = 'Stop'
$hex = '{PAYLOAD}'
$bytes = [byte[]]::new($hex.Length / 2)
for ($i = 0; $i -lt $bytes.Length; $i++) { $bytes[$i] = [Convert]::ToByte($hex.Substring($i * 2, 2), 16) }
$p = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json
[void][Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]
[void][Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime]
$e = [Security.SecurityElement]
$x = '<toast activationType="protocol" launch="' + $e::Escape($p.launch) + '" scenario="' + $p.scenario + '"><visual><binding template="ToastGeneric"><text>' + $e::Escape($p.title) + '</text><text>' + $e::Escape($p.body) + '</text></binding></visual></toast>'
$d = New-Object Windows.Data.Xml.Dom.XmlDocument
$d.LoadXml($x)
[Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($p.app).Show([Windows.UI.Notifications.ToastNotification]::new($d))
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    NotifySend,
    Toast,
}

/// Linux와 Windows의 알림.
pub struct DesktopNotifier {
    runner: Arc<dyn ProcessRunner>,
    bin: String,
    backend: Backend,
}

/// notify-send 본문은 간단한 마크업을 읽는다. 항목 제목이 태그로 읽히지 않게 바꾼다.
fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn hex(text: &str) -> String {
    text.bytes().fold(String::with_capacity(text.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// 누르면 열 링크. 항목이 없으면 패널만 연다.
pub fn reveal_link(items: &[i64]) -> String {
    let ids: Vec<String> = items.iter().map(i64::to_string).collect();
    format!("{REVEAL_LINK_PREFIX}{}", ids.join(","))
}

/// `kairos-reveal:items=1,2`에서 항목 ID를 골라 낸다. 모양이 다르면 None이다. 끝의 `/`는 Windows가 붙일 수 있어 받는다.
pub fn parse_reveal_link(link: &str) -> Option<Vec<i64>> {
    let rest = link.strip_prefix(REVEAL_LINK_PREFIX)?;
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() {
        return Some(Vec::new());
    }
    rest.split(',')
        .map(|id| {
            (!id.is_empty() && id.len() <= 18 && id.bytes().all(|b| b.is_ascii_digit()))
                .then(|| id.parse().ok())
                .flatten()
        })
        .collect()
}

impl DesktopNotifier {
    /// `notify-send` 경로를 받는다.
    pub fn notify_send(runner: Arc<dyn ProcessRunner>, bin: impl Into<String>) -> Self {
        Self { runner, bin: bin.into(), backend: Backend::NotifySend }
    }

    /// `powershell.exe` 경로를 받는다.
    pub fn toast(runner: Arc<dyn ProcessRunner>, bin: impl Into<String>) -> Self {
        Self { runner, bin: bin.into(), backend: Backend::Toast }
    }

    fn request(&self, alert: &Alert) -> ProcessRequest {
        match self.backend {
            Backend::NotifySend => {
                let urgency = match alert.tier {
                    AlertTier::P0 => "--urgency=critical",
                    AlertTier::P1 => "--urgency=normal",
                };
                let args = ["--app-name=kairos", urgency, "--", &alert.title, &escape_markup(&alert.body)];
                ProcessRequest::new(self.bin.clone(), args, LINUX_TIMEOUT)
            }
            Backend::Toast => {
                let payload = serde_json::json!({
                    "app": TOAST_APP_ID,
                    "title": alert.title,
                    "body": alert.body,
                    "launch": reveal_link(&alert.items),
                    // 급한 알림은 사용자가 닫을 때까지 남긴다.
                    "scenario": if alert.tier == AlertTier::P0 { "reminder" } else { "default" },
                });
                let script = TOAST_SCRIPT.replace("{PAYLOAD}", &hex(&payload.to_string()));
                let args = ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script];
                ProcessRequest::new(self.bin.clone(), args, WINDOWS_TIMEOUT)
            }
        }
        .max_output(MAX_OUTPUT)
    }
}

impl Notifier for DesktopNotifier {
    fn notify(&self, alert: &Alert) -> bool {
        self.deliver(alert) == Delivery::Posted
    }

    /// 시간 초과는 이미 떴을 수 있어 `Unknown`이다. 그 밖의 실패는 게시하지 않았다.
    fn deliver(&self, alert: &Alert) -> Delivery {
        match self.runner.run(&self.request(alert)).end {
            ProcessEnd::Exited(0) => Delivery::Posted,
            ProcessEnd::TimedOut | ProcessEnd::Cancelled | ProcessEnd::OutputTooLarge => Delivery::Unknown,
            _ => Delivery::NotPosted,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kairos_core::process::ProcessOutput;

    use super::*;

    struct Recorder(Mutex<Vec<ProcessRequest>>, ProcessEnd);

    impl ProcessRunner for Recorder {
        fn run(&self, request: &ProcessRequest) -> ProcessOutput {
            self.0.lock().unwrap().push(request.clone());
            ProcessOutput::ended(self.1)
        }
    }

    fn alert(tier: AlertTier) -> Alert {
        Alert { tier, title: "kairos".into(), body: "리뷰 <b>요청</b> & 'x'".into(), items: vec![3, 41] }
    }

    #[test]
    fn notify_send는_인자_배열로_부르고_마크업을_바꾼다() {
        let runner = Arc::new(Recorder(Mutex::new(Vec::new()), ProcessEnd::Exited(0)));
        let notifier = DesktopNotifier::notify_send(runner.clone(), "/usr/bin/notify-send");
        assert_eq!(notifier.deliver(&alert(AlertTier::P0)), Delivery::Posted);
        let seen = runner.0.lock().unwrap();
        assert_eq!(seen[0].program, "/usr/bin/notify-send");
        assert_eq!(
            seen[0].args,
            ["--app-name=kairos", "--urgency=critical", "--", "kairos", "리뷰 &lt;b&gt;요청&lt;/b&gt; &amp; 'x'"]
        );
        assert!(seen[0].env.is_none(), "환경을 비우지 않는다. 알림 서버 주소가 환경에 있다");
    }

    #[test]
    fn 토스트는_상수_스크립트에_16진수만_넣는다() {
        let runner = Arc::new(Recorder(Mutex::new(Vec::new()), ProcessEnd::Exited(0)));
        let notifier =
            DesktopNotifier::toast(runner.clone(), r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        notifier.deliver(&alert(AlertTier::P1));
        let seen = runner.0.lock().unwrap();
        let script = seen[0].args.last().unwrap();
        let line = script.lines().nth(1).unwrap();
        let hex = line.strip_prefix("$hex = '").unwrap().strip_suffix('\'').unwrap();
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
        let bytes: Vec<u8> =
            (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
        let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(payload["launch"], "kairos-reveal:items=3,41");
        assert_eq!(payload["app"], TOAST_APP_ID);
        assert_eq!(payload["body"], "리뷰 <b>요청</b> & 'x'", "XML 이스케이프는 스크립트가 한다");
        assert_eq!(script.replace(line, ""), TOAST_SCRIPT.replace("$hex = '{PAYLOAD}'", ""));
    }

    #[test]
    fn 결과는_세_갈래다() {
        for (end, expected) in [
            (ProcessEnd::Exited(1), Delivery::NotPosted),
            (ProcessEnd::Missing, Delivery::NotPosted),
            (ProcessEnd::TimedOut, Delivery::Unknown),
        ] {
            let runner = Arc::new(Recorder(Mutex::new(Vec::new()), end));
            assert_eq!(DesktopNotifier::notify_send(runner, "notify-send").deliver(&alert(AlertTier::P1)), expected);
        }
    }

    #[test]
    fn 링크는_숫자_id만_받는다() {
        assert_eq!(parse_reveal_link(&reveal_link(&[3, 41])), Some(vec![3, 41]));
        assert_eq!(parse_reveal_link("kairos-reveal:items=7/"), Some(vec![7]));
        assert_eq!(parse_reveal_link("kairos-reveal:items="), Some(vec![]));
        for bad in [
            "kairos-reveal:items=1,,2",
            "kairos-reveal:items=-1",
            "kairos-reveal:items=1;calc",
            "https://x",
            "kairos-reveal:items=1 2",
        ] {
            assert_eq!(parse_reveal_link(bad), None, "{bad}");
        }
    }
}
