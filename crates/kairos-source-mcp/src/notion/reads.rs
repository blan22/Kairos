//! 검색 결과에서 읽을 스레드를 고르고, 후속 읽기의 결과를 모은다.

use std::collections::{HashMap, HashSet};

use kairos_core::Instant;
use kairos_core::model::{
    CollectContext, McpCall, McpExpand, McpResponse, NOTION_SENDER, Outcome, SkipCounts, ThreadReadOutcome,
    ThreadReadReport, ThreadReadState,
};
use serde_json::{Map, Value, json};

use super::QUERY;
use super::records::{Records, is_notion_sender, to_records};
use crate::gmail::{parse_threads_page, sender_address};
use crate::shared::{
    Failure, MAX_WINDOW_DAYS, ParseFailure, RECORD_NULL, TargetRead, body_of, ceil_days, failure_of, pages_of,
    read_pages, reads_of, scan_start,
};
use kairos_core::connector::{MAIL_SEARCH, MAIL_THREAD};

/// 밀린 메일을 읽는 호출의 표시. 응답 id가 쪽(#)이나 대상(!)과 섞이지 않는다.
pub(super) const BACKLOG_MARK: &str = "~old";
/// 매 틱 읽는 최근 구간.
pub(super) const FRESH_DAYS: i64 = 2;

const PAGE_SIZE: i64 = 50;
const MAX_READS: i64 = 12;
/// 밀린 메일 한 번의 폭. 앞 조각과 하루 겹치고 틱마다 하루씩 전진한다.
const BACKLOG_SPAN_DAYS: i64 = 2;
const WINDOW_MARGIN_MS: i64 = 3_600_000;
/// 이만큼 실패하고 이 시간이 지나면 격리한다. 도구 전체의 짧은 장애로 메일을 격리하지 않으려고 시간도 본다.
const QUARANTINE_FAILURES: i64 = 5;
const QUARANTINE_AFTER_MS: i64 = 3_600_000;

/// 오래된 쪽부터 읽을 조각.
#[derive(Clone, Copy, Debug)]
pub(super) struct Backlog {
    pub newer_days: i64,
    pub older_days: i64,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Scan {
    pub capped: bool,
    /// 없으면 최근 구간만으로 창을 덮는다.
    pub backlog: Option<Backlog>,
}

pub(super) fn scan_of(context: &CollectContext) -> Scan {
    let needed =
        scan_start(context, QUERY).map_or(0, |start| ceil_days(context.now.ms_since(start) + WINDOW_MARGIN_MS));
    if needed <= FRESH_DAYS {
        return Scan { capped: false, backlog: None };
    }
    let newer_days = needed.min(MAX_WINDOW_DAYS);
    let older_days = (newer_days - BACKLOG_SPAN_DAYS).max(1);
    Scan { capped: needed > MAX_WINDOW_DAYS, backlog: Some(Backlog { newer_days, older_days }) }
}

pub(super) fn search_call(id: String, query: &str, context: &CollectContext) -> McpCall {
    let reads = context.thread_reads.as_ref().and_then(|reads| reads.get(QUERY));
    let reads = reads.map_or(&[][..], Vec::as_slice);
    let skip: SkipCounts =
        reads.iter().filter_map(|read| Some((read.external_id.clone(), read.message_count?))).collect();
    let retry_ids = reads
        .iter()
        .filter(|read| read.failures > 0 && read.message_count.is_none())
        .map(|read| read.external_id.clone())
        .collect();
    let mut args = Map::new();
    args.insert("query".to_owned(), json!(format!("from:{NOTION_SENDER} {query}")));
    args.insert("pageSize".to_owned(), json!(PAGE_SIZE));
    args.insert("view".to_owned(), json!("THREAD_VIEW_METADATA_ONLY"));
    McpCall {
        id,
        query: QUERY.to_owned(),
        server: MAIL_SEARCH.server().to_owned(),
        tool: MAIL_SEARCH.tool().to_owned(),
        args,
        expand: Some(McpExpand { tool: MAIL_THREAD.tool().to_owned(), max_reads: MAX_READS, skip, retry_ids }),
    }
}

/// 메시지 수는 `typeof === 'number'`일 때만 값이다.
fn message_count_of(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

/// 검색이 보여 준 발신자로 스레드를 가른다. 목록이 잘렸으면 Notion 메시지가 가려져 있을 수 있어 읽는다.
fn is_other_sender(thread: &Value) -> bool {
    let messages = thread.get("messages").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    if messages.iter().any(|message| is_notion_sender(message.get("sender"))) {
        return false;
    }
    if messages.is_empty() || messages.iter().any(|message| sender_address(message.get("sender")).is_none()) {
        return false;
    }
    !message_count_of(thread.get("messageCount")).is_some_and(|count| count > messages.len() as f64)
}

fn is_settled(thread: &Value, state: Option<&ThreadReadState>) -> bool {
    let Some(stored) = state.and_then(|state| state.message_count) else {
        return false;
    };
    !message_count_of(thread.get("messageCount")).is_some_and(|count| count > stored as f64)
}

/// 두 검색이 함께 쓰는 관측.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Seen {
    pub other: bool,
    pub quarantined: bool,
}

/// 검색 하나의 결과.
pub(super) struct Lane<'a> {
    pub responded: bool,
    pub failure: Option<Failure>,
    pub pages_read: usize,
    has_more: bool,
    /// null 스레드가 끼어 있었다. 무엇이었는지 몰라 이 검색을 다 읽은 것으로 치지 않는다.
    broken: bool,
    /// 읽어야 하는데 아직 처리하지 못한 스레드.
    pub pending: HashSet<String>,
    pub listed: HashMap<String, Value>,
    pub reads: Vec<TargetRead<'a>>,
}

