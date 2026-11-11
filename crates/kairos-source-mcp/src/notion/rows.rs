//! 메일 본문 한 줄의 해석. 동작 문구, 행위자, 표 칸을 뽑는다.
//!
//! 본문 구조(측정 25통, 링크는 줄임). `[|...|](추적 링크)` 줄은 네 가지다.
//!
//! ```text
//! [| || 📝 | 페이지 제목 |](...)                 페이지 머리. 바로 다음 줄이 동작 줄이다
//! | Y | 홍길동 이 페이지 블록에서 댓글을 남겼습니다.[](...) |     동작 줄. 앞 줄이 내용이어도 온다(198회 중 53회)
//! | | 홍길동 댓글 내용[](...) 홍길동 다음 댓글[](...) |           내용 줄
//! | | 신규 | |, [이름내용](...)                                  내용 줄
//! [| | || | 담당자 || || 합성 값 |](...)         미리보기. 동작 줄에 이어지는 내용이고 제목이 아니다
//! [| | 미리보기 연속 |](...)                     칸 모양이 머리와 같다. 그래서 미리보기 뒤의 머리는 제목을 믿지 못한다
//! [| || 📝 | 제목 || | 부제 |](...)              요약 메일 앞머리의 추천 목록. 업데이트가 아니다
//! Notion                                         바닥글 시작
//! ```
//!
//! 머리말의 'N페이지의 변경 사항'은 문구가 있는 19통 모두 머리 수와 같았다. 미리보기 뒤의 머리를 믿는 근거로 쓴다.

use std::sync::LazyLock;

use kairos_core::js;
use regex::{Captures, NoExpand, Regex};

use super::parser::BlockKind;
use crate::shared::pattern;

/// 측정 최대는 618자다. 긴 줄은 머리가 아니다.
const MAX_HEAD_ROW: usize = 4096;
/// 측정 최대는 9자, 세 낱말, 2명이다. 넘으면 사람 이름이 아니라 내용이다.
const MAX_ACTOR_CHARS: usize = 40;
const MAX_ACTOR_WORDS: usize = 5;
const MAX_ACTORS: usize = 20;
/// 행위자 칸의 길이 상한. 줄이 길어도 아래 정규식이 앞부분만 보게 한다.
const MAX_ACTOR_CELL: usize = 240;
const MAX_LINK_TARGET: usize = 2048;

static LINKED_ROW: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(r"^\[\|({dot}*)\|\]\(https://mg\.mail\.notion\.com[^){ws}]*\)$", dot = js::DOT, ws = js::WS))
});
static BARE_URL: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r"https?://[^{ws}]+", ws = js::WS)));
static TRACKING_HOST: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"mg\.mail\.notion\.com[^{ws}]*", ws = js::WS)));
static SPACE_RUN: LazyLock<Regex> = LazyLock::new(|| pattern(&format!("[{ws}]+", ws = js::WS)));
/// 표의 두 번째 칸 전체가 문구일 때만 인정한다. 내용 칸 일부가 문구를 흉내 내도 행위자가 되지 않는다.
static ACTION_PHRASE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(
        r"^({dot}{{1,{MAX_ACTOR_CELL}}}?) ?이 페이지 ?(블록에서 댓글을 남겼습니다|페이지에서 나를 멘션했습니다|페이지를 편집했습니다)\.?$",
        dot = js::DOT
    ))
});
static LOOSE_ACTOR: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!(r"^({dot}{{1,{MAX_ACTOR_CELL}}}?) ?이 페이지", dot = js::DOT)));
/// 문구가 조금 달라도 태그는 놓치지 않는다. 측정 25통의 내용 줄에는 이 형태가 없었다.
static LOOSE_MENTION: LazyLock<Regex> = LazyLock::new(|| pattern("(?:나를|회원님을|저를) ?(?:멘션|언급)"));
static LOOSE_COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern("(?:블록|페이지)에서 댓글"));
static LOOSE_ACTION: LazyLock<Regex> = LazyLock::new(|| pattern(&format!("[{ws}]이 페이지", ws = js::WS)));
static OTHERS_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| pattern(&format!("[{ws}]*외[{ws}]*[0-9]+[{ws}]*명$", ws = js::WS)));

/// `]`, `(`, 주소, `)`로 이어진 링크 끝. 없으면 None이다.
/// 원본의 `[^)\s]{0,2048}`은 UTF-16 단위로 세어서 글자 수로 세는 `regex` 대신 직접 훑는다.
fn link_end(target: &str) -> Option<usize> {
    let mut used = 0;
    for (index, c) in target.char_indices() {
        if c == ')' {
            return Some(index);
        }
        used += c.len_utf16();
        if js::is_js_space(c) || used > MAX_LINK_TARGET {
            return None;
        }
    }
    None
}

/// `/\]\(https?:\/\/[^)\s]{0,2048}\)/g`를 `]`로 바꾼다.
pub(super) fn without_links(line: &str) -> String {
    const OPENING: &str = "](http";
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(found) = rest.find(OPENING) {
        let after = &rest[found + OPENING.len()..];
        let target = after.strip_prefix("s://").or_else(|| after.strip_prefix("://"));
        out.push_str(&rest[..=found]);
        rest = match target.and_then(|target| Some(&target[link_end(target)? + 1..])) {
            Some(after_link) => after_link,
            None => &rest[found + 1..],
        };
    }
    out.push_str(rest);
    out
}

