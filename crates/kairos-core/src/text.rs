//! 외부 문자열의 정제. 저장과 표시 앞에 반드시 거친다.

use std::sync::LazyLock;

use regex::{NoExpand, Regex};

use crate::js;

pub const TEXT_HEAD_LIMIT: usize = 120;
/// 정규식에 넣기 전에 자르는 배수. 저장은 앞부분뿐이라 그 뒤를 볼 이유가 없다.
const SCAN_FACTOR: usize = 16;

// 제어 시퀀스는 통째로 지운다. 문자만 지우면 `[31m`이 글자로 남는다.
// 문자열형 시퀀스는 종결자가 있을 때만 지운다. 없으면 뒤의 내용까지 사라진다.
static ESCAPE_SEQUENCES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?:\x1B\[|\x{9B})[0-?]*[ -/]*[@-~]", // CSI
        r"|(?:\x1B\]|\x{9D})[^\x07\x1B\x{90}\x{98}\x{9C}-\x{9F}]*(?:\x07|\x1B\\|\x{9C})", // OSC
        r"|(?:\x1B[PX^_]|[\x{90}\x{98}\x{9E}\x{9F}])[^\x1B\x{90}\x{98}\x{9C}-\x{9F}]*(?:\x1B\\|\x{9C})", // DCS, SOS, PM, APC
        r"|\x1B[ -/]*[0-~]", // 나머지 ESC 시퀀스
    ))
    .expect("상수 정규식")
});
static LINE_LIKE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\t\n\x0B\x0C\r\x{85}\x{2028}\x{2029}]+").expect("상수 정규식"));
static CONTROL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\x00-\x1F\x7F-\x{9F}]").expect("상수 정규식"));
static BIDI: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x{61C}\x{200E}\x{200F}\x{202A}-\x{202E}\x{2066}-\x{2069}]").expect("상수 정규식"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" {2,}").expect("상수 정규식"));
// 쓰는 서비스의 토큰 형식이다. 낱말처럼 보일 수 있는 접두어는 길이를 길게 잡는다.
// \b는 ASCII 기준이어야 한다. 유니코드 기준이면 `한글ghp_...`처럼 붙은 토큰을 놓친다.
static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    let patterns = [
        "gh[pousr]_[A-Za-z0-9]{8,}",
        "github_pat_[A-Za-z0-9_]{8,}",
        r"xox[a-z]\.xox[a-z]-[A-Za-z0-9-]{8,}",
        "xox[a-z]-[A-Za-z0-9-]{8,}",
        "xapp-[A-Za-z0-9-]{8,}",
        "sk-ant-[A-Za-z0-9_-]{8,}",
        "AKIA[0-9A-Z]{16}",
        r"eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
        "figd_[A-Za-z0-9_-]{16,}",
        "ntn_[A-Za-z0-9]{16,}",
        "secret_[A-Za-z0-9]{32,}",
        "[0-9a-fA-F]{32,}",
    ];
    Regex::new(&format!(r"(?-u:\b)(?:{})(?-u:\b)", patterns.join("|"))).expect("상수 정규식")
});
// 스킴이 있는 주소와 www로 시작하는 주소. Slack의 <주소|글자> 꺾쇠는 끝으로 친다.
static LINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r#"(?-u:\b)(?:[A-Za-z][A-Za-z0-9+.-]*://|[wW][wW][wW]\.)[^{}<>|"']+"#, js::WS))
        .expect("상수 정규식")
});
static MAILTO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?-u:\b)[mM][aA][iI][lL][tT][oO]:").expect("상수 정규식"));
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+").expect("상수 정규식"));

fn erase(pattern: &Regex, text: &str) -> String {
    pattern.replace_all(text, NoExpand("")).into_owned()
}

/// 외부 문자열을 저장과 표시에 쓸 수 있는 한 줄로 만든다.
/// C0, C1, ESC 시퀀스, bidi 재정렬 문자를 없애고, 코드 포인트 기준 limit자로 자른다.
pub fn sanitize_text(input: &str, limit: usize) -> String {
    let scanned = js::slice_utf16(input, limit * SCAN_FACTOR);
    let without_escapes = erase(&ESCAPE_SEQUENCES, &scanned);
    let one_line = LINE_LIKE.replace_all(&without_escapes, NoExpand(" "));
    let printable = erase(&BIDI, &erase(&CONTROL, &one_line));
    let squeezed = SPACES.replace_all(&printable, NoExpand(" "));
    let cleaned = js::trim(&squeezed);
    match cleaned.char_indices().nth(limit) {
        Some((end, _)) => js::trim_end(&cleaned[..end]).to_owned(),
        None => cleaned.to_owned(),
    }
}

/// 기본 길이의 `sanitize_text`.
pub fn sanitize(input: &str) -> String {
    sanitize_text(input, TEXT_HEAD_LIMIT)
}

fn redact_secrets(input: &str) -> String {
    let scanned = js::slice_utf16(input, TEXT_HEAD_LIMIT * SCAN_FACTOR);
    SECRET.replace_all(&scanned, NoExpand("[redacted]")).into_owned()
}

/// 저장할 앞부분. 정제에 더해 토큰처럼 보이는 값을 가린다. head_ref_oid 같은 식별자에는 쓰지 않는다.
pub fn sanitize_head(input: &str) -> String {
    sanitize(&redact_secrets(input))
}