pub(super) fn scan_lane<'a>(
    id: &str,
    responses: &'a [McpResponse],
    states: &HashMap<&str, &ThreadReadState>,
    seen: &mut Seen,
) -> Result<Lane<'a>, ParseFailure> {
    let pages = pages_of(responses, id);
    let read = read_pages(&pages, parse_threads_page);
    let mut lane = Lane {
        responded: !pages.is_empty(),
        failure: read.failure,
        pages_read: read.parsed.len(),
        has_more: read.parsed.last().is_some_and(|page| page.has_more),
        broken: false,
        pending: HashSet::new(),
        listed: HashMap::new(),
        reads: reads_of(responses, id),
    };
    for thread in read.parsed.into_iter().flat_map(|page| page.threads) {
        if thread.is_null() {
            lane.broken = true;
            continue;
        }
        let thread_id = thread.get("id").and_then(Value::as_str).filter(|id| !id.is_empty());
        let thread_id = thread_id.ok_or(ParseFailure::new("gmail_thread_id"))?.to_owned();
        if is_other_sender(&thread) {
            seen.other = true;
            continue;
        }
        let state = states.get(thread_id.as_str()).copied();
        if is_settled(&thread, state) {
            seen.quarantined |= state.is_some_and(|state| state.quarantined_at.is_some());
        } else {
            lane.pending.insert(thread_id.clone());
        }
        lane.listed.insert(thread_id, thread);
    }
    Ok(lane)
}

/// 잠깐 막힌 것은 스레드의 문제가 아니다. 시작도 못 한 호출과 인증, 한도는 실패로 세지 않는다.
fn is_thread_fault(failure: Failure) -> bool {
    !matches!(failure.outcome, Outcome::Auth | Outcome::RateLimited) && failure.error_code != "mode_deadline"
}

/// 후속 읽기의 집계.
pub(super) struct ReadTally {
    pub records: Vec<Records>,
    pub reports: Vec<ThreadReadReport>,
    pub resolved: HashSet<String>,
    read_error: Option<&'static str>,
    sender_broken: bool,
    block_cap: bool,
    quarantined: bool,
    other: bool,
}

