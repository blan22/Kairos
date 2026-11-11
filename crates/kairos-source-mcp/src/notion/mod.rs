//! 설계 13절. Notion MCP에는 알림함이 없어 Gmail의 Notion 알림 메일을 읽는다.
//! - 1단계 search_threads: 새 스레드 ID를 찾는다. 제목으로는 멘션을 구분할 수 없다(측정).
//! - 2단계 get_thread: 모드가 같은 틱의 검색 결과로만 부른다(`McpCall::expand`).
//!
//! 본문은 이 모듈 안에서 파싱하고 버린다. 저장은 페이지 제목, 행위자, 종류, mentions_me, 스레드 ID, 시각, 링크다.
//! 메일 제목의 내용과 추적 링크(mg.mail.notion.com), 댓글 내용은 저장하지 않는다.
//!
//! 한 메일이 여러 페이지를 묶으므로 페이지 블록마다 이벤트 1건을 낸다. 메일은 바뀌지 않아 순번이 안정적이다.
//! 읽지 못한 스레드는 저장되지 않고 다음 틱에 다시 읽힌다. 그동안 결과는 partial이고 watermark는 멈춘다.
//! 같은 스레드가 한 시간 넘게 실패하면 격리해 고지하고, 나머지가 멈추지 않게 한다.
//!
//! 검색은 둘이다. 최근 이틀은 매 틱 읽어 새 멘션이 밀린 메일 뒤로 가지 않는다. 그보다 오래된 공백은 오래된 쪽부터
//! 이틀씩 잘라 읽고 progress_at을 옮긴다. 창 전체를 한 번에 읽으면 쪽 수가 쌓여 마감 안에 읽기를 시작하지 못한다.
//! Gmail 검색의 시각 표현은 newer_than과 older_than만 정확하다(측정. epoch는 0건, 날짜는 시간대가 어긋난다).

mod parser;
mod reads;
mod records;
mod rows;

use std::collections::HashMap;
use std::sync::Arc;

use kairos_core::model::{
    CollectContext, McpCall, McpResponse, McpSource, Outcome, ParserFailed, QuerySpec, Source, SourceMode, SourceName,
    SourceResult, ThreadReadState,
};
use kairos_core::ports::Clock;

use crate::shared::{DAY_MS, ResultFrame, call_id, pages_of, scan_start};
pub use parser::{BlockKind, MailBlock, parse_mail};
use reads::{BACKLOG_MARK, FRESH_DAYS, Lane, ReadTally, Seen, apply_read, scan_lane, scan_of, search_call};

const SOURCE: SourceName = SourceName::Notion;
const QUERY: &str = "mail";

fn states_of(context: &CollectContext) -> HashMap<&str, &ThreadReadState> {
    let reads = context.thread_reads.as_ref().and_then(|reads| reads.get(QUERY));
    reads.map_or(&[][..], Vec::as_slice).iter().map(|state| (state.external_id.as_str(), state)).collect()
}

/// Gmail의 Notion 알림 메일을 블록 단위로 읽는 소스.
pub struct NotionSource {
    queries: Vec<QuerySpec>,
    clock: Arc<dyn Clock>,
}

