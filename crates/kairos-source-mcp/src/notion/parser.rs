//! 메일 한 통을 페이지 블록으로 나눈다.

use std::sync::LazyLock;

use fancy_regex::{Regex as FancyRegex, RegexBuilder};
use kairos_core::js;
use regex::Regex;
use unicode_normalization::UnicodeNormalization;

use super::rows::{Row, to_row, without_links, without_tracking};
use crate::calendar::self_name;
use crate::shared::pattern;

/// 블록의 종류.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockKind {
    Mention,
    Comment,
    Edit,
    Promo,
    Unknown,
}

/// 강한 순서. 한 블록에 동작이 여럿이면 앞선 것이 종류다.
const KIND_ORDER: [BlockKind; 3] = [BlockKind::Mention, BlockKind::Comment, BlockKind::Edit];

/// 메일의 페이지 블록 하나.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MailBlock {
    pub kind: BlockKind,
    pub title: Option<String>,
    pub actors: Vec<String>,
    /// 행위자가 '외 N명'으로 묶여 이름을 다 모른다.
    pub others: bool,
    pub mentions_me: bool,
}

/// 본인 이름의 성과 이름. 한국어 이름은 첫 글자가 성이다. 설정하지 않았으면 None이고 이름 일치는 없다.
fn name_parts() -> Option<(String, String)> {
    kairos_core::profile::current().identity.display_name.as_deref().map(|name| {
        let mut chars = name.chars();
        let family = chars.next().map(String::from).unwrap_or_default();
        (regex::escape(&family), regex::escape(chars.as_str()))
    })
}

/// 업데이트 메일 제목의 이름은 표시 이름이라 외부 문자열이다. 워크스페이스를 고정해 앞에서부터 맞춘다.
/// 워크스페이스 이름은 조직 프로필의 값이다. 없으면 이 형식의 메일을 알아보지 않는다.
static PROMO_SUBJECT: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let workspace = regex::escape(kairos_core::profile::current().org.notion_workspace.as_deref()?);
    Some(pattern(&format!(r"^(?:{workspace}의 인기 페이지|추천 페이지 - {workspace}): ({dot}+)$", dot = js::DOT)))
});
/// 칸 안쪽이 파이프 네 개로 시작한다. 페이지 머리는 많아야 세 개다.
static PREVIEW_ROW: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r"^(?:[{ws}]*\|){{4}}", ws = js::WS)));
/// 제목 칸 뒤에 부제 칸이 있다.
static LISTING_ROW: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"[^|{ws}][{ws}]*\|[{ws}]*\|[{ws}]*\|[{ws}]*[^|{ws}]", ws = js::WS)));
static STATED_PAGES: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!("([0-9]+)[{ws}]*페이지의 변경 사항", ws = js::WS)));
static HAS_WORD: LazyLock<Regex> = LazyLock::new(|| pattern(r"[\p{L}\p{N}]"));
/// 앞 글자가 한글이면 기준서, 표준서 같은 다른 낱말이다. 성까지 쓴 이름은 일치다.
/// 위치마다 하는 일이 일정해 시간은 줄 길이에 비례한다. 기본 되짚기 상한은 긴 줄에서 오류가 되어 이름을 놓치므로 푼다.
static MY_NAME: LazyLock<Option<FancyRegex>> = LazyLock::new(|| {
    let (family, given) = name_parts()?;
    Some(
        RegexBuilder::new(&format!("(?<![가-힣])(?:{family})?{given}"))
            .backtrack_limit(usize::MAX)
            .build()
            .expect("이스케이프한 이름의 정규식"),
    )
});
/// 이름을 모르는 글쓴이가 붙은 형태. 두 글자 이상이어야 기준서 같은 낱말과 갈린다.
static GLUED_NAME: LazyLock<Option<Regex>> =
    LazyLock::new(|| name_parts().map(|(_, given)| pattern(&format!("^[가-힣]{{2,4}}{given}"))));

fn has_my_name(text: &str) -> bool {
    MY_NAME.as_ref().is_some_and(|name| name.is_match(text).unwrap_or(false))
}

/// 아이콘은 글자가 없는 칸이다. 비어 있을 수 있다. 제목 뒤의 칸은 부제이므로 쓰지 않는다.
fn page_title(cells: &str) -> Option<String> {
    let cleaned = without_tracking(&without_links(cells));
    let parts: Vec<&str> = cleaned.split('|').map(js::trim).filter(|cell| !cell.is_empty()).collect();
    let named = match parts.first() {
        Some(first) if !HAS_WORD.is_match(first) => &parts[1..],
        _ => &parts[..],
    };
    named.first().map(|title| (*title).to_owned())
}

/// 내용 줄은 글쓴이 이름으로 시작하기도 한다. 그 이름은 나를 부른 것이 아니다. 놓침을 오탐보다 무겁게 본다.
fn names_me(line: &str, actors: &[String], others: bool) -> bool {
    line.split(['[', ']', '|', '*']).any(|segment| {
        let text = js::trim(segment);
        if others && GLUED_NAME.as_ref().is_some_and(|glued| glued.is_match(text)) {
            return true;
        }
        let Some(author) = actors.iter().find(|actor| text.starts_with(actor.as_str())) else {
            return has_my_name(text);
        };
        let rest = &text[author.len()..];
        if has_my_name(rest) {
            return true;
        }
        // 글쓴이가 띄어 쓴 머리면 뒤만 본다. 붙어 있으면 행위자가 내 이름의 앞부분일 수 있다.
        author != self_name() && !rest.starts_with(js::is_js_space) && has_my_name(text)
    })
}

