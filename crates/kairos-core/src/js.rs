//! JavaScript 의미를 그대로 재현하는 도구. 설계 문서 5.3절의 표를 코드로 옮긴 것이다.
//!
//! 저장된 값과 판정이 TS 시절과 같아야 하므로, 편의상 Rust 기본 동작으로 바꾸면 안 된다.

/// JS 정규식 `\s`와 `String.prototype.trim`의 공백 집합을 문자 클래스 안에 넣어 쓰는 조각.
/// Rust의 `\s`와 달리 U+FEFF를 포함하고 U+0085를 제외한다.
pub const WS: &str = r"\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

/// JS 정규식 `.`. 줄 끝 넷을 제외한다. Rust의 `.`은 `\n`만 제외한다.
pub const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

/// JS `\s`에 해당하는 문자인가.
pub fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\x0B' | '\x0C' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}' | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}'
    )
}

/// `String.prototype.trim`.
pub fn trim(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

/// `String.prototype.trimEnd`.
pub fn trim_end(text: &str) -> &str {
    text.trim_end_matches(is_js_space)
}

/// `String.prototype.length`. UTF-16 단위 수다.
pub fn utf16_len(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// `text.slice(0, limit)` 뒤에 `toWellFormed()`를 한 결과. 서로게이트 쌍이 잘리면 U+FFFD가 된다.
pub fn slice_utf16(text: &str, limit: usize) -> String {
    let mut used = 0;
    for (index, c) in text.char_indices() {
        let width = c.len_utf16();
        if used + width > limit {
            let mut out = text[..index].to_owned();
            if used < limit {
                out.push('\u{FFFD}');
            }
            return out;
        }
        used += width;
    }
    text.to_owned()
}

/// `JSON.parse`. 짝 없는 서로게이트 이스케이프도 받는다. 그런 값은 U+FFFD로 읽는다.
/// serde_json은 이 입력을 거부하는데, 외부 응답 하나 때문에 질의 전체가 실패하면 안 된다.
pub fn parse_json(text: &str) -> Result<serde_json::Value, serde_json::Error> {
    match serde_json::from_str(text) {
        Ok(value) => Ok(value),
        Err(error) => {
            let repaired = repair_lone_surrogates(text);
            if repaired == text { Err(error) } else { serde_json::from_str(&repaired) }
        }
    }
}

const fn hex_value(byte: u8) -> Option<u32> {
    match byte {
        b'0'..=b'9' => Some((byte - b'0') as u32),
        b'a'..=b'f' => Some((byte - b'a') as u32 + 10),
        b'A'..=b'F' => Some((byte - b'A') as u32 + 10),
        _ => None,
    }
}

fn unicode_escape_at(bytes: &[u8], index: usize) -> Option<u32> {
    if bytes.get(index) != Some(&b'\\') || bytes.get(index + 1) != Some(&b'u') {
        return None;
    }
    let mut value = 0;
    for offset in 2..6 {
        value = value * 16 + hex_value(*bytes.get(index + offset)?)?;
    }
    Some(value)
}

fn repair_lone_surrogates(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    let mut copied = 0;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            index += 1;
            continue;
        }
        let Some(unit) = unicode_escape_at(bytes, index) else {
            // `\\` 같은 다른 이스케이프는 두 글자를 건너뛴다. 그래야 `\\ud83d`를 이스케이프로 착각하지 않는다.
            index += 2;
            continue;
        };
        let high = (0xD800..0xDC00).contains(&unit);
        let low = (0xDC00..0xE000).contains(&unit);
        let paired = high && unicode_escape_at(bytes, index + 6).is_some_and(|next| (0xDC00..0xE000).contains(&next));
        if paired {
            index += 12;
        } else if high || low {
            out.push_str(&text[copied..index]);
            out.push_str("\\ufffd");
            index += 6;
            copied = index;
        } else {
            index += 6;
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 공백_집합은_js와_같다() {
        assert_eq!(trim("\u{FEFF} 가 \u{3000}"), "가");
        assert_eq!(trim("\u{85}가"), "\u{85}가");
        assert_eq!(trim_end("가 \u{A0}"), "가");
    }

    #[test]
    fn 길이와_자르기는_utf16_단위다() {
        assert_eq!(utf16_len("가😀"), 3);
        assert_eq!(slice_utf16("가😀나", 3), "가😀");
        assert_eq!(slice_utf16("가😀나", 2), "가\u{FFFD}");
        assert_eq!(slice_utf16("가나", 9), "가나");
        assert_eq!(slice_utf16("😀", 0), "");
    }

    #[test]
    fn 짝_없는_서로게이트를_받는다() {
        assert_eq!(parse_json(r#"{"a":"x\ud83d"}"#).unwrap()["a"], "x\u{FFFD}");
        assert_eq!(parse_json(r#"["\ude00y"]"#).unwrap()[0], "\u{FFFD}y");
        assert_eq!(parse_json(r#"["\ud83d\ude00"]"#).unwrap()[0], "😀");
        assert_eq!(parse_json(r#"["\\ud83d"]"#).unwrap()[0], "\\ud83d");
        assert!(parse_json("{").is_err());
    }
}
