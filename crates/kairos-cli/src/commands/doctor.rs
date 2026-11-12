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
        // 종료 코드만으로는 무엇이 문제인지 보이지 않는다. 끝에 모아서 한 번 더 보여 준다.
        context.console().print("");
        if report.problems.is_empty() {
            context.console().print("문제 없음");
        } else {
            context.console().print(&format!("확인할 것 {}건", report.problems.len()));
            for problem in &report.problems {
                context.console().print(&format!("  - {problem}"));
            }
        }
        Ok(report.exit_code)
    }
}
