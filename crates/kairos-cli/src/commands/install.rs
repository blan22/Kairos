use std::path::PathBuf;

use kairos_core::KairosResult;
use kairos_ops::{InstallMode, InstallOptions, Installer, UninstallOptions};

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
const UNINSTALL_OPTIONS: [OptionSpec; 3] = [flag("dry-run"), text("launch-agents-dir"), flag("no-launchctl")];

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

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&UNINSTALL_OPTIONS).parse(args)?;
        let options = UninstallOptions {
            launch_agents_dir: parsed.text("launch-agents-dir").map(PathBuf::from),
            dry_run: parsed.flag("dry-run"),
            skip_launchctl: parsed.flag("no-launchctl"),
        };
        let result = Installer::new(context.runner()).uninstall(&options)?;
        Ok(print_lines(context, &result.lines, result.failed))
    }
}
