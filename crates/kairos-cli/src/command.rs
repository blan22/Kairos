//! 명령의 얼굴과 등록부. 새 명령은 `Command` 구현과 `commands::registry`의 한 줄이다.

use std::panic::{AssertUnwindSafe, catch_unwind};

use kairos_core::text::sanitize;
use kairos_core::{KairosError, KairosResult};

use crate::app::AppContext;
use crate::help;
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
                // 옵션을 잘못 줬으면 받는 옵션을 바로 보여 준다.
                if code.starts_with(PARSE_ERROR) {
                    let usage = self.usage().or_else(|| help::find(self.name()).and_then(|entry| entry.usage));
                    if let Some(usage) = usage {
                        context.console().error(usage);
                    }
                }
                1
            }
        }
    }
}

/// 명령의 등록부. 도움말의 묶음과 순서는 `help`의 표가 정한다.
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

    /// 명령 앞부분에 `--help`나 `-h`가 있는가. `--` 뒤는 값이라 보지 않는다.
    fn asks_help(rest: &[String]) -> bool {
        rest.iter().take_while(|arg| *arg != "--").any(|arg| arg == "--help" || arg == "-h")
    }

    /// `argv`는 프로그램 이름을 뺀 인자다. 반환값은 종료 코드다.
    pub fn dispatch(&self, context: &AppContext, argv: &[String]) -> i32 {
        let console = context.console();
        let Some((name, rest)) = argv.split_first() else {
            help::print_overview(console, self, false);
            return 2;
        };
        match name.as_str() {
            "--version" | "-V" | "version" => {
                console.print(&help::version_line());
                return 0;
            }
            "help" | "--help" | "-h" => {
                return match rest.first().map(String::as_str) {
                    None => {
                        help::print_overview(console, self, false);
                        0
                    }
                    Some("--all") => {
                        help::print_overview(console, self, true);
                        0
                    }
                    Some(topic) if help::print_command(console, self, topic) => 0,
                    Some(topic) => self.unknown(context, topic),
                };
            }
            _ => {}
        }
        let Some(command) = self.find(name) else {
            return self.unknown(context, name);
        };
        if Self::asks_help(rest) {
            help::print_command(console, self, name);
            return 0;
        }
        command.run(context, rest)
    }

    fn unknown(&self, context: &AppContext, name: &str) -> i32 {
        let console = context.console();
        console.print(&format!("알 수 없는 명령: {}", sanitize(name)));
        if let Some(near) = help::suggest(self, name) {
            console.print(&format!("혹시 이 명령인가요: kairos {near}"));
        }
        console.print("명령 목록은 kairos help로 봅니다");
        2
    }
}
