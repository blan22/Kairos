//! 설치 뒤 단계. Mac 알림 권한을 받고 Orca 패널 적재를 묻는다.
//!
//! 알림 권한은 터미널에서 묻지 않는다. macOS 권한 창이 묻는다(`commands::notifier`).
//! 둘 다 설치본(`<홈>/current`)을 쓴다. 개발 트리의 번들과 패널 폴더는 권한과 승인을 따로 받기 때문이다.
//! 패널 승인은 Orca가 보여 주는 권한 문구를 그대로 띄우고 사용자가 `y`라고 답할 때만 지문을 넘긴다.
//! `--yes`면 문구를 띄운 뒤 묻지 않고 승인한다. 지문은 이 설치본 폴더를 적재한 Orca의 응답에서만 받는다.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use kairos_core::KairosResult;
use kairos_core::process::{ProcessOutput, ProcessRequest};
use regex::Regex;

use super::Wizard;
use crate::commands::notifier::{Grant, Missing, NotifierHelper};

/// 설치본 안의 Orca 패널 폴더.
const PANEL_DIR: [&str; 2] = ["plugins", "orca-panel"];
/// 패널의 플러그인 키(`orca-plugin.json`의 게시자와 이름).
const PANEL_KEY: &str = "local.kairos";
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

    /// 터미널에서 묻지 않는다. 다른 Mac 앱처럼 macOS 권한 창이 묻고, 허용하면 그때부터 알림이 간다.
    /// 전에 거절했으면 권한 창이 다시 뜨지 않는다. 그때만 시스템 설정을 열지 묻는다. `--yes`와 입력이 끝난 실행은 열지 않고 알리기만 한다.
    fn notifier(&self, runtime: &Path) -> KairosResult<()> {
        let Some(helper) = NotifierHelper::in_runtime(self.context, runtime) else {
            self.console.print(Missing::Helper.message());
            return Ok(());
        };
        if helper.grant(self.console) != Grant::DeniedBefore || self.yes {
            return Ok(());
        }
        // 입력이 끝났으면 사람이 없다. 화면을 띄우지 않는다.
        let open = match self.ask("시스템 설정의 알림 화면을 열까요? [Y/n]")? {
            None => false,
            Some(answer) => !matches!(answer.to_lowercase().as_str(), "n" | "no" | "아니오" | "아니요"),
        };
        if open && !helper.open_settings() {
            self.console.print("시스템 설정을 열지 못했습니다. 직접 여세요");
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
        let path = dir.to_string_lossy();
        let load = ["plugin", "dev", "load", path.as_ref()];
        let first = self.run(&orca, &load, SHORT_TIMEOUT);
        let text = text_of(&first);
        let Some(fingerprint) =
            FINGERPRINT.captures(&text).and_then(|found| found.get(1)).map(|found| found.as_str().to_owned())
        else {
            if first.success() {
                self.settle(&orca, &dir);
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
            // `orca plugin dev load --approve`를 직접 부르면 타입 파일이 남아 워커가 뜨지 않는다. setup이 정리까지 한다.
            self.console.print("승인하지 않았습니다. 나중에 kairos setup을 다시 실행하면 이 단계를 다시 묻습니다");
            return Ok(());
        }
        let approved = self.run(&orca, &[&load[..], &["--approve", &fingerprint]].concat(), SHORT_TIMEOUT);
        if approved.success() {
            self.settle(&orca, &dir);
            self.console.print("Orca 패널을 승인했습니다. 사이드바의 kairos 아이콘이나 Mod+Alt+Shift+K로 엽니다");
        } else {
            self.console.print("승인하지 못했습니다");
            self.print_orca(&text_of(&approved));
        }
        Ok(())
    }

    /// 적재가 쓴 타입 파일 폴더(`.orca`)를 지우고 워커를 새로 띄운다.
    /// Orca(1.4.255에서 측정)는 개발 폴더를 승인할 때 `.orca`를 빼고 해시를 적지만, 워커를 띄울 때는 넣고 잰다.
    /// components 패널이면 `dev load`가 늘 `.orca/types`를 쓰므로, 두면 워커가 `content hash mismatch`로 뜨지 않는다.
    /// 타입 파일은 편집기용이라 설치본에는 없어도 된다. 재시작은 실패가 쌓여 멈춘 워커도 풀어 준다.
    fn settle(&self, orca: &str, dir: &Path) {
        match fs::remove_dir_all(dir.join(".orca")) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                self.console.print(&format!(
                    "패널의 타입 파일을 지우지 못했습니다({error}). 패널이 뜨지 않으면 문제 해결 문서를 보세요"
                ));
                return;
            }
        }
        self.run(orca, &["plugin", "restart", PANEL_KEY], SHORT_TIMEOUT);
    }

    fn print_orca(&self, text: &str) {
        for line in text.lines().filter(|line| !line.starts_with("Next step")) {
            self.console.print(&format!("  {line}"));
        }
    }
}
