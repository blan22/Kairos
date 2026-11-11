use std::collections::BTreeMap;

use kairos_agenda::StateBuilder;
use kairos_agenda::{Presenter, SourceHealthBook};
use kairos_core::model::SourceName;
use kairos_core::{Instant, KairosResult};
use kairos_ledger::{ObjectRepo, upcoming_deploys};
use kairos_policy::DeployLinkStatus;
use kairos_store::Database;

use super::shared::{line, shown};
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag};
use crate::command::Command;

const OPTIONS: [OptionSpec; 4] = [flag("would-show"), flag("json"), flag("items"), flag("all")];

pub struct List;

/// kairos list --json과 --items. 기본 출력은 보드다.
fn item_list(context: &AppContext, json: bool, all: bool) -> KairosResult<i32> {
    let database = context.open_db()?;
    let now = context.now();
    let response = Presenter::new(&database).list_response(&context.next_context(&database, now)?, all)?;
    if json {
        context.console().emit(&response)?;
        return Ok(0);
    }
    if response.items.is_empty() {
        context.console().print("열린 항목 없음");
    }
    for view in &response.items {
        context.console().print(&line(view));
    }
    Ok(0)
}

fn print_would_show(context: &AppContext, database: &Database) -> KairosResult<()> {
    let console = context.console();
    let rows = StateBuilder::new(database.connection()).would_show_events()?;
    console.print(&format!("표시 없이 기록 중: 표시됐을 항목 {}건", rows.len()));
    let mut order: Vec<String> = Vec::new();
    let mut by_rule: BTreeMap<String, usize> = BTreeMap::new();
    for row in &rows {
        let rule = format!("{} {}", row.tier, row.event.rule_id);
        if !by_rule.contains_key(&rule) {
            order.push(rule.clone());
        }
        *by_rule.entry(rule).or_default() += 1;
    }
    for rule in order {
        console.print(&format!("{rule} {}건", by_rule[&rule]));
    }
    for row in &rows {
        let event = &row.event;
        let catchup = if event.catchup { " (공백 복구)" } else { "" };
        let label = event.source.parse::<SourceName>().map_or("undefined", SourceName::label);
        let title = shown(event.text_head.as_deref());
        console.print(&format!("{} {label} {} {}{catchup} {title}", row.tier, event.rule_id, event.occurred_at));
    }
    Ok(())
}

fn print_board(context: &AppContext, database: &Database, at: Instant) -> KairosResult<()> {
    let console = context.console();
    let rows = ObjectRepo::new(database.connection()).kind_counts()?;
    if rows.is_empty() {
        console.print("저장된 항목 없음");
    }
    for (kind, total, baseline) in rows {
        console.print(&format!("{kind} {total}건 (기준선 {baseline}건)"));
    }
    for deploy in upcoming_deploys(database.connection(), at)? {
        let status = if deploy.link.status == DeployLinkStatus::NoLink {
            "연결 없음".to_owned()
        } else {
            format!("연결 {}건", deploy.link.object_keys.len())
        };
        console.print(&format!("배포 {} {}: {status}", deploy.date, shown(deploy.object.text_head.as_deref())));
    }
    Ok(())
}

impl Command for List {
    fn name(&self) -> &'static str {
        "list"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos list [--would-show | --items | --json [--all]]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&OPTIONS).parse(args)?;
        // --items는 사람이 터미널에서 직접 보는 것이라 게이트를 따르지 않는다.
        if parsed.flag("json") {
            return item_list(context, true, parsed.flag("all"));
        }
        if parsed.flag("items") {
            return item_list(context, false, true);
        }
        let at = context.now();
        let database = context.open_db()?;
        let control = context.control();
        let warning = context.health_line(&database, at, &control)?;
        let first: Vec<String> =
            warning.into_iter().chain(SourceHealthBook::new(database.connection()).notice_lines(at)?).collect();
        context.console().print(&if first.is_empty() { "수집 정상".to_owned() } else { first.join(", ") });
        if parsed.flag("would-show") {
            print_would_show(context, &database)?;
        } else {
            print_board(context, &database, at)?;
        }
        Ok(0)
    }
}
