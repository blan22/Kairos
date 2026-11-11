//! 수집 모드가 부르는 명령. 어느 쪽도 외부 문자열을 출력하지 않는다. 호출 인자와 건수, 코드만 나간다.

use kairos_core::KairosResult;
use kairos_core::js;
use kairos_core::model::{Source, SourceMode};
use kairos_ledger::OffDateBook;
use kairos_ops::ToolResults;
use kairos_tick::{IngestPayload, ResponseIngestor, TickPlanner, page_info_of};
use serde_json::Value;

use crate::app::AppContext;
use crate::command::Command;

const CONTRACT_TEST: &str = "KAIROS_CONTRACT_TEST";

fn collector_results(context: &AppContext) -> ToolResults {
    ToolResults::for_collector(context.home(), context.user_home())
}

pub struct PlanTick;

impl Command for PlanTick {
    fn name(&self) -> &'static str {
        "plan-tick"
    }

    fn execute(&self, context: &AppContext, _args: &[String]) -> KairosResult<i32> {
        let database = context.open_db()?;
        let extra = context.control().off_date_set();
        let off_dates = OffDateBook::new(database.connection()).load_watch_off_dates(&extra)?;
        // 계약 시험(설계 9.5)은 주기와 무관하게 질의마다 호출 1회를 확인한다.
        let force = context.env(CONTRACT_TEST) == Some("1");
        let plan =
            TickPlanner::new(database.connection()).plan_tick(&context.sources(), context.now(), &off_dates, force)?;
        context.console().emit(&plan)?;
        Ok(0)
    }
}

pub struct PageInfo;

impl Command for PageInfo {
    fn name(&self) -> &'static str {
        "page-info"
    }

    fn execute(&self, context: &AppContext, _args: &[String]) -> KairosResult<i32> {
        let Ok(body) = js::parse_json(&context.console().read_input()?) else {
            context.console().error("kairos page-info 실패: stdin이 JSON이 아닙니다");
            return Ok(2);
        };
        let content = body.get("content").unwrap_or(&Value::Null);
        let text = collector_results(context).peek_response(content)?;
        context.console().emit(&page_info_of(text.as_deref()))?;
        Ok(0)
    }
}

pub struct Ingest;

impl Ingest {
    fn refuse(context: &AppContext, message: &str) -> KairosResult<i32> {
        context.console().error(&format!("kairos ingest 실패: {message}"));
        Ok(2)
    }

    fn mcp_source(context: &AppContext, payload: &IngestPayload) -> Option<std::sync::Arc<dyn Source>> {
        let source = context.registry().find(payload.source)?;
        matches!(source.mode(), SourceMode::Mcp(_)).then(|| source.clone())
    }
}

impl Command for Ingest {
    fn name(&self) -> &'static str {
        "ingest"
    }

    fn execute(&self, context: &AppContext, _args: &[String]) -> KairosResult<i32> {
        let Ok(body) = js::parse_json(&context.console().read_input()?) else {
            return Self::refuse(context, "stdin이 JSON이 아닙니다");
        };
        let Some(payload) = IngestPayload::from_value(&body) else {
            return Self::refuse(context, "입력 형식이 맞지 않습니다");
        };
        let Some(source) = Self::mcp_source(context, &payload) else {
            return Self::refuse(context, "MCP 소스가 아닙니다");
        };
        let database = context.open_db()?;
        // 파서는 원문만 본다. 저장된 큰 응답은 여기서 되찾고 파일을 지운다.
        let responses = collector_results(context).recover_responses(payload.responses)?.responses;
        let payload = IngestPayload { responses, ..payload };
        let off_dates = context.control().off_date_set();
        let ingestor = ResponseIngestor::new(&database, context.clock_arc());
        let summary = ingestor.ingest_responses(source.as_ref(), &payload, &off_dates)?;
        context.console().emit(&summary)?;
        Ok(0)
    }
}
