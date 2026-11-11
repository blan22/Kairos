//! 검색 응답 한 쪽의 해석. 본문은 레코드 머리글을 흉내 낼 수 있어 순번으로 레코드를 가른다.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use kairos_core::{Instant, js};
use regex::{Captures, Regex};
use serde_json::Value;
use url::Url;

use crate::shared::{JsonObject, ParseFailure, angle_address, pattern, storable_instant};

/// 메시지 1건. 본문은 이벤트를 만드는 동안만 쓰고 저장하지 않는다.
#[derive(Clone, Debug, PartialEq)]
pub struct SlackRecord {
    pub channel_id: String,
    pub is_dm: bool,
    pub user_id: String,
    pub email: Option<String>,
    pub ts: String,
    /// `ts`의 순간. 범위를 넘는 ts의 레코드는 여기까지 오지 못한다.
    pub at: Instant,
    pub thread_ts: Option<String>,
    pub permalink: String,
    pub text: String,
}

/// 검색 응답 한 쪽.
#[derive(Clone, Debug, PartialEq)]
pub struct SlackPage {
    pub declared: i64,
    pub records: Vec<SlackRecord>,
    /// 읽지 못한 레코드 수. 남은 레코드는 그대로 쓰고 이 수는 record_quarantined로 드러난다.
    pub quarantined: i64,
    pub has_more: bool,
}

static MESSAGES_HEADER: LazyLock<Regex> = LazyLock::new(|| pattern(r"^## Messages \(([0-9]+) results?\)$"));
static RESULT_HEADER: LazyLock<Regex> = LazyLock::new(|| pattern(r"^### Result ([0-9]+) of ([0-9]+)$"));
static CHANNEL_LINE: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"^Channel: ({dot}*) \(ID: ([A-Z0-9]+)\)$", dot = js::DOT)));
static FROM_LINE: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"^From: ({dot}*)\(ID: ([A-Z0-9]+)\)[{ws}]*$", dot = js::DOT, ws = js::WS)));
static TS_LINE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^Message_ts: ([0-9]+\.[0-9]+)$"));
static PERMALINK_LINE: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"^Permalink: \[link\]\((https://[^){ws}]+)\)$", ws = js::WS)));
static NEXT_CURSOR: LazyLock<Regex> = LazyLock::new(|| pattern(r"use cursor `[^`]+`"));

/// `Number(digits)`. 건수와 순번을 같은 값으로 비교한다.
fn count(digits: &str) -> f64 {
    digits.parse().unwrap_or(f64::INFINITY)
}

/// 레코드 머리글은 다음 줄이 Channel:인 경우뿐이다. 그렇지 않은 줄은 본문이다.
fn record_start(lines: &[&str], index: usize) -> Option<(f64, f64)> {
    if !lines.get(index + 1)?.starts_with("Channel: ") {
        return None;
    }
    let header = RESULT_HEADER.captures(lines[index])?;
    Some((count(&header[1]), count(&header[2])))
}

struct RecordLayout {
    /// 진짜임을 확정한 레코드의 줄 범위. 끝은 포함하지 않는다.
    spans: Vec<(usize, usize)>,
    /// 진짜 머리글을 가릴 수 없는 순번의 수.
    ambiguous: usize,
}

/// 순번별 머리글 후보. 건수는 응답이 밝힌 값이라 그만큼 미리 잡지 않고, 비지 않았음을 확인한 뒤 펼친다.
fn candidates_of(lines: &[&str], declared: usize) -> Result<(Vec<Vec<usize>>, Vec<usize>), ParseFailure> {
    let mut found: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    let mut headers = Vec::new();
    for index in 0..lines.len() {
        let Some((position, total)) = record_start(lines, index) else { continue };
        if total != declared as f64 || position < 1.0 || position > declared as f64 {
            continue;
        }
        found.entry(position as usize - 1).or_default().push(index);
        headers.push(index);
    }
    let gap = found.keys().enumerate().find(|(rank, slot)| rank != *slot).map(|(rank, _)| rank);
    if let Some(missing) = gap.or((found.len() < declared).then_some(found.len())) {
        let later_found = found.keys().next_back().is_some_and(|last| *last > missing);
        return Err(ParseFailure::new(if later_found { "result_sequence" } else { "count_mismatch" }));
    }
    Ok((found.into_values().collect(), headers))
}

/// 헤더 순번으로 자른다. 본문은 머리글을 흉내 낼 수 있고, 진짜 머리글을 지우지는 못한다.
/// - 이 쪽의 건수와 맞지 않는 머리글은 본문이다.
/// - 순번 k의 진짜 머리글은 k-1의 가장 이른 후보 뒤, k+1의 가장 늦은 후보 앞에 있다. 1번은 어떤 본문보다 앞이다.
/// - 이 범위에 후보가 둘 이상이면 위조와 진짜를 가릴 수 없다. 그 순번만 격리한다.
///
/// 한 메시지 때문에 쪽 전체를 멈추면 watermark가 고정되어 같은 메시지를 영원히 다시 읽는다.
/// 진짜 머리글이 빠진 경우만 형식이 깨진 것이라 parse_error다.
fn record_layout(lines: &[&str], declared: usize) -> Result<RecordLayout, ParseFailure> {
    let sequence = ParseFailure::new("result_sequence");
    let (candidates, headers) = candidates_of(lines, declared)?;
    if declared > 0 && candidates[0].first() != headers.first() {
        return Err(sequence);
    }

    let mut earliest: Vec<usize> = Vec::with_capacity(declared);
    for slot in &candidates {
        let floor = earliest.last().copied();
        let found = slot.iter().copied().find(|line| floor.is_none_or(|floor| *line > floor));
        earliest.push(found.ok_or(sequence)?);
    }
    let mut latest = earliest.clone();
    let mut ceiling = lines.len();
    for index in (1..declared).rev() {
        let found = candidates[index].iter().copied().rfind(|line| *line < ceiling).ok_or(sequence)?;
        latest[index] = found;
        ceiling = found;
    }

    let spans: Vec<(usize, usize)> = earliest
        .iter()
        .zip(&latest)
        .filter(|(start, last)| start == last)
        .map(|(start, _)| {
            let next = headers.partition_point(|line| line <= start);
            (*start, headers.get(next).copied().unwrap_or(lines.len()))
        })
        .collect();
    let ambiguous = declared - spans.len();
    Ok(RecordLayout { spans, ambiguous })
}

