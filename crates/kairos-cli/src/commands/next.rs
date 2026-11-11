use kairos_agenda::{NextOptions, Presenter, RevealFile};
use kairos_core::KairosResult;

use super::shared::line;
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::reply::Reply;

const USAGE: &str = "사용법: kairos next [--seen] [--surface panel|orca|claude-code|vscode] [--ack] [--json]";
const OPTIONS: [OptionSpec; 4] = [flag("json"), flag("seen"), flag("ack"), text("surface")];

pub struct Next;

impl Command for Next {
    fn name(&self) -> &'static str {
        "next"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(USAGE)
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&OPTIONS).parse(args)?;
        let json = parsed.flag("json");
        // 호스트가 자기 이름을 넘긴다. Mac 알림은 카드가 아니라 수집 끝의 P1 알림이 맡는다.
        if !super::surface::SURFACES.contains(&parsed.text("surface").unwrap_or("panel")) {
            return Ok(Reply::new(context, json).usage(USAGE));
        }
        let database = context.open_db()?;
        let next_context = context.next_context(&database, context.now())?;
        let options = NextOptions { seen: parsed.flag("seen"), ack: parsed.flag("ack") };
        let mut response = Presenter::new(&database).next_response(&next_context, options)?;
        drop(database);
        // 알림을 눌러 연 항목. 10분이 지난 요청은 싣지 않는다.
        response.reveal = RevealFile::new(context.home()).recent(response.at);
        let console = context.console();
        if json {
            console.emit(&response)?;
            return Ok(0);
        }
        if let Some(health) = response.health.as_deref().filter(|health| !health.is_empty()) {
            console.print(health);
        }
        if let Some(held) = response.focus.held_line.as_deref().filter(|held| !held.is_empty()) {
            console.print(held);
        }
        let Some(card) = &response.card else {
            let hold = response.hold.as_ref().map(|hold| hold.message.as_str());
            console.print(hold.unwrap_or("지금 낼 제안이 없습니다"));
            return Ok(0);
        };
        console.print(&card.headline);
        for view in &card.items {
            console.print(&line(view));
        }
        if card.more > 0 {
            console.print(&format!("+{}", card.more));
        }
        Ok(0)
    }
}