pub(super) fn without_tracking(text: &str) -> String {
    let linkless = BARE_URL.replace_all(text, NoExpand(""));
    TRACKING_HOST.replace_all(&linkless, NoExpand("")).into_owned()
}

/// 행위자 칸의 이름들.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Actors {
    pub names: Vec<String>,
    /// 행위자가 '외 N명'으로 묶여 이름을 다 모른다.
    pub others: bool,
}

/// 행위자 칸을 이름으로 나눈다. 이름 같지 않으면 None이다. 내용이 actor_id로 저장되면 안 된다.
fn actors_of(text: &str) -> Option<Actors> {
    let cleaned = without_tracking(text);
    let parts: Vec<&str> = cleaned.split(',').map(js::trim).filter(|part| !part.is_empty()).collect();
    if parts.is_empty() || parts.len() > MAX_ACTORS {
        return None;
    }
    let mut names = Vec::with_capacity(parts.len());
    let mut others = false;
    for part in parts {
        let bare = js::trim(part.strip_suffix('님').unwrap_or(part));
        let suffixless = OTHERS_SUFFIX.replace(bare, NoExpand(""));
        let name = js::trim(&suffixless);
        others |= name != bare;
        if name.is_empty() || js::utf16_len(name) > MAX_ACTOR_CHARS || name.split(' ').count() > MAX_ACTOR_WORDS {
            return None;
        }
        names.push(name.to_owned());
    }
    Some(Actors { names, others })
}

/// 줄의 동작.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RowAction {
    pub kind: BlockKind,
    pub actors: Option<Actors>,
}

/// 본문 한 줄.
#[derive(Clone, Debug)]
pub(super) struct Row {
    /// 링크만 뺀 줄. `[]`가 댓글의 경계로 남는다. 이름 판정에 쓴다.
    pub content: String,
    /// 동작 판정에 쓰는 글자. 빈 링크와 연속 공백을 없앴다.
    pub text: String,
    /// `[|...|](추적 링크)` 줄의 칸.
    pub cells: Option<String>,
    pub action: Option<RowAction>,
    /// 동작 줄로 보인다. 문구를 몰라도 앞 줄을 머리로 판정하는 데 쓴다.
    pub action_like: bool,
}

/// 두 칸 표 줄의 둘째 칸. 칸이 더 있거나 표가 아니면 None이다.
fn action_cell(text: &str) -> Option<&str> {
    if !text.starts_with('|') {
        return None;
    }
    let mut parts = text.split('|').skip(2);
    let cell = parts.next()?;
    let rest = parts.next().unwrap_or("");
    (parts.next().is_none() && js::trim(rest).is_empty()).then(|| js::trim(cell))
}

/// 행위자 칸 제한은 UTF-16 단위다. 글자 수로 맞춘 뒤 길이를 다시 잰다.
fn actor_capture<'t>(pattern: &Regex, cell: &'t str) -> Option<Captures<'t>> {
    pattern.captures(cell).filter(|found| js::utf16_len(&found[1]) <= MAX_ACTOR_CELL)
}

fn kind_of_phrase(phrase: &str) -> BlockKind {
    if phrase.contains("멘션") {
        BlockKind::Mention
    } else if phrase.contains("댓글") {
        BlockKind::Comment
    } else {
        BlockKind::Edit
    }
}

fn action_of(text: &str) -> Option<RowAction> {
    let cell = action_cell(text);
    if let Some(exact) = cell.and_then(|cell| actor_capture(&ACTION_PHRASE, cell))
        && let Some(actors) = actors_of(&exact[1])
    {
        return Some(RowAction { kind: kind_of_phrase(&exact[2]), actors: Some(actors) });
    }
    let actors = cell.and_then(|cell| actor_capture(&LOOSE_ACTOR, cell)).and_then(|named| actors_of(&named[1]));
    if LOOSE_MENTION.is_match(text) {
        return Some(RowAction { kind: BlockKind::Mention, actors });
    }
    cell.filter(|cell| LOOSE_COMMENT.is_match(cell)).map(|_| RowAction { kind: BlockKind::Comment, actors })
}

pub(super) fn to_row(line: &str) -> Row {
    let content = without_links(line);
    // 공백을 접어 두면 아래 정규식은 줄 길이에 비례한다. 행위자 칸은 길이를 제한해 뒀다.
    let glued = content.replace("[]", "");
    let folded = SPACE_RUN.replace_all(&glued, NoExpand(" "));
    let text = js::trim(&folded).to_owned();
    let action = action_of(&text);
    let linked = (js::utf16_len(line) <= MAX_HEAD_ROW).then(|| LINKED_ROW.captures(line)).flatten();
    let action_like = action.is_some() || (text.starts_with('|') && LOOSE_ACTION.is_match(&text));
    Row { cells: linked.map(|found| found[1].to_owned()), content, text, action, action_like }
}
