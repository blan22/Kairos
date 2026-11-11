//! 설계 3.6. 발신자와 제목만 넘긴다. 분류는 규칙 엔진이 한다.
//! 응답은 {threads, resultCountEstimate, nextPageToken?}이고 0건은 `{}`다(측정).
//! resultCountEstimate는 추정치라 expected_count로 쓰지 않는다.
//! 메시지에 date가 오지 않는다(측정, 세 view 모두). 없으면 첫 관측 시각을 쓴다.
//!
//! 조회 창은 마지막 ok부터 지금까지를 덮는다. 수집기는 주말에 돌지 않아 금요일 밤에서 월요일 아침까지
//! 약 61시간이 비고, 인증이 끊긴 공백은 더 길 수 있다. 창의 상한을 넘은 공백은 window_capped로 남긴다.

use kairos_core::connector::MAIL_SEARCH;
use std::sync::{Arc, LazyLock};

use kairos_core::model::{
    ActorKind, CollectContext, EventType, McpCall, McpResponse, McpSource, NOTION_SENDER, NormalizedEvent,
    NormalizedObject, ObjectKind, Outcome, ParserFailed, QuerySpec, Source, SourceMode, SourceName, SourceResult,
    TrustTier,
};
use kairos_core::ports::Clock;
use kairos_core::{Instant, js};
use regex::Regex;
use serde_json::{Map, Value, json};

use crate::shared::{
    JsonObject, MailWindow, ParseFailure, RECORD_NULL, ResultFrame, angle_address, call_id, has_next_token,
    is_trusted_address, last_by_key, parse_queries, read_pages, self_identity, storable_instant,
};

pub use crate::shared::MAX_WINDOW_DAYS;

/// 메일 웹 주소만 링크로 받는다. 추적 링크를 저장하지 않기 위해서다.
pub(crate) const VIEW_URL_PREFIX: &str = "https://mail.google.com/";

const SOURCE: SourceName = SourceName::Gmail;
const QUERY: &str = "recent";
const PAGE_SIZE: i64 = 50;
/// 본인 메일 주소. Google 계정 주소(개인 캘린더 ID)와 같다.
fn self_address() -> &'static str {
    self_identity(SourceName::Calendar)
}
const KNOWN_EMPTY_KEYS: [&str; 2] = ["resultCountEstimate", "nextPageToken"];

static NO_REPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i-u:no[-_.]?reply)").expect("상수 정규식"));
static BARE_ADDRESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r#"^[^<>{ws}"]+@[^<>{ws}"]+$"#, ws = js::WS)).expect("상수 정규식"));

/// 발신자 글자에서 주소를 뽑아 소문자로 낸다. 문자열이 아니면 None이다.
pub fn sender_address(sender: Option<&Value>) -> Option<String> {
    let text = js::trim(sender?.as_str()?);
    let address = angle_address(text).or_else(|| BARE_ADDRESS.is_match(text).then_some(text))?;
    Some(address.to_lowercase())
}

/// 꺾쇠가 둘 이상이면 표시 이름이 주소를 흉내 낸 것이다. 어떤 주소도 신원으로 믿지 않는다.
pub(crate) fn has_forged_name(sender: Option<&Value>) -> bool {
    sender.and_then(Value::as_str).is_some_and(|sender| sender.matches('<').count() >= 2)
}

fn actor_kind_of(address: Option<&str>) -> ActorKind {
    match address {
        Some(address) if address == self_address().to_lowercase() => ActorKind::SelfActor,
        Some(address) if NO_REPLY.is_match(address) => ActorKind::Bot,
        _ => ActorKind::Human,
    }
}

fn trust_tier_of(address: Option<&str>, kind: ActorKind) -> TrustTier {
    match kind {
        ActorKind::SelfActor => TrustTier::T0,
        ActorKind::Bot => TrustTier::T2,
        ActorKind::Human if is_trusted_address(address) => TrustTier::T1,
        ActorKind::Human => TrustTier::T3,
    }
}

fn occurred_at(message: Option<&Value>, now: Instant) -> String {
    let date = message.and_then(|message| message.get("date")).and_then(Value::as_str).and_then(Instant::parse);
    date.and_then(|at| storable_instant(at.millis() as f64)).unwrap_or(now).to_iso()
}

/// view가 SUMMARY라 messages에는 가장 최근 메시지 하나만 온다.
fn to_records(thread: &Value, now: Instant) -> Result<(NormalizedObject, NormalizedEvent), ParseFailure> {
    let id = thread.get("id").and_then(Value::as_str).filter(|id| !id.is_empty());
    let id = id.ok_or(ParseFailure::new("gmail_thread_id"))?;
    let message = thread.get("messages").and_then(Value::as_array).and_then(|messages| messages.last());
    let sender = message.and_then(|message| message.get("sender"));
    let address = sender_address(sender);
    let trusted = if has_forged_name(sender) { None } else { address.as_deref() };
    let actor_kind = actor_kind_of(trusted);
    let trust_tier = trust_tier_of(trusted, actor_kind);
    let subject = message.and_then(|message| message.get("subject")).and_then(Value::as_str).map(str::to_owned);
    let view_url = thread.get("viewUrl").and_then(Value::as_str);
    let permalink = view_url.filter(|url| url.starts_with(VIEW_URL_PREFIX)).map(str::to_owned);
    let at = occurred_at(message, now);
    let object_key = format!("gmail:{id}");
    let object = NormalizedObject {
        actor_id: address.clone(),
        permalink: permalink.clone(),
        text_head: subject.clone(),
        ts: Some(at.clone()),
        trust_tier: Some(trust_tier),
        ..NormalizedObject::new(object_key.clone(), SOURCE, ObjectKind::MailThread, id)
    };
    let event = NormalizedEvent {
        actor_id: address,
        object_key: Some(object_key.clone()),
        permalink,
        text_head: subject,
        trust_tier: Some(trust_tier),
        ..NormalizedEvent::new(object_key, SOURCE, EventType::MailThread, at, actor_kind)
    };
    Ok((object, event))
}

