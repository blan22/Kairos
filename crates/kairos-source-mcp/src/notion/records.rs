//! 읽은 스레드를 객체와 이벤트로 바꾼다.

use kairos_core::model::{
    ActorKind, EventType, NOTION_SENDER, NormalizedEvent, NormalizedObject, ObjectKind, TrustTier,
};
use kairos_core::text::sanitize_head;
use kairos_core::{Instant, js};
use serde_json::Value;

use super::SOURCE;
use super::parser::{BlockKind, MailBlock, parse_mail};
use crate::calendar::self_name;
use crate::gmail::{VIEW_URL_PREFIX, has_forged_name, sender_address};
use crate::shared::{JsonObject, ParseFailure, storable_instant};

/// 측정 최대 12블록의 몇 배. 위조 메일 한 통이 항목을 쏟아내지 못하게 한다.
const MAX_BLOCKS: usize = 100;

fn event_type_of(kind: BlockKind) -> EventType {
    match kind {
        BlockKind::Mention => EventType::NotionMention,
        BlockKind::Comment => EventType::NotionComment,
        BlockKind::Edit => EventType::NotionEdit,
        BlockKind::Promo => EventType::NotionPromo,
        BlockKind::Unknown => EventType::NotionUnknown,
    }
}

fn actor_kind_of(block: &MailBlock) -> ActorKind {
    if block.kind == BlockKind::Promo {
        return ActorKind::Bot;
    }
    let alone = !block.others && !block.actors.is_empty();
    if alone && block.actors.iter().all(|actor| actor == self_name()) { ActorKind::SelfActor } else { ActorKind::Human }
}

/// `StringToNumber`. 공백을 지운 뒤 빈 글자는 0, 부호 없는 진법 표기와 십진 표기만 받는다.
fn number_of_text(text: &str) -> f64 {
    let text = js::trim(text);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = text.strip_prefix(prefix) {
            let parsed = digits.chars().map(|c| c.to_digit(radix)).collect::<Option<Vec<u32>>>();
            return match parsed {
                Some(values) if !values.is_empty() => {
                    values.into_iter().fold(0.0, |sum, value| sum * f64::from(radix) + f64::from(value))
                }
                _ => f64::NAN,
            };
        }
    }
    match text {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    // Rust는 `inf`와 `nan`을 받는다. JS의 십진 표기는 숫자로 시작하거나 부호, 점 다음에 숫자가 온다.
    let decimal = text.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.' | b'e' | b'E'));
    if decimal { text.parse().unwrap_or(f64::NAN) } else { f64::NAN }
}

/// `Number(value)`. 값이 없으면 NaN이다.
fn number_of(value: Option<&Value>) -> f64 {
    match value {
        None | Some(Value::Object(_)) => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => f64::from(u8::from(*flag)),
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => number_of_text(text),
        Some(Value::Array(items)) => match items.as_slice() {
            [] => 0.0,
            [Value::Bool(_)] => f64::NAN,
            [one] => number_of(Some(one)),
            _ => f64::NAN,
        },
    }
}

/// 읽을 수 없거나 저장할 수 없는 값이면 지금 시각으로 둔다.
fn occurred_at(message: &Value, now: Instant) -> String {
    let ms = match message.get("date").and_then(Value::as_str) {
        Some(date) => Instant::parse(date).map_or(f64::NAN, |at| at.millis() as f64),
        None => number_of(message.get("internalDate")),
    };
    storable_instant(ms).unwrap_or(now).to_iso()
}

fn body_text(message: &Value) -> Option<&str> {
    message.get("plaintextBody").and_then(Value::as_str)
}

fn mail_link(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).filter(|url| url.starts_with(VIEW_URL_PREFIX)).map(str::to_owned)
}

/// 읽은 스레드 하나의 기록.
pub(super) struct Records {
    pub object: NormalizedObject,
    pub events: Vec<NormalizedEvent>,
    /// 블록이 상한을 넘어 일부만 낸다.
    pub capped: bool,
}

