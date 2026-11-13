use kairos_agenda::{Explainer, ItemRepo, NextOptions, Presenter, RevealFile, SourceHealthBook};
use kairos_core::KairosResult;

use super::shared::line;
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::reply::Reply;

const USAGE: &str =
    "사용법: kairos next [--seen | --seen-ready] [--surface panel|orca|claude-code|vscode] [--ack] [--json]";
const OPTIONS: [OptionSpec; 5] = [flag("json"), flag("seen"), flag("seen-ready"), flag("ack"), text("surface")];

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
        let presenter = Presenter::new(&database);
        // --seen-ready는 세션 소식만 확인으로 적는다. 카드를 그리지 않는 화면이 쓴다. --seen은 둘 다 적으므로 함께 오면 --seen을 따른다.
        let mut response = if parsed.flag("seen-ready") && !options.seen {
            presenter.next_response_ready_seen(&next_context, options)?
        } else {
            presenter.next_response(&next_context, options)?
        };
        // 소스별 수집 상태. 게이트가 닫혀 있어도 낸다. 기록만 하는 동안에도 수집이 도는지 보여야 한다.
        let sources =
            SourceHealthBook::summary(&context.source_health(&database, next_context.now, &next_context.control)?);
        response.sources = (!sources.is_empty()).then_some(sources);
        // 펼친 카드가 보여 줄 근거와 상태 표시줄의 새 항목 점. golden이 지키는 Presenter 출력과 따로 채운다.
        if let Some(card) = response.card.as_mut() {
            Explainer::new(database.connection()).fill(&mut card.items)?;
        }
        response.newest = ItemRepo::new(database.connection()).newest_open_id()?;
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