/// 외부에서 온 앞부분의 저장 값. 추적 링크와 참석자 주소가 앞부분에 끼어 저장되지 않게 자리표시자로 바꾼다.
/// 링크 안의 토큰은 앞에서 가려지고 주소는 다음 단계에서 통째로 바뀐다. 사용자가 직접 쓴 제목에는 `sanitize_head`를 쓴다.
pub fn sanitize_external(input: &str) -> String {
    let scanned = redact_secrets(input);
    let linkless = LINK.replace_all(&scanned, NoExpand("[링크]"));
    let plain = MAILTO.replace_all(&linkless, NoExpand(""));
    sanitize(&EMAIL.replace_all(&plain, NoExpand("[주소]")))
}

/// 문자열이 아닌 입력과 서로게이트 시험은 타입과 `js::parse_json`이 맡는다
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c0와_del을_지운다() {
        assert_eq!(sanitize("a\u{0}b\u{7}c\u{7f}d"), "abcd");
    }

    #[test]
    fn c1을_지운다() {
        assert_eq!(sanitize("a\u{80}b\u{85}c\u{9f}d"), "ab cd");
    }

    #[test]
    fn esc_시퀀스를_통째로_지운다() {
        assert_eq!(sanitize("\u{1b}[31m빨강\u{1b}[0m"), "빨강");
        assert_eq!(sanitize("\u{1b}]0;title\u{7}본문"), "본문");
        assert_eq!(sanitize("\u{9b}2J본문"), "본문");
        assert_eq!(sanitize("\u{1b}c본문"), "본문");
    }

    #[test]
    fn 종결자_없는_시퀀스는_뒤의_내용을_지우지_않는다() {
        assert_eq!(sanitize("앞\u{9f}뒤"), "앞뒤");
        assert_eq!(sanitize("앞\u{90}뒤"), "앞뒤");
        assert_eq!(sanitize("앞\u{9d}뒤"), "앞뒤");
    }

    #[test]
    fn bidi_재정렬_문자를_지운다() {
        let bidi = "\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}\u{200e}\u{200f}\u{61c}";
        assert_eq!(sanitize(&format!("a{bidi}b")), "ab");
    }

    #[test]
    fn 줄바꿈과_탭은_공백_하나가_된다() {
        assert_eq!(sanitize("첫 줄\r\n둘째\t줄\u{2028}셋째"), "첫 줄 둘째 줄 셋째");
    }

    #[test]
    fn 앞에서_120자로_자른다() {
        assert_eq!(TEXT_HEAD_LIMIT, 120);
        assert_eq!(sanitize(&"가".repeat(500)).chars().count(), 120);
    }

    #[test]
    fn 정제_뒤에_길이를_잰다() {
        let input = format!("{}{}", "\u{7}".repeat(200), "a".repeat(120));
        assert_eq!(sanitize(&input), "a".repeat(120));
    }

    #[test]
    fn 서로게이트_쌍을_가르지_않는다() {
        assert_eq!(sanitize(&"😀".repeat(130)).chars().count(), 120);
    }

    #[test]
    fn 문자열_시작_문자가_반복된_긴_입력도_빠르게_끝난다() {
        for start in ["\u{9d}", "\u{90}", "\u{98}", "\u{9e}", "\u{9f}", "\u{1b}]", "\u{1b}P"] {
            let started = std::time::Instant::now();
            sanitize(&start.repeat(40_000));
            assert!(started.elapsed().as_millis() < 100, "{start:?}");
        }
    }

    #[test]
    fn 자른_범위_안의_시퀀스는_그대로_지운다() {
        let input = format!("\u{9d}0;x\u{7}본문{}", "가".repeat(40_000));
        assert!(sanitize(&input).starts_with("본문"));
    }

    #[test]
    fn 저장되는_앞부분은_토큰처럼_보이는_값을_가린다() {
        let cases = [
            format!("ghp_{}", "a".repeat(36)),
            "xoxb-1234567890-abcdef".into(),
            "sk-ant-api03-abcdefghij".into(),
            "a".repeat(40),
        ];
        for token in cases {
            assert_eq!(sanitize_head(&format!("앞 {token} 뒤")), "앞 [redacted] 뒤", "{token}");
        }
        assert_eq!(sanitize_head("일반 제목 #12710"), "일반 제목 #12710");
        assert_eq!(sanitize(&"a".repeat(40)), "a".repeat(40), "식별자 정제는 그대로다");
    }

    #[test]
    fn 한글에_붙은_토큰도_가린다() {
        let token = format!("ghp_{}", "a".repeat(36));
        assert_eq!(sanitize_head(&format!("토큰{token}끝")), "토큰[redacted]끝");
    }

    #[test]
    fn 외부_앞부분의_링크와_주소를_가린다() {
        assert_eq!(sanitize_external("보기 https://mg.mail.notion.com/x?y=1 끝"), "보기 [링크] 끝");
        assert_eq!(sanitize_external("mailto:a.b@c.example 연락"), "[주소] 연락");
        assert_eq!(sanitize_external("WWW.Example.com 보기"), "[링크] 보기");
    }
}