impl NotionSource {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { queries: vec![QuerySpec::new(QUERY, 900, 0)], clock }
    }

    fn parse_query(&self, spec: &QuerySpec, responses: &[McpResponse], context: &CollectContext) -> SourceResult {
        let scan = scan_of(context);
        let fresh_id = call_id(SOURCE, QUERY);
        let states = states_of(context);
        let window_from = scan_start(context, QUERY).unwrap_or_else(|| context.now.minus_ms(FRESH_DAYS * DAY_MS));
        let frame = ResultFrame {
            source: SOURCE,
            spec,
            now: context.now,
            pages: responses.len() as i64,
            window_from: Some(window_from.to_iso()),
            window_to: Some(context.now.to_iso()),
            clock: self.clock.as_ref(),
        };

        let mut seen = Seen::default();
        let lanes = scan_lane(&fresh_id, responses, &states, &mut seen).and_then(|fresh| {
            let backlog_id = format!("{fresh_id}{BACKLOG_MARK}");
            let old = scan.backlog.map(|_| scan_lane(&backlog_id, responses, &states, &mut seen)).transpose()?;
            Ok((fresh, old))
        });
        let (mut fresh, mut old): (Lane<'_>, Option<Lane<'_>>) = match lanes {
            Ok(lanes) => lanes,
            Err(failure) => return frame.empty_result(Outcome::ParseError, Some(failure.error_code())),
        };
        if let (Some(failure), 0) = (fresh.failure, fresh.pages_read) {
            return frame.empty_result(failure.outcome, Some(failure.error_code));
        }

        let mut tally = ReadTally::new(seen);
        for lane in std::iter::once(&fresh).chain(&old) {
            for read in &lane.reads {
                if lane.pending.contains(read.target_id) {
                    let listed = lane.listed.get(read.target_id);
                    let state = states.get(read.target_id).copied();
                    apply_read(&mut tally, read.target_id, listed, read.response, state, context.now);
                }
            }
        }
        for lane in std::iter::once(&mut fresh).chain(&mut old) {
            lane.pending.retain(|id| !tally.resolved.contains(id));
        }

        // 조각을 다 읽어야 읽은 표시를 옮긴다. 정렬 순서를 모르므로 일부만 읽고는 전진하지 않는다.
        let old_responded = old.as_ref().filter(|lane| lane.responded);
        let slice_done = old_responded.is_some_and(|lane| tally.unread_of(lane).is_none());
        let reached_fresh = match scan.backlog {
            None => true,
            Some(backlog) => slice_done && backlog.older_days < FRESH_DAYS,
        };
        let unread = tally
            .unread_of(&fresh)
            .or_else(|| old_responded.and_then(|lane| tally.unread_of(lane)))
            .or((!reached_fresh).then_some("notion_backlog"));
        let notice = unread.or(tally.notice());
        let error_code = if scan.capped { Some("window_capped") } else { notice };
        let progress_at = scan
            .backlog
            .filter(|_| slice_done)
            .map(|backlog| context.now.minus_ms(backlog.older_days * DAY_MS).to_iso());
        let (objects, events): (Vec<_>, Vec<_>) =
            tally.records.into_iter().map(|records| (records.object, records.events)).unzip();
        let events: Vec<_> = events.into_iter().flatten().collect();
        SourceResult {
            parsed_count: events.len() as i64,
            watermark: unread.is_none().then(|| context.now.to_iso()),
            progress_at,
            thread_reads: tally.reports,
            objects,
            events,
            ..frame.empty_result(if unread.is_some() { Outcome::Partial } else { Outcome::Ok }, error_code)
        }
    }
}

impl Source for NotionSource {
    fn name(&self) -> SourceName {
        SOURCE
    }

    fn queries(&self) -> &[QuerySpec] {
        &self.queries
    }

    fn mode(&self) -> SourceMode<'_> {
        SourceMode::Mcp(self)
    }
}

impl McpSource for NotionSource {
    fn needs_thread_reads(&self) -> bool {
        true
    }

    fn plan(&self, context: &CollectContext) -> Vec<McpCall> {
        let id = call_id(SOURCE, QUERY);
        let mut calls = vec![search_call(id.clone(), &format!("newer_than:{FRESH_DAYS}d"), context)];
        if let Some(backlog) = scan_of(context).backlog {
            let query = format!("newer_than:{}d older_than:{}d", backlog.newer_days, backlog.older_days);
            calls.push(search_call(format!("{id}{BACKLOG_MARK}"), &query, context));
        }
        calls
    }

    fn parse(&self, responses: &[McpResponse], context: &CollectContext) -> Result<Vec<SourceResult>, ParserFailed> {
        match self.queries.first() {
            Some(spec) if !pages_of(responses, &call_id(SOURCE, QUERY)).is_empty() => {
                Ok(vec![self.parse_query(spec, responses, context)])
            }
            _ => Ok(Vec::new()),
        }
    }
}
