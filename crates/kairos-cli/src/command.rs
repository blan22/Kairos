//! 명령의 얼굴과 등록부. 새 명령은 `Command` 구현과 `commands::registry`의 한 줄이다.

use std::panic::{AssertUnwindSafe, catch_unwind};

use kairos_core::text::sanitize;
use kairos_core::{KairosError, KairosResult};

use crate::app::AppContext;
use crate::reply::Reply;

const JSON_FLAG: &str = "--json";
const PARSE_ERROR: &str = "ERR_PARSE_ARGS";
/// `error.code`가 없는 throw의 값. 패닉이 여기에 해당한다.
const UNEXPECTED: &str = "unexpected";

pub trait Command: Send + Sync {
    fn name(&self) -> &'static str;

    /// 옵션 오류에 답할 사용법 줄. 없으면 `--json`을 줘도 JSON으로 답하지 않고 표준 오류로 간다.
    fn usage(&self) -> Option<&'static str> {
        None
    }

    /// 명령의 몸통. 반환값은 종료 코드다. 원본의 throw가 `Err`다.
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32>;

    /// 몸통을 돌리고 오류를 끝낸다. 원본의 `withJsonErrors`와 `main`의 catch다.
    fn run(&self, context: &AppContext, args: &[String]) -> i32 {
        let outcome = catch_unwind(AssertUnwindSafe(|| self.execute(context, args)));
        let error = match outcome {
            Ok(Ok(code)) => return code,
            Ok(Err(error)) => error,
            Err(_) => KairosError::new(UNEXPECTED),
        };
        let code = error.code();
        match self.usage().filter(|_| args.iter().any(|arg| arg == JSON_FLAG)) {
            Some(usage) if code.starts_with(PARSE_ERROR) => Reply::new(context, true).usage(usage),
            Some(_) => Reply::new(context, true).fail("failed", code, 1),
            None => {
                context.console().error(&format!("kairos {} 실패: {}", self.name(), sanitize(code)));
                1
            }
        }
    }
}

/// 등록 순서가 `usage()`의 순서다.
#[derive(Default)]
pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with(mut self, command: impl Command + 'static) -> Self {
        self.commands.push(Box::new(command));
        self
    }

    pub fn names(&self) -> Vec<&'static str> {
        self.commands.iter().map(|command| command.name()).collect()
    }

    pub fn find(&self, name: &str) -> Option<&dyn Command> {
        self.commands.iter().find(|command| command.name() == name).map(AsRef::as_ref)
    }

    fn usage(&self, context: &AppContext) {
        context.console().print("사용법: kairos <명령>");
        context.console().print(&format!("명령: {}", self.names().join(", ")));
    }

    /// `argv`는 프로그램 이름을 뺀 인자다. 반환값은 종료 코드다.
    pub fn dispatch(&self, context: &AppContext, argv: &[String]) -> i32 {
        let Some((name, rest)) = argv.split_first() else {
            self.usage(context);
            return 2;
        };
        if name == "help" || name == "--help" {
            self.usage(context);
            return 0;
        }
        let Some(command) = self.find(name) else {
            context.console().print(&format!("알 수 없는 명령: {}", sanitize(name)));
            self.usage(context);
            return 2;
        };
        command.run(context, rest)
    }
}