impl ReadTally {
    pub fn new(seen: Seen) -> Self {
        Self {
            records: Vec::new(),
            reports: Vec::new(),
            resolved: HashSet::new(),
            read_error: None,
            sender_broken: false,
            block_cap: false,
            quarantined: seen.quarantined,
            other: seen.other,
        }
    }

    /// 이 검색에서 못 읽은 까닭. 다 읽었으면 None이다.
    pub fn unread_of(&self, lane: &Lane<'_>) -> Option<&'static str> {
        let reason = lane.failure.map(|failure| failure.error_code).or(lane.has_more.then_some("page_cap"));
        let reason = reason.or(lane.broken.then_some(RECORD_NULL));
        reason.or_else(|| {
            if lane.pending.is_empty() {
                None
            } else if self.sender_broken {
                Some("notion_sender_mismatch")
            } else {
                Some(self.read_error.unwrap_or("notion_read_cap"))
            }
        })
    }

    /// 전진은 하되 알려야 하는 일.
    pub fn notice(&self) -> Option<&'static str> {
        if self.quarantined {
            Some("notion_quarantined")
        } else if self.block_cap {
            Some("notion_block_cap")
        } else {
            self.other.then_some("notion_other_sender")
        }
    }

    fn record_failure(&mut self, id: &str, listed: Option<&Value>, state: Option<&ThreadReadState>, now: Instant) {
        let failures = state.map_or(0, |state| state.failures) + 1;
        let since = match state.and_then(|state| state.first_failed_at.as_deref()) {
            Some(first) => Instant::parse(first),
            None => Some(now),
        };
        let young = since.is_some_and(|since| now.ms_since(since) < QUARANTINE_AFTER_MS);
        if failures < QUARANTINE_FAILURES || young {
            self.reports.push(ThreadReadReport {
                external_id: id.to_owned(),
                outcome: ThreadReadOutcome::Failed,
                message_count: None,
            });
            return;
        }
        // 메시지가 더 붙으면 그때 다시 읽는다.
        let count =
            message_count_of(listed.and_then(|thread| thread.get("messageCount"))).map_or(1, |count| count as i64);
        self.reports.push(ThreadReadReport {
            external_id: id.to_owned(),
            outcome: ThreadReadOutcome::Quarantined,
            message_count: Some(count),
        });
        self.resolved.insert(id.to_owned());
        self.quarantined = true;
    }

    fn read_thread(
        &mut self,
        id: &str,
        listed: Option<&Value>,
        response: &McpResponse,
        state: Option<&ThreadReadState>,
        now: Instant,
    ) -> Result<(), ParseFailure> {
        let read = body_of(response)?;
        let records = to_records(id, &read, now)?;
        let messages = read.get("messages").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        if records.is_none() && messages.iter().any(|message| sender_address(message.get("sender")).is_none()) {
            self.sender_broken = true;
            self.record_failure(id, listed, state, now);
            return Ok(());
        }
        self.resolved.insert(id.to_owned());
        let count = message_count_of(read.get("messageCount")).map_or(messages.len() as i64, |count| count as i64);
        self.reports.push(ThreadReadReport {
            external_id: id.to_owned(),
            outcome: ThreadReadOutcome::Read,
            message_count: Some(count),
        });
        match records {
            None => self.other = true,
            Some(records) => {
                self.block_cap |= records.capped;
                self.records.push(records);
            }
        }
        Ok(())
    }
}

/// 후속 읽기 응답 하나를 집계에 넣는다. 스레드 하나의 해석 실패가 다른 스레드를 막지 않는다.
pub(super) fn apply_read(
    tally: &mut ReadTally,
    id: &str,
    listed: Option<&Value>,
    response: &McpResponse,
    state: Option<&ThreadReadState>,
    now: Instant,
) {
    if response.is_error {
        let failure = failure_of(response);
        tally.read_error.get_or_insert(failure.error_code);
        if is_thread_fault(failure) {
            tally.record_failure(id, listed, state, now);
        }
        return;
    }
    if let Err(failure) = tally.read_thread(id, listed, response, state, now) {
        tally.read_error.get_or_insert(failure.error_code());
        tally.record_failure(id, listed, state, now);
    }
}