struct Page<'a> {
    title: Option<String>,
    rows: Vec<&'a Row>,
    /// 동작 줄이 뒤따라 머리로 판정했다.
    is_head: bool,
    /// 미리보기 바로 뒤의 머리다. 칸 모양이 미리보기 연속과 같다.
    after_preview: bool,
}

impl Page<'_> {
    fn nameless() -> Self {
        Self { title: None, rows: Vec::new(), is_head: false, after_preview: false }
    }
}

fn to_block(page: &Page<'_>) -> MailBlock {
    let mut kinds = Vec::new();
    let mut actors: Vec<String> = Vec::new();
    let mut others = false;
    let mut content = Vec::new();
    for row in &page.rows {
        let Some(action) = &row.action else {
            content.push(row.content.as_str());
            continue;
        };
        kinds.push(action.kind);
        match &action.actors {
            None => content.push(row.content.as_str()),
            Some(found) => {
                others |= found.others;
                for actor in &found.names {
                    if !actors.contains(actor) {
                        actors.push(actor.clone());
                    }
                }
            }
        }
    }
    let kind = KIND_ORDER.into_iter().find(|kind| kinds.contains(kind)).unwrap_or(BlockKind::Unknown);
    MailBlock {
        kind,
        // 형식을 모르는 블록의 머리는 본문일 수 있다.
        title: if kind == BlockKind::Unknown { None } else { page.title.clone() },
        mentions_me: kind == BlockKind::Mention || content.iter().any(|line| names_me(line, &actors, others)),
        actors,
        others,
    }
}

fn unknown_mail() -> MailBlock {
    MailBlock { kind: BlockKind::Unknown, title: None, actors: Vec::new(), others: false, mentions_me: false }
}

/// 미리보기 연속이 동작 앞에 오면 머리와 구별되지 않는다. 머리말의 페이지 수가 맞을 때만 그 자리의 제목을 믿는다.
fn drop_doubtful_titles(pages: &mut [Page<'_>], stated: Option<f64>) {
    let heads = pages.iter().filter(|page| page.is_head).count();
    if stated == Some(heads as f64) {
        return;
    }
    for page in pages.iter_mut().filter(|page| page.after_preview) {
        page.title = None;
    }
}

/// 머리말이 밝힌 페이지 수. 0은 없는 것으로 친다.
fn stated_pages(text: &str) -> Option<f64> {
    let digits = STATED_PAGES.captures(text)?;
    digits[1].parse().ok().filter(|count| *count != 0.0)
}

fn to_pages(rows: &[Row]) -> (Vec<Page<'_>>, Option<f64>) {
    let mut pages: Vec<Page<'_>> = Vec::new();
    let mut stated = None;
    let mut after_action = false;
    for (index, row) in rows.iter().enumerate() {
        if pages.is_empty() && stated.is_none() {
            stated = stated_pages(&row.text);
        }
        if let Some(cells) = row.cells.as_deref().filter(|cells| !PREVIEW_ROW.is_match(cells)) {
            let previous_linked = index > 0 && rows[index - 1].cells.is_some();
            let after_preview = after_action && previous_linked;
            if rows.get(index + 1).is_some_and(|next| next.action_like) {
                pages.push(Page { title: page_title(cells), rows: Vec::new(), is_head: true, after_preview });
                after_action = false;
                continue;
            }
            if !after_action {
                // 머리 형태인데 동작이 뒤따르지 않으면 형식이 바뀐 것일 수 있다. 제목 없는 unknown 블록으로 남긴다.
                if !LISTING_ROW.is_match(cells) {
                    pages.push(Page::nameless());
                }
                continue;
            }
        }
        // 머리를 놓친 동작 줄도 버리지 않는다. 앞머리의 다른 줄은 어느 블록에도 속하지 않는다.
        if row.action.is_some() && pages.is_empty() {
            pages.push(Page::nameless());
        }
        if let Some(page) = pages.last_mut() {
            page.rows.push(row);
        }
        after_action = row.action_like || (after_action && row.cells.is_some());
    }
    (pages, stated)
}

/// 메일 한 통을 블록으로 나눈다. 동작 문구가 없는 메일과 블록도 unknown으로 남긴다.
pub fn parse_mail(subject: &str, body: &str) -> Vec<MailBlock> {
    let heading: String = subject.nfc().collect();
    let normalized: String = body.nfc().collect();
    let lines: Vec<&str> = normalized.split('\n').map(js::trim).filter(|line| !line.is_empty()).collect();
    let footer = lines.iter().rposition(|line| *line == "Notion").unwrap_or(lines.len());
    let rows: Vec<Row> = lines[..footer].iter().map(|line| to_row(line)).collect();

    // 본문에 동작이 있으면 제목보다 본문을 믿는다. 요약 메일 제목은 표시 이름을 담는다.
    if let Some(promo) = PROMO_SUBJECT.as_ref().and_then(|promo| promo.captures(&heading))
        && rows.iter().all(|row| row.action.is_none())
    {
        let cleaned = without_tracking(&promo[1]);
        let title = Some(js::trim(&cleaned)).filter(|title| !title.is_empty()).map(str::to_owned);
        return vec![MailBlock { kind: BlockKind::Promo, title, ..unknown_mail() }];
    }

    let (mut pages, stated) = to_pages(&rows);
    if pages.is_empty() {
        return vec![unknown_mail()];
    }
    drop_doubtful_titles(&mut pages, stated);
    pages.iter().map(to_block).collect()
}
