use std::path::{Path, PathBuf};

use kairos_core::KairosResult;
use kairos_core::text::sanitize_text;
use kairos_ops::{
    InstallMode, InstallOptions, Installer, RemovalPlan, UninstallOptions, plan_runtime_removal, purge_refusal,
    remove_data, remove_runtime, runtime_refusal,
};

use super::tools::{PLAIN_LIMIT, is_plain, shell_quote};
use super::{notifier, sources};
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;

const INSTALL_OPTIONS: [OptionSpec; 8] = [
    flag("dry-run"),
    flag("apply"),
    flag("register"),
    text("launch-agents-dir"),
    text("preview-dir"),
    text("claude"),
    text("gh"),
    text("root"),
];
const UNINSTALL_OPTIONS: [OptionSpec; 6] =
    [flag("dry-run"), text("launch-agents-dir"), flag("no-launchctl"), flag("runtime"), flag("purge"), flag("yes")];

/// 화면에 찍을 경로. 폴더 이름에 든 제어 문자와 bidi 문자는 찍지 않는다.
fn shown(path: &Path) -> String {
    sanitize_text(&path.to_string_lossy(), PLAIN_LIMIT)
}

fn print_lines(context: &AppContext, lines: &[String], failed: bool) -> i32 {
    for line in lines {
        context.console().print(line);
    }
    i32::from(failed)
}

pub struct Install;

impl Command for Install {
    fn name(&self) -> &'static str {
        "install"
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&INSTALL_OPTIONS).parse(args)?;
        let root = match parsed.text("root") {
            Some(root) => PathBuf::from(root),
            None => context.layout()?.asset_root(),
        };
        // 등록은 --register일 때만. 플래그가 없거나 --dry-run이 있으면 아무것도 쓰지 않는다.
        let mode = if parsed.flag("dry-run") {
            InstallMode::DryRun
        } else if parsed.flag("register") {
            InstallMode::Register
        } else if parsed.flag("apply") {
            InstallMode::Apply
        } else {
            InstallMode::DryRun
        };
        let options = InstallOptions {
            mode,
            launch_agents_dir: parsed.text("launch-agents-dir").map(PathBuf::from),
            preview_dir: parsed.text("preview-dir").map(PathBuf::from),
            claude: parsed.text("claude").map(str::to_owned),
            gh: parsed.text("gh").map(str::to_owned),
            ..InstallOptions::new(context.home().clone(), root)
        };
        let result = Installer::new(context.runner()).install(&options)?;
        Ok(print_lines(context, &result.lines, result.failed))
    }
}

pub struct Uninstall;

