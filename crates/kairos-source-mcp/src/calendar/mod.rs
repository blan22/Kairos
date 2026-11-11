//! 설계 3.5. calendar 소스는 objects만 낸다. 새 회의, 시간 변경, 취소는 Ledger가 스냅샷을 비교해 만든다.
//! 삭제된 일정은 응답에서 빠질 뿐이다. ok 스냅샷 2회 연속 부재가 취소의 증거다(Ledger의 부재 집계).
//!
//! 응답(측정): {accessRole, summary, timeZone, events?, nextPageToken?}. 0건이면 events 키가 없다.
//! 종일 일정의 date는 `2026-11-05T00:00:00Z` 형식이다. 앞 10자만 쓴다.

mod dates;
mod objects;

use std::sync::Arc;

use kairos_core::connector::CALENDAR_EVENTS;
use kairos_core::instant::local_offset_ms;
use kairos_core::model::{
    CollectContext, McpCall, McpResponse, McpSource, Outcome, ParserFailed, QuerySpec, Source, SourceMode, SourceName,
    SourceResult,
};
use kairos_core::ports::Clock;
use kairos_core::profile;
use kairos_core::{Instant, LocalDate};
use serde_json::{Map, Value, json};

use crate::shared::{
    JsonObject, ParseFailure, RECORD_NULL, ResultFrame, call_id, has_next_token, parse_queries, read_pages,
    self_identity,
};
use objects::to_objects;

const SOURCE: SourceName = SourceName::Calendar;
const PAGE_SIZE: i64 = 250;
const HOUR_MS: i64 = 3_600_000;
const WINDOW_BACK_MS: i64 = HOUR_MS;
const WINDOW_AHEAD_DAYS: i64 = 14;
const OFF_WINDOW_AHEAD_DAYS: i64 = 60;

const PERSONAL: &str = "personal";
const DEPLOY: &str = "deploy";
const HOLIDAYS: &str = "holidays";
const CREW_OFF: &str = "crew_off";

/// 본인 이름. 연차 일정의 제목은 `<이름> 연차`다(측정). Notion 알림의 행위자 이름도 이것과 맞춘다.
pub fn self_name() -> &'static str {
    profile::current().identity.display_name.as_deref().unwrap_or(crate::shared::UNSET_IDENTITY)
}

/// 개인 캘린더 ID는 본인 Google 주소다.
pub(crate) fn personal_calendar() -> &'static str {
    self_identity(SOURCE)
}

/// 배포 캘린더 ID. 설정하지 않았으면 빈 값이다.
pub fn deploy_calendar() -> &'static str {
    profile::current().org.deploy_calendar.as_deref().unwrap_or_default()
}

pub(crate) fn deploy_tag() -> &'static str {
    &profile::current().org.deploy_tag
}

/// 질의가 읽는 캘린더. 설정하지 않은 캘린더의 질의는 만들지 않는다.
fn calendar_of(query: &str) -> Option<&'static str> {
    let profile = profile::current();
    match query {
        PERSONAL => profile.identity.google_email.as_deref(),
        DEPLOY => profile.org.deploy_calendar.as_deref(),
        HOLIDAYS => profile.org.holiday_calendar.as_deref(),
        _ => profile.org.leave_calendar.as_deref().filter(|_| profile.identity.display_name.is_some()),
    }
}

/// list_events는 오프셋 없는 현지 시각을 받는다(도구 설명).
fn local_date_time(at: Instant) -> String {
    let minutes = at.local_minutes();
    format!("{}T{:02}:{:02}:{:02}", at.local_date(), minutes / 60, minutes % 60, at.local_seconds())
}

/// 현지 자정의 오프셋 표기(`+09:00`).
fn offset_text(at: Instant) -> String {
    let offset = local_offset_ms(at.millis()) / 60_000;
    let sign = if offset < 0 { '-' } else { '+' };
    format!("{sign}{:02}:{:02}", offset.abs() / 60, offset.abs() % 60)
}

struct Window {
    from: String,
    to: String,
    from_instant: String,
    to_instant: String,
}

fn window_of(query: &str, now: Instant) -> Window {
    if query == PERSONAL || query == DEPLOY {
        let from = now.minus_ms(WINDOW_BACK_MS);
        let to = now.plus_ms(WINDOW_AHEAD_DAYS * 24 * HOUR_MS);
        return Window {
            from: local_date_time(from),
            to: local_date_time(to),
            from_instant: from.to_iso(),
            to_instant: to.to_iso(),
        };
    }
    // 쉬는 날은 오늘 자정부터 본다. 지나간 날은 근무일 판정에 쓰이지 않는다.
    let today: LocalDate = now.local_date();
    let end = today.add_days(OFF_WINDOW_AHEAD_DAYS);
    let from_offset = offset_text(Instant::at_local(today, 0));
    let to_offset = offset_text(Instant::at_local(end, 0));
    Window {
        from: format!("{today}T00:00:00"),
        to: format!("{end}T00:00:00"),
        from_instant: format!("{today}T00:00:00{from_offset}"),
        to_instant: format!("{end}T00:00:00{to_offset}"),
    }
}

/// 비어 있지 않은 문자열만 값으로 친다.
fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|text| !text.is_empty())
}

/// 일정 응답 한 쪽.
#[derive(Debug)]
struct CalendarPage {
    summary: Option<String>,
    access_role: String,
    events: Vec<Value>,
    has_more: bool,
}

