//! 감시자 한 번. Linux와 Windows의 스케줄러가 5분마다 부른다. macOS는 같은 판정을 하는 zsh 스크립트를 쓴다.
//! 판정은 `kairos_ops::Watchdog`에 있다. 여기서는 시각, 시간대, 알림만 고른다.
//! `KAIROS_WATCHDOG_NOW`(초)와 `KAIROS_WATCHDOG_DRY=1`은 zsh 판과 같은 시험용 값이다.

use std::sync::Arc;

use kairos_core::KairosResult;
use kairos_core::ports::{Alert, AlertTier, Delivery, Notifier};
use kairos_ops::Watchdog as Check;

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec};
use crate::command::Command;

const OPTIONS: [OptionSpec; 0] = [];
const TITLE: &str = "kairos 점검";
/// 알릴 도구가 없을 때의 코드. zsh 판과 같다.
const NO_NOTIFIER: i32 = 127;
/// 게시됐는지 모를 때의 코드. macOS 헬퍼의 게시 실패 코드와 같다.
const UNKNOWN: i32 = 4;

fn deliver(notifier: Option<&Arc<dyn Notifier>>, message: &str) -> i32 {
    let Some(notifier) = notifier else {
        return NO_NOTIFIER;
    };
    let alert = Alert { tier: AlertTier::P1, title: TITLE.to_owned(), body: message.to_owned(), items: Vec::new() };
    match notifier.deliver(&alert) {
        Delivery::Posted => 0,
        Delivery::NotPosted => 1,
        Delivery::Unknown => UNKNOWN,
    }
}

pub struct Watchdog;

impl Command for Watchdog {
    fn name(&self) -> &'static str {
        "watchdog"
    }

    /// 판정 결과는 표준 출력에 한 줄씩 찍는다. 스케줄러가 로그 파일에 붙인다. 알림이 실패해도 0으로 끝난다(다음 실행이 다시 알린다).
    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        ArgParser::new(&OPTIONS).parse(args)?;
        let now = context
            .env("KAIROS_WATCHDOG_NOW")
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or_else(|| context.now().millis().div_euclid(1000));
        let check = Check {
            home: context.home(),
            now,
            zone: kairos_core::profile::current().schedule.zone,
            dry: context.env("KAIROS_WATCHDOG_DRY") == Some("1"),
        };
        let notifier = context.notifier();
        let notify = |message: &str| deliver(notifier.as_ref(), message);
        for line in check.run(&notify)? {
            context.console().print(&line);
        }
        Ok(0)
    }
}