pub(super) fn is_notion_sender(sender: Option<&Value>) -> bool {
    sender_address(sender).as_deref() == Some(NOTION_SENDER) && !has_forged_name(sender)
}

struct PlacedBlock {
    block: MailBlock,
    at: String,
    link: Option<String>,
    index: usize,
}

/// 앞 블록의 순번을 유지한 채 상한만큼 남긴다. 나를 부른 블록을 먼저 살린다.
fn kept_blocks(blocks: Vec<PlacedBlock>) -> Vec<PlacedBlock> {
    if blocks.len() <= MAX_BLOCKS {
        return blocks;
    }
    let (called_me, rest): (Vec<_>, Vec<_>) = blocks.into_iter().partition(|placed| placed.block.mentions_me);
    let mut kept: Vec<_> = called_me.into_iter().chain(rest).take(MAX_BLOCKS).collect();
    kept.sort_by_key(|placed| placed.index);
    kept
}

/// 스레드의 Notion 메시지를 이벤트로 바꾼다. Notion이 보낸 메시지가 없으면 None이다.
pub(super) fn to_records(thread_id: &str, thread: &JsonObject, now: Instant) -> Result<Option<Records>, ParseFailure> {
    let shape = ParseFailure::new("notion_thread_shape");
    if thread.get("id").and_then(Value::as_str) != Some(thread_id) {
        return Err(shape);
    }
    let messages = thread.get("messages").and_then(Value::as_array).ok_or(shape)?;
    let messages: Vec<&Value> = messages.iter().filter(|message| is_notion_sender(message.get("sender"))).collect();
    let Some(last) = messages.last() else {
        return Ok(None);
    };
    // 빈 본문을 읽은 것으로 치면 그 메일은 다시 읽히지 않는다.
    if messages.iter().any(|message| body_text(message).is_none_or(|body| js::trim(body).is_empty())) {
        return Err(ParseFailure::new("notion_body"));
    }

    let object_key = format!("notion-mail:{thread_id}");
    let permalink = mail_link(thread.get("viewUrl"));
    // 뒤에 붙은 메시지는 뒤 순번을 받으므로 앞 블록의 키가 바뀌지 않는다.
    let mut blocks = Vec::new();
    for message in &messages {
        let at = occurred_at(message, now);
        let link = mail_link(message.get("viewUrl")).or_else(|| permalink.clone());
        let subject = message.get("subject").and_then(Value::as_str).unwrap_or("");
        for block in parse_mail(subject, body_text(message).unwrap_or("")) {
            let index = blocks.len();
            blocks.push(PlacedBlock { block, at: at.clone(), link: link.clone(), index });
        }
    }
    let total = blocks.len();
    let kept = kept_blocks(blocks);
    let capped = kept.len() < total;
    let object = NormalizedObject {
        permalink,
        ts: Some(occurred_at(last, now)),
        trust_tier: Some(TrustTier::T2),
        ..NormalizedObject::new(object_key.clone(), SOURCE, ObjectKind::NotionMail, thread_id)
    };
    let events = kept.into_iter().map(|placed| to_event(&object_key, placed)).collect();
    Ok(Some(Records { object, events, capped }))
}

fn to_event(object_key: &str, placed: PlacedBlock) -> NormalizedEvent {
    let PlacedBlock { block, at, link, index } = placed;
    let actors = sanitize_head(&block.actors.join(", "));
    let actor_id = if block.kind == BlockKind::Promo {
        Some(NOTION_SENDER.to_owned())
    } else {
        Some(actors).filter(|actors| !actors.is_empty())
    };
    let dedupe_key = format!("{object_key}:{index}");
    let actor_kind = actor_kind_of(&block);
    NormalizedEvent {
        actor_id,
        object_key: Some(object_key.to_owned()),
        mentions_me: Some(block.mentions_me),
        permalink: link,
        text_head: block.title,
        trust_tier: Some(TrustTier::T2),
        ..NormalizedEvent::new(dedupe_key, SOURCE, event_type_of(block.kind), at, actor_kind)
    }
}
