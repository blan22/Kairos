//! 설치 뒤 단계. Mac 알림 권한과 Orca 패널 적재를 묻고 그 자리에서 한다.
//!
//! 둘 다 설치본(`<홈>/current`)을 쓴다. 개발 트리의 번들과 패널 폴더는 권한과 승인을 따로 받기 때문이다.
//! 패널 승인은 Orca가 보여 주는 권한 문구를 그대로 띄우고 사용자가 `y`라고 답할 때만 지문을 넘긴다.
//! `--yes`면 문구를 띄운 뒤 묻지 않고 승인한다. 지문은 이 설치본 폴더를 적재한 Orca의 응답에서만 받는다.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use kairos_core::KairosResult;
use kairos_core::process::{ProcessOutput, ProcessRequest};
use regex::Regex;

use super::Wizard;

/// LaunchServices 등록기. 알림을 누르면 macOS가 등록된 번들을 띄운다. 시험은 `KAIROS_LSREGISTER`로 가짜를 준다.
const LSREGISTER_BIN: &str =
    "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";
/// 설치본 안의 알림 헬퍼 번들.
const NOTIFIER_APP: [&str; 3] = ["ops", "notifier", "KairosNotifier.app"];
/// 번들 안의 실행 파일.
const NOTIFIER_EXEC: [&str; 3] = ["Contents", "MacOS", "KairosNotifier"];
/// 설치본 안의 Orca 패널 폴더.
const PANEL_DIR: [&str; 2] = ["plugins", "orca-panel"];
/// 헬퍼의 `setup`은 권한 창에서 최대 120초 기다린다.
const NOTIFIER_SETUP_TIMEOUT: Duration = Duration::from_secs(150);
const SHORT_TIMEOUT: Duration = Duration::from_secs(30);
/// Orca가 승인을 받을 때 보여 주는 지문 줄.
static FINGERPRINT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^Fingerprint:\s*(ap1-[0-9a-f]{16,64})\s*$").expect("상수 정규식"));

fn join(base: &Path, parts: &[&str]) -> PathBuf {
    parts.iter().fold(base.to_path_buf(), |path, part| path.join(part))
}

/// 출력 두 갈래를 한 덩어리로 본다. Orca는 안내를 stdout과 stderr 어느 쪽에도 쓴다.
fn text_of(output: &ProcessOutput) -> String {
    [output.stdout.trim_end(), output.stderr.trim_end()]
        .iter()
        .filter(|part| !part.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("\n")
}

impl Wizard<'_> {
    /// 다섯째 단계. 설치본이 없으면 건너뛴다.
    pub(super) fn surfaces(&self) -> KairosResult<()> {
        self.step("5/5 알림과 Orca 패널");
        let Ok(runtime) = fs::canonicalize(self.root.join("current")) else {
            self.console.print("설치본이 없어 건너뜁니다. 설치한 뒤 kairos setup을 다시 실행하세요");
            return Ok(());
        };
        self.notifier(&runtime)?;
        self.panel(&runtime)?;
        self.console.print("");
        self.console.print("상태는 kairos doctor로 봅니다.");
        self.console.print("세션은 잠겨 있습니다. 차단 시험을 확인한 뒤 kairos sessions on으로 켭니다");
        Ok(())
    }

    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> ProcessOutput {
        self.context.runner().run(&ProcessRequest::new(program, args.iter().copied(), timeout))
    }

    fn notifier(&self, runtime: &Path) -> KairosResult<()> {
        let (app, bin) = match self.context.env("KAIROS_NOTIFIER").map(str::trim).filter(|value| !value.is_empty()) {
            // 시험의 가짜 헬퍼. 번들은 실행 파일의 세 단계 위다.
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
        if !bin.is_file() {
            self.console.print("알림 헬퍼가 설치본에 없습니다. ops/notifier/build.sh로 만든 뒤 다시 설치하세요");
            return Ok(());
        }
        let bin_text = bin.to_string_lossy();
        if self.run(&bin_text, &["status"], SHORT_TIMEOUT).stdout.trim() == "authorized" {
            self.console.print("Mac 알림: 권한이 이미 있습니다");
            return Ok(());
        }
        if !self.agree("Mac 알림 권한을 받을까요? 권한 창이 뜨면 허용을 누르세요", true)? {
            self.console.print(&format!("나중에: \"{bin_text}\" setup"));
            return Ok(());
        }
        let lsregister = self
            .context
            .env("KAIROS_LSREGISTER")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(LSREGISTER_BIN);
        // 등록이 실패해도 권한은 받을 수 있다. 알림을 눌렀을 때 패널이 열리지 않을 뿐이다.
        if !self.run(lsregister, &["-f", &app.to_string_lossy()], SHORT_TIMEOUT).success() {
            self.console.print("LaunchServices 등록에 실패했습니다. 알림을 눌러도 패널이 열리지 않을 수 있습니다");
        }
        let done = self.run(&bin_text, &["setup"], NOTIFIER_SETUP_TIMEOUT);
        if done.success() {
            self.console.print("Mac 알림 권한을 받았습니다");
        } else {
            self.console.print("알림 권한을 받지 못했습니다. 시스템 설정 > 알림 > KairosNotifier에서 켜거나 kairos setup을 다시 실행하세요");
        }
        Ok(())
    }

    fn panel(&self, runtime: &Path) -> KairosResult<()> {
        let orca = self.context.panel_orca_bin();
        if !Path::new(&orca).is_file() {
            self.console.print("Orca가 없어 패널을 건너뜁니다");
            return Ok(());
        }
        let dir = join(runtime, &PANEL_DIR);
        if !dir.is_dir() {
            self.console.print("설치본에 Orca 패널이 없습니다. 다시 설치하세요");
            return Ok(());
        }
        if !self.agree("Orca 패널을 올릴까요?", true)? {
            return Ok(());
        }
        let dir = dir.to_string_lossy();
        let load = ["plugin", "dev", "load", dir.as_ref()];
        let first = self.run(&orca, &load, SHORT_TIMEOUT);
        let text = text_of(&first);
        let Some(fingerprint) =
            FINGERPRINT.captures(&text).and_then(|found| found.get(1)).map(|found| found.as_str().to_owned())
        else {
            if first.success() {
                self.console.print("Orca 패널을 올렸습니다");
            } else {
                self.console.print("Orca 패널을 올리지 못했습니다. Orca가 켜져 있는지 확인하세요");
                self.print_orca(&text);
            }
            return Ok(());
        };
        // Orca가 보여 주는 권한 문구다. 다음 단계 안내(명령 줄)는 setup이 대신하므로 뺀다.
        self.console.print("Orca가 이 권한으로 승인을 받습니다:");
        self.print_orca(&text);
        if !self.agree("이 권한으로 kairos 패널을 승인할까요?", false)? {
            self.console
                .print(&format!("승인하지 않았습니다. 나중에: orca plugin dev load '{dir}' --approve {fingerprint}"));
            return Ok(());
        }
        let approved = self.run(&orca, &[&load[..], &["--approve", &fingerprint]].concat(), SHORT_TIMEOUT);
        if approved.success() {
            self.console.print("Orca 패널을 승인했습니다. 사이드바의 종 아이콘이나 Mod+Alt+Shift+K로 엽니다");
        } else {
            self.console.print("승인하지 못했습니다");
            self.print_orca(&text_of(&approved));
        }
        Ok(())
    }

    fn print_orca(&self, text: &str) {
        for line in text.lines().filter(|line| !line.starts_with("Next step")) {
            self.console.print(&format!("  {line}"));
        }
    }
}
