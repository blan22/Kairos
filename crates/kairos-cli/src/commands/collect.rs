use std::sync::LazyLock;

use kairos_core::model::SourceName;
use kairos_core::{Instant, KairosResult};
use kairos_ledger::Ledger;
use kairos_ops::counts_as_failure;
use kairos_tick::{CollectReport, CollectStatus, HaltFlag, js_number_text};
use regex::Regex;

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::js_number::seconds_fixed1;

const OPTIONS: [OptionSpec; 4] = [flag("force"), flag("resume"), text("skip-slack-until"), flag("reread-notion")];

static SLACK_TS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]+(?:\.[0-9]+)?$").expect("상수 정규식"));

/// Slack의 Message_ts(초)나 ISO 시각을 받는다. 지금 이후는 건너뛸 수 없다.
fn skip_instant(value: &str, at: Instant) -> Option<Instant> {
    if SLACK_TS.is_match(value) {
        let ms = value.parse::<f64>().ok()? * 1000.0;
        // `new Date(ms)`는 소수를 버린다.
        return (ms <= at.millis() as f64).then(|| Instant::from_millis(ms as i64));
    }
    Instant::parse(value).filter(|instant| *instant <= at)
}

/// 파서가 고칠 수 없는 이유로 Slack 질의가 멈췄을 때 쓰는 복구 명령. 건너뛴 구간은 doctor에 남는다.
fn skip_slack(context: &AppContext, value: &str) -> KairosResult<i32> {
    let at = context.now();
    let console = context.console();
    let Some(until) = skip_instant(value, at) else {
        console.print(
            "사용법: kairos collect --skip-slack-until <Message_ts 또는 ISO 시각>. 지금 이후는 고를 수 없습니다.",
        );
        return Ok(2);
    };
    let database = context.open_db()?;
    let ledger = Ledger::new(&database);
    let mut moved = Vec::new();
    if let Some(slack) = context.registry().find(SourceName::Slack) {
        for query in slack.queries() {
            if ledger.skip_cursor(SourceName::Slack, &query.id, until, at)? {
                moved.push(query.id.as_str());
            }
        }
    }
    console.print(&if moved.is_empty() {
        "옮긴 커서가 없습니다.".to_owned()
    } else {
        format!("Slack {}: {until}까지 건너뜁니다.", moved.join(", "))
    });
    Ok(0)
}

/// Notion 파서를 고친 뒤 부른다. 형식을 몰라 저장한 메일과 격리한 메일을 다음 틱부터 다시 읽는다.
fn reread_notion(context: &AppContext) -> KairosResult<i32> {
    let database = context.open_db()?;
    let count = Ledger::new(&database).reread_notion(context.now())?;
    context.console().print(&format!("Notion 메일 {count}건을 다시 읽습니다."));
    Ok(0)
}

fn known(value: Option<f64>) -> String {
    value.map_or_else(|| "알 수 없음".to_owned(), js_number_text)
}

fn print_counts(context: &AppContext, report: &CollectReport) {
    let lines = [
        (report.notified, "P0 알림"),
        (report.notified_p1, "P1 알림"),
        (report.merged_p1, "P1 묶음"),
        (report.dropped_p1, "P1 미발송"),
        (report.failed_p1, "P1 전달 실패"),
    ];
    for (count, label) in lines {
        if count > 0 {
            context.console().print(&format!("{label} {count}건"));
        }
    }
}

fn print_ran(context: &AppContext, report: &CollectReport) -> i32 {
    let console = context.console();
    let rows: Vec<_> = report.mode_outcomes.iter().chain(&report.outcomes).collect();
    let bad: Vec<_> = rows.iter().filter(|row| counts_as_failure(row.outcome, row.error_code.as_deref())).collect();
    let contract = if report.contract_test { " (계약 시험)" } else { "" };
    let slept = if report.slept { " (잠자기로 끊긴 틱)" } else { "" };
    console.print(&format!("수집 완료: {}건 기록, 실패 {}건{contract}{slept}", rows.len(), bad.len()));
    if let Some(mode) = &report.mode {
        console.print(&format!(
            "수집 모드: num_turns {}, 비용 {}, {}초",
            known(mode.num_turns),
            known(mode.cost_usd),
            seconds_fixed1(mode.elapsed_ms),
        ));
    }
    for row in bad {
        let code = row.error_code.as_deref().filter(|code| !code.is_empty());
        let code = code.map_or_else(String::new, |code| format!(" {code}"));
        console.print(&format!("  {} {}: {}{code}", row.source, row.query, row.outcome));
    }
    match &report.tool_results {
        None => console.print("큰 응답 파일 정리 실패"),
        Some(sweep) if sweep.removed + sweep.remaining > 0 => {
            console.print(&format!("큰 응답 파일: 지움 {}건, 남음 {}건", sweep.removed, sweep.remaining));
        }
        Some(_) => {}
    }
    print_counts(context, report);
    if report.halted {
        let reason = report.reason.as_deref().unwrap_or("null");
        console.print(&format!("{reason}. 다음 틱부터 수집을 멈춥니다."));
    }
    i32::from(report.halted)
}

pub struct Collect;

impl Command for Collect {
    fn name(&self) -> &'static str {
        "collect"
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&OPTIONS).parse(args)?;
        if parsed.flag("resume") {
            let cleared = HaltFlag::new(context.home(), context.clock_arc()).clear()?;
            context.console().print(if cleared {
                "수집 중단을 해제했습니다."
            } else {
                "수집 중단 상태가 아닙니다."
            });
            return Ok(0);
        }
        if let Some(value) = parsed.text("skip-slack-until") {
            return skip_slack(context, value);
        }
        if parsed.flag("reread-notion") {
            return reread_notion(context);
        }
        let report = context.tick_runner().run(parsed.flag("force"))?;
        if report.status == CollectStatus::Ran {
            return Ok(print_ran(context, &report));
        }
        context.console().print(&format!("수집 안 함: {}", report.reason.as_deref().unwrap_or("null")));
        Ok(i32::from(report.status == CollectStatus::Halted))
    }
}