/// 스레드 목록 응답 한 쪽.
#[derive(Debug)]
pub(crate) struct ThreadsPage {
    pub threads: Vec<Value>,
    pub has_more: bool,
}

pub(crate) fn parse_threads_page(mut body: JsonObject) -> Result<ThreadsPage, ParseFailure> {
    let has_more = has_next_token(&body);
    match body.remove("threads") {
        Some(Value::Array(threads)) => Ok(ThreadsPage { threads, has_more }),
        // 0건과 형식 깨짐을 구분한다. `{}`만 0건이다.
        None if body.keys().all(|key| KNOWN_EMPTY_KEYS.contains(&key.as_str())) => {
            Ok(ThreadsPage { threads: Vec::new(), has_more })
        }
        _ => Err(ParseFailure::new("gmail_shape")),
    }
}

/// 받은 메일 스레드를 읽는 소스.
pub struct GmailSource {
    queries: Vec<QuerySpec>,
    clock: Arc<dyn Clock>,
}

impl GmailSource {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self { queries: vec![QuerySpec::new(QUERY, 3600, 0)], clock }
    }

    fn parse_query(&self, spec: &QuerySpec, pages: &[&McpResponse], context: &CollectContext) -> SourceResult {
        let window = MailWindow::of(context, QUERY);
        let frame = ResultFrame {
            source: SOURCE,
            spec,
            now: context.now,
            pages: pages.len() as i64,
            window_from: Some(window.from.to_iso()),
            window_to: Some(context.now.to_iso()),
            clock: self.clock.as_ref(),
        };
        let read = read_pages(pages, parse_threads_page);
        let (parsed, failure) = (read.parsed, read.failure);
        if let (Some(failure), true) = (failure, parsed.is_empty()) {
            return frame.empty_result(failure.outcome, Some(failure.error_code));
        }

        // 페이지가 겹치면 같은 스레드가 다시 올 수 있다.
        let listed: Vec<&Value> = parsed.iter().flat_map(|page| &page.threads).collect();
        let (broken, listed): (Vec<&Value>, Vec<&Value>) = listed.into_iter().partition(|thread| thread.is_null());
        let threads = last_by_key(listed.into_iter().map(|thread| (thread.get("id").and_then(Value::as_str), thread)));
        let records: Result<Vec<_>, ParseFailure> =
            threads.into_iter().map(|thread| to_records(thread, context.now)).collect();
        let records = match records {
            Ok(records) => records,
            Err(failure) => return frame.empty_result(Outcome::ParseError, Some(failure.error_code())),
        };
        // 쪽 상한에 걸리면 창의 나머지를 못 읽었다. 정렬 순서를 모르므로 전진하지 않고 드러낸다.
        let page_cap = parsed.last().is_some_and(|page| page.has_more).then_some("page_cap");
        // null 스레드는 무엇이었는지 모른다. 나머지를 쓰되 전진하지 않아 다음 틱에 다시 읽는다.
        let null_thread = (!broken.is_empty()).then_some(RECORD_NULL);
        let unread = failure.map(|failure| failure.error_code).or(page_cap).or(null_thread);
        let notice = unread.or(window.capped.then_some("window_capped"));
        let parsed_count = records.len() as i64;
        let (objects, events) = records.into_iter().unzip();
        SourceResult {
            parsed_count,
            // 상한을 넘은 공백은 되돌릴 수 없다. 멈추면 공백만 커지므로 전진하고 window_capped로 고지한다.
            watermark: unread.is_none().then(|| context.now.to_iso()),
            objects,
            events,
            ..frame.empty_result(if unread.is_some() { Outcome::Partial } else { Outcome::Ok }, notice)
        }
    }
}

impl Source for GmailSource {
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

impl McpSource for GmailSource {
    fn plan(&self, context: &CollectContext) -> Vec<McpCall> {
        let days = MailWindow::of(context, QUERY).days;
        let mut args = Map::new();
        args.insert("query".to_owned(), json!(format!("newer_than:{days}d -from:{NOTION_SENDER}")));
        args.insert("pageSize".to_owned(), json!(PAGE_SIZE));
        args.insert("view".to_owned(), json!("THREAD_VIEW_SUMMARY"));
        vec![McpCall {
            id: call_id(SOURCE, QUERY),
            query: QUERY.to_owned(),
            server: MAIL_SEARCH.server().to_owned(),
            tool: MAIL_SEARCH.tool().to_owned(),
            args,
            expand: None,
        }]
    }

    fn parse(&self, responses: &[McpResponse], context: &CollectContext) -> Result<Vec<SourceResult>, ParserFailed> {
        Ok(parse_queries(SOURCE, &self.queries, responses, |spec, pages| self.parse_query(spec, pages, context)))
    }
}
