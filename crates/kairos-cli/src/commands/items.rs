use kairos_agenda::{ActionContext, ActionResult, ItemActions, LaterWhen, Presenter, TriageDecision};
use kairos_core::text::sanitize_head;
use kairos_core::{Instant, KairosResult, OffDates};
use kairos_ledger::OffDateBook;
use kairos_store::Database;
use serde_json::json;

use super::shared::{line, parse_ids};
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::reply::Reply;

const JSON_ONLY: [OptionSpec; 1] = [flag("json")];
const LATER_OPTIONS: [OptionSpec; 2] = [flag("json"), text("when")];
const LATER_USAGE: &str = "사용법: kairos later <항목 ID>... [--when hour|tomorrow] [--json]";
const DONE_OPTIONS: [OptionSpec; 2] = [flag("json"), flag("drop")];
const KEEP_OPTIONS: [OptionSpec; 2] = [flag("json"), flag("until-deploy")];

/// ID를 검사하고 조작을 한 뒤 결과를 낸다.
fn act(
    context: &AppContext,
    values: &[String],
    json: bool,
    work: impl FnOnce(&Database, &[i64], Instant, &OffDates) -> KairosResult<Vec<ActionResult>>,
) -> KairosResult<i32> {
    let reply = Reply::new(context, json);
    let ids = match parse_ids(values, reply) {
        Ok(ids) => ids,
        Err(code) => return Ok(code),
    };
    let at = context.now();
    let database = context.open_db()?;
    let off_dates = OffDateBook::new(database.connection()).load_off_dates(&context.control().off_date_set())?;
    let results = work(&database, &ids, at, &off_dates)?;
    drop(database);
    reply.actions(&results)
}

pub struct Later;

impl Command for Later {
    fn name(&self) -> &'static str {
        "later"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(LATER_USAGE)
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&LATER_OPTIONS).parse(args)?;
        let json = parsed.flag("json");
        // 때는 고정 낱말 둘뿐이다. 없으면 다음 묶음이다.
        let when = match parsed.text("when") {
            None => LaterWhen::NextBundle,
            Some("hour") => LaterWhen::Hour,
            Some("tomorrow") => LaterWhen::Tomorrow,
            Some(_) => return Ok(Reply::new(context, json).usage(LATER_USAGE)),
        };
        act(context, parsed.positionals(), json, |database, ids, now, off_dates| {
            ItemActions::new(database).later_when(ids, &ActionContext { now, off_dates }, when)
        })
    }
}

pub struct Done;

impl Command for Done {
    fn name(&self) -> &'static str {
        "done"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos done <항목 ID>... [--drop] [--json]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&DONE_OPTIONS).parse(args)?;
        let drop = parsed.flag("drop");
        act(context, parsed.positionals(), parsed.flag("json"), |database, ids, now, _| {
            ItemActions::new(database).done(ids, now, drop)
        })
    }
}

pub struct Keep;

impl Command for Keep {
    fn name(&self) -> &'static str {
        "keep"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos keep <항목 ID>... [--until-deploy] [--json]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&KEEP_OPTIONS).parse(args)?;
        let until_deploy = parsed.flag("until-deploy");
        act(context, parsed.positionals(), parsed.flag("json"), |database, ids, now, off_dates| {
            ItemActions::new(database).keep(ids, &ActionContext { now, off_dates }, until_deploy)
        })
    }
}

pub struct Add;

impl Command for Add {
    fn name(&self) -> &'static str {
        "add"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos add <제목> [--json]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let title = parsed.positionals().join(" ");
        if sanitize_head(&title).is_empty() {
            return Ok(reply.usage("사용법: kairos add <제목>"));
        }
        let at = context.now();
        let database = context.open_db()?;
        let result = ItemActions::new(&database).add(&title, at)?;
        drop(database);
        reply.actions(&[result])
    }
}

pub struct Triage;

impl Triage {
    fn decide(
        context: &AppContext,
        values: &[String],
        decision: TriageDecision,
        at: Instant,
        reply: Reply<'_>,
    ) -> KairosResult<i32> {
        let ids = match parse_ids(values, reply) {
            Ok(ids) => ids,
            Err(code) => return Ok(code),
        };
        let database = context.open_db()?;
        let results = ItemActions::new(&database).triage_decide(&ids, decision, at)?;
        drop(database);
        reply.actions(&results)
    }

    fn show(context: &AppContext, at: Instant, reply: Reply<'_>) -> KairosResult<i32> {
        let database = context.open_db()?;
        let batch = ItemActions::new(&database).triage_batch(at)?;
        let items = Presenter::new(&database).triage_views(&batch.rows, at)?;
        drop(database);
        if reply.json() {
            return reply.success(json!({ "at": at, "items": items, "remaining": batch.remaining }));
        }
        if items.is_empty() {
            context.console().print("고를 기준선 항목이 없습니다");
        }
        for view in &items {
            context.console().print(&line(view));
        }
        if batch.remaining > 0 {
            context.console().print(&format!("남은 건 {}건", batch.remaining));
        }
        Ok(0)
    }
}

impl Command for Triage {
    fn name(&self) -> &'static str {
        "triage"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("사용법: kairos triage [keep|drop <항목 ID>...] [--json]")
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&JSON_ONLY).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let at = context.now();
        match parsed.positionals().split_first() {
            None => Self::show(context, at, reply),
            Some((decision, rest)) if decision == "keep" => {
                Self::decide(context, rest, TriageDecision::Keep, at, reply)
            }
            Some((decision, rest)) if decision == "drop" => {
                Self::decide(context, rest, TriageDecision::Drop, at, reply)
            }
            Some(_) => Ok(reply.usage("사용법: kairos triage [keep|drop <항목 ID>...]")),
        }
    }
}