impl Command for Uninstall {
    fn name(&self) -> &'static str {
        "uninstall"
    }

    /// 기본은 launchd 잡만 내린다. `--runtime`은 실행본과 명령 링크까지, `--purge`는 데이터 폴더와 키체인의 세션 토큰까지 지운다.
    /// 지울 것을 먼저 보여 주고 `y`일 때만 지운다(`--yes`면 묻지 않는다). 잡을 먼저 내려 수집이 지우는 중인 DB를 쓰지 않게 한다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&UNINSTALL_OPTIONS).parse(args)?;
        let console = context.console();
        let purge = parsed.flag("purge");
        let runtime = purge || parsed.flag("runtime");
        let dry_run = parsed.flag("dry-run");
        let (home, user_home) = (context.home().root(), context.user_home());
        // dry-run은 목록만 보여 주므로 세션 안에서도 된다.
        if runtime && !dry_run && sources::session_caller(context) {
            console.print("세션 안에서는 설치본을 지울 수 없습니다. 일반 터미널에서 실행하세요");
            return Ok(1);
        }
        // 잘못 준 KAIROS_HOME에서 남의 폴더를 지우지 않는다. 실행본만 지울 때도 자리를 본다.
        let refusal = match (purge, runtime) {
            (true, _) => purge_refusal(home, user_home),
            (false, true) => runtime_refusal(home, user_home),
            (false, false) => None,
        };
        if let Some(reason) = refusal {
            console.print(&format!("지우지 않음: {reason}"));
            return Ok(1);
        }
        let plan = runtime.then(|| plan_runtime_removal(home, user_home));
        if let Some(plan) = &plan {
            console.print(if dry_run { "지울 것(dry-run이라 지우지 않음)" } else { "지울 것" });
            for path in plan.command_link.iter().chain(plan.current.iter()).chain(plan.runtimes.iter()) {
                console.print(&format!("  {}", shown(path)));
            }
            if purge {
                console.print(&format!("  {} (수집 기록, 설정, 백업, 로그 전체)", shown(home)));
                console.print("  키체인의 세션 토큰");
            } else if plan.is_empty() {
                console.print("  실행본 없음");
            }
            if !dry_run && !parsed.flag("yes") && !Self::confirmed(context, purge)? {
                console.print("그만둡니다. 아무것도 바꾸지 않았습니다");
                return Ok(1);
            }
        }
        let options = UninstallOptions {
            launch_agents_dir: parsed.text("launch-agents-dir").map(PathBuf::from),
            dry_run,
            skip_launchctl: parsed.flag("no-launchctl"),
            before_removal: runtime,
        };
        let result = Installer::new(context.runner()).uninstall(&options)?;
        let mut failed = result.failed;
        for line in &result.lines {
            console.print(line);
        }
        // 시험과 복구용 옵션이다. 잡이 내려갔는지 확인하지 않고 지우게 되므로 알린다.
        if runtime && options.skip_launchctl && !dry_run {
            console.print("launchctl을 부르지 않았습니다. 올라 있는 잡이 있으면 지운 경로를 계속 가리킵니다");
        }
        let Some(plan) = plan else {
            return Ok(i32::from(failed));
        };
        // 내려가지 않은 잡은 지운 경로를 계속 가리킨다. 그 자리에 다시 놓이는 파일을 launchd가 돌리므로 여기서 멈춘다.
        if failed && !dry_run {
            console.print("잡을 내리지 못해 실행본과 데이터는 지우지 않았습니다. 위 줄을 확인한 뒤 다시 실행하세요");
            return Ok(1);
        }
        // 실행본을 지우면 경로를 알 수 없다. 내리는 명령에 쓸 패널 폴더를 먼저 읽는다.
        let panels = plan.panel_dirs();
        let removed = if dry_run { Ok(false) } else { Self::remove(context, &plan, purge) };
        // 플러그인 승인은 사용자가 했으므로 내리는 것도 사용자가 한다.
        for panel in panels {
            // 붙여 넣을 명령이다. 그대로 찍을 수 없는 경로면 명령에 넣지 않는다.
            let path = panel.to_string_lossy();
            let target = if is_plain(&path) {
                shell_quote(&path)
            } else {
                "<설치본의 plugins/orca-panel 경로>".to_owned()
            };
            console.print(&format!("Orca 패널을 올린 적이 있다면 내리세요: orca plugin dev unload {target}"));
        }
        // 지우다 멈췄어도 패널을 내리는 방법은 위에서 알렸다.
        failed |= removed?;
        Ok(i32::from(failed))
    }
}

impl Uninstall {
    /// 계획대로 지운다. 키체인 토큰을 지우지 못했으면 true다. 데이터 폴더를 먼저 지운다. 데이터를 지우다 멈추면 토큰은 남는다.
    fn remove(context: &AppContext, plan: &RemovalPlan, purge: bool) -> KairosResult<bool> {
        let console = context.console();
        let (home, user_home) = (context.home().root(), context.user_home());
        // 번들을 지우기 전에 LaunchServices에서 뺀다. 실패해도 이어 간다. 없는 번들은 macOS가 스스로 정리한다.
        for app in plan.notifier_apps() {
            notifier::unregister(context, &app);
        }
        for line in remove_runtime(plan)? {
            console.print(&sanitize_text(&line, PLAIN_LIMIT));
        }
        if !purge {
            console.print(&format!("데이터 폴더는 남겼습니다: {}", shown(home)));
            return Ok(false);
        }
        remove_data(home, user_home)?;
        console.print(&format!("지움 {}", shown(home)));
        match context.keychain().delete() {
            Ok(true) => console.print("지움 키체인의 세션 토큰"),
            Ok(false) => {}
            Err(_) => {
                console
                    .print("키체인의 세션 토큰을 지우지 못했습니다. 키체인 접근에서 kairos-session-token을 지우세요");
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// 지우기 전에 묻는다. 기본은 아니오이고 입력이 끝나도 아니오다.
    fn confirmed(context: &AppContext, purge: bool) -> KairosResult<bool> {
        let console = context.console();
        console.prompt(if purge {
            "데이터까지 모두 지웁니다. 되돌릴 수 없습니다. 계속할까요? [y/N]"
        } else {
            "실행본을 지웁니다. 데이터 폴더는 남깁니다. 계속할까요? [y/N]"
        });
        let answer = console.read_line()?.unwrap_or_default();
        Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes" | "예" | "네"))
    }
}