fn parse_calendar_page(mut body: JsonObject) -> Result<CalendarPage, ParseFailure> {
    let shape = ParseFailure::new("calendar_shape");
    let access_role = body.get("accessRole").and_then(Value::as_str).ok_or(shape)?.to_owned();
    let events = match body.remove("events") {
        None => Vec::new(),
        Some(Value::Array(events)) => events,
        Some(_) => return Err(shape),
    };
    Ok(CalendarPage {
        summary: text(body.get("summary")).map(str::to_owned),
        access_role,
        events,
        has_more: has_next_token(&body),
    })
}

fn args_of(query: &str, now: Instant) -> Map<String, Value> {
    let window = window_of(query, now);
    let mut args = Map::new();
    args.insert("calendarId".to_owned(), json!(calendar_of(query).unwrap_or_default()));
    args.insert("startTime".to_owned(), json!(window.from));
    args.insert("endTime".to_owned(), json!(window.to));
    args.insert("pageSize".to_owned(), json!(PAGE_SIZE));
    if query == DEPLOY {
        args.insert("fullText".to_owned(), json!(deploy_tag()));
    }
    if query == CREW_OFF {
        args.insert("fullText".to_owned(), json!(self_name()));
    }
    args
}

/// 개인, 배포, 공휴일, 연차 캘린더를 읽는 소스.
pub struct CalendarSource {
    queries: Vec<QuerySpec>,
    clock: Arc<dyn Clock>,
}

impl CalendarSource {
    /// 설정한 캘린더의 질의만 만든다. 개인 캘린더(본인 주소)가 없으면 등록 단계에서 연결 없음이 된다.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        let queries = [
            QuerySpec::new(PERSONAL, 180, 0),
            QuerySpec::new(DEPLOY, 1800, 0),
            QuerySpec::new(HOLIDAYS, 86_400, 0),
            QuerySpec::new(CREW_OFF, 86_400, 0),
        ]
        .into_iter()
        .filter(|spec| calendar_of(&spec.id).is_some())
        .collect();
        Self { queries, clock }
    }

    fn parse_query(&self, spec: &QuerySpec, pages: &[&McpResponse], context: &CollectContext) -> SourceResult {
        let window = window_of(&spec.id, context.now);
        let frame = ResultFrame {
            source: SOURCE,
            spec,
            now: context.now,
            pages: pages.len() as i64,
            window_from: Some(window.from_instant),
            window_to: Some(window.to_instant),
            clock: self.clock.as_ref(),
        };
        let read = read_pages(pages, parse_calendar_page);
        let (parsed, failure) = (read.parsed, read.failure);
        if let (Some(failure), true) = (failure, parsed.is_empty()) {
            return frame.empty_result(failure.outcome, Some(failure.error_code));
        }

        // 설계 9.2: 내 캘린더의 소유자로 읽었는지 매번 단언한다. summary 불일치는 Ledger가 contract_break로 바꾼다.
        let is_personal = spec.id == PERSONAL;
        if is_personal && parsed.iter().any(|page| page.access_role != "owner") {
            return frame.empty_result(Outcome::ContractBreak, Some("calendar_access_role"));
        }
        if is_personal && parsed.iter().any(|page| page.summary.is_none()) {
            return frame.empty_result(Outcome::ContractBreak, Some("calendar_identity_missing"));
        }

        let listed: Vec<&Value> = parsed.iter().flat_map(|page| &page.events).collect();
        let (broken, events): (Vec<&Value>, Vec<&Value>) = listed.into_iter().partition(|event| event.is_null());
        let objects = match to_objects(&spec.id, &events) {
            Ok(objects) => objects,
            Err(failure) => return frame.empty_result(Outcome::ParseError, Some(failure.error_code())),
        };
        // 쪽 상한이나 저장된 큰 응답으로 다음 쪽을 못 읽었다. ok로 내면 못 읽은 일정이 스냅샷 부재가 되어 취소로 오판된다.
        // null 일정도 같다. 그 일정이 무엇이었는지 모르니 나머지만 쓰고 ok로 내지 않는다.
        let page_cap = parsed.last().is_some_and(|page| page.has_more).then_some("page_cap");
        let null_event = (!broken.is_empty()).then_some(RECORD_NULL);
        let unread = failure.map(|failure| failure.error_code).or(page_cap).or(null_event);
        SourceResult {
            parsed_count: (events.len() + broken.len()) as i64,
            identity: if is_personal { parsed.first().and_then(|page| page.summary.clone()) } else { None },
            watermark: unread.is_none().then(|| context.now.to_iso()),
            snapshot: true,
            objects,
            ..frame.empty_result(if unread.is_some() { Outcome::Partial } else { Outcome::Ok }, unread)
        }
    }
}

impl Source for CalendarSource {
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

impl McpSource for CalendarSource {
    fn plan(&self, context: &CollectContext) -> Vec<McpCall> {
        self.queries
            .iter()
            .map(|spec| McpCall {
                id: call_id(SOURCE, &spec.id),
                query: spec.id.clone(),
                server: CALENDAR_EVENTS.server().to_owned(),
                tool: CALENDAR_EVENTS.tool().to_owned(),
                args: args_of(&spec.id, context.now),
                expand: None,
            })
            .collect()
    }

    fn parse(&self, responses: &[McpResponse], context: &CollectContext) -> Result<Vec<SourceResult>, ParserFailed> {
        Ok(parse_queries(SOURCE, &self.queries, responses, |spec, pages| self.parse_query(spec, pages, context)))
    }
}
