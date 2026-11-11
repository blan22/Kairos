use kairos_core::KairosResult;

use crate::app::AppContext;
use crate::command::Command;

pub struct Doctor;

impl Command for Doctor {
    fn name(&self) -> &'static str {
        "doctor"
    }

    fn execute(&self, context: &AppContext, _args: &[String]) -> KairosResult<i32> {
        let report = context.doctor().diagnose()?;
        for line in &report.lines {
            context.console().print(line);
        }
        Ok(report.exit_code)
    }
}