/// 표시 이름은 작성자가 정한다. 꺾쇠가 하나일 때 그 끝의 주소만 쓰고, 그 밖에는 주소가 없는 것으로 본다.
fn sender_email(sender: &str) -> Option<String> {
    (sender.matches('<').count() == 1).then(|| angle_address(sender)).flatten().map(str::to_owned)
}

fn field_line<'a>(fields: &[&'a str], pattern: &Regex) -> Result<Captures<'a>, ParseFailure> {
    fields.iter().copied().find_map(|line| pattern.captures(line)).ok_or(ParseFailure::new("record_field"))
}

/// 메시지 ts를 순간으로 바꾼다. 표현할 수 없는 값이면 그 메시지만 격리한다.
fn instant_of(ts: &str) -> Result<Instant, ParseFailure> {
    storable_instant((count(ts) * 1000.0).round()).ok_or(ParseFailure::new("record_field"))
}

fn parse_record(lines: &[&str]) -> Result<SlackRecord, ParseFailure> {
    let text_index = lines.iter().position(|line| line.starts_with("Text:"));
    let text_index = text_index.ok_or(ParseFailure::new("record_field"))?;
    let fields = &lines[..text_index];

    let channel = field_line(fields, &CHANNEL_LINE)?;
    let sender = field_line(fields, &FROM_LINE)?;
    let ts = field_line(fields, &TS_LINE)?;
    let permalink = field_line(fields, &PERMALINK_LINE)?;
    let (label, channel_id) = (&channel[1], &channel[2]);
    let (ts, permalink) = (&ts[1], &permalink[1]);

    let mismatch = ParseFailure::new("permalink_mismatch");
    // 호스트와 경로는 브라우저 규칙으로 정규화한 값을 본다.
    let url = Url::parse(permalink).map_err(|_| mismatch)?;
    let expected_path = format!("/archives/{channel_id}/p{}", ts.replacen('.', "", 1));
    if !url.host_str().unwrap_or("").ends_with(".slack.com") || url.path() != expected_path {
        return Err(mismatch);
    }

    let first = js::trim(&lines[text_index]["Text:".len()..]);
    let mut body: Vec<&str> = std::iter::once(first).chain(lines[text_index + 1..].iter().copied()).collect();
    while body.last().is_some_and(|line| matches!(js::trim(line), "" | "---")) {
        body.pop();
    }

    Ok(SlackRecord {
        channel_id: channel_id.to_owned(),
        is_dm: label == "DM" || label == "Group DM" || channel_id.starts_with('D'),
        user_id: sender[2].to_owned(),
        email: sender_email(&sender[1]),
        ts: ts.to_owned(),
        at: instant_of(ts)?,
        thread_ts: url.query_pairs().find(|(key, _)| key == "thread_ts").map(|(_, value)| value.into_owned()),
        permalink: permalink.to_owned(),
        text: js::trim(&body.join("\n")).to_owned(),
    })
}

/// 응답 본문 하나를 읽는다. 형식이 어긋나면 실패다. 읽지 못한 메시지는 쪽을 멈추지 않고 격리한다.
pub fn parse_search_page(body: &JsonObject) -> Result<SlackPage, ParseFailure> {
    let shape = ParseFailure::new("slack_shape");
    let results = body.get("results").and_then(Value::as_str).ok_or(shape)?;
    let pagination = body.get("pagination_info").and_then(Value::as_str).ok_or(shape)?;
    let has_more = NEXT_CURSOR.is_match(pagination);
    if !has_more && !pagination.contains("End of results") {
        return Err(ParseFailure::new("slack_pagination"));
    }

    let lines: Vec<&str> = results.split('\n').collect();
    if !lines[0].starts_with("# Search Results for:") {
        return Err(shape);
    }
    let header = lines.iter().skip(1).find(|line| !js::trim(line).is_empty()).copied().unwrap_or("");
    if header == "No results found." {
        return Ok(SlackPage { declared: 0, records: Vec::new(), quarantined: 0, has_more: false });
    }

    let declared = MESSAGES_HEADER.captures(header).ok_or(shape)?;
    let total = count(&declared[1]);
    let layout = record_layout(&lines, total as usize)?;
    let mut records = Vec::with_capacity(layout.spans.len());
    let mut quarantined = layout.ambiguous;
    let mut first_failure = None;
    for (start, end) in layout.spans {
        match parse_record(&lines[start + 1..end]) {
            Ok(record) => records.push(record),
            Err(failure) => {
                first_failure.get_or_insert(failure);
                quarantined += 1;
            }
        }
    }
    // 살아남은 레코드가 없으면 메시지 하나가 아니라 형식이 바뀐 것일 수 있다. 전진하지 않는다.
    if let (Some(failure), true) = (first_failure, records.is_empty()) {
        return Err(failure);
    }
    Ok(SlackPage { declared: total as i64, records, quarantined: quarantined as i64, has_more })
}
