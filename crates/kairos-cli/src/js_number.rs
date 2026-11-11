//! JS의 숫자 변환. 명령의 분 값이 `Number(문자열)`을 거친다.

use std::sync::LazyLock;

use kairos_core::js;
use regex::Regex;

static DECIMAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[+-]?(?:[0-9]+\.?[0-9]*|\.[0-9]+)(?:[eE][+-]?[0-9]+)?$").expect("상수 정규식"));

fn radix(text: &str) -> Option<f64> {
    let (radix, digits) = match text.get(..2)? {
        "0x" | "0X" => (16, &text[2..]),
        "0o" | "0O" => (8, &text[2..]),
        "0b" | "0B" => (2, &text[2..]),
        _ => return None,
    };
    if digits.is_empty() {
        return Some(f64::NAN);
    }
    let mut value = 0.0_f64;
    for digit in digits.chars() {
        let Some(digit) = digit.to_digit(radix) else {
            return Some(f64::NAN);
        };
        value = value * f64::from(radix) + f64::from(digit);
    }
    Some(value)
}

/// `Number(text)`. 빈 문자열은 0이고 앞뒤 공백을 지운다. `inf`, `nan`처럼 Rust만 받는 글자는 NaN이다.
pub fn js_number(text: &str) -> f64 {
    let trimmed = js::trim(text);
    match trimmed {
        "" => return 0.0,
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(value) = radix(trimmed) {
        return value;
    }
    if !DECIMAL.is_match(trimmed) {
        return f64::NAN;
    }
    trimmed.parse().unwrap_or(f64::NAN)
}

/// `Number.isInteger`.
pub fn is_integer(value: f64) -> bool {
    value.is_finite() && value.fract() == 0.0
}

/// `(ms / 1000).toFixed(1)`. 정확히 가운데이면 JS는 큰 쪽으로 가고 Rust의 서식은 짝수 쪽으로 간다.
/// 천분의 값 중 이진수로 정확히 가운데인 경우는 0.25, 0.75 꼴뿐이다.
pub fn seconds_fixed1(ms: i64) -> String {
    if ms >= 0 && ms % 500 == 250 {
        let tenths = (ms + 50) / 100;
        return format!("{}.{}", tenths / 10, tenths % 10);
    }
    format!("{:.1}", ms as f64 / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number는_js의_문자열_변환을_따른다() {
        for (text, expected) in [
            ("30", 30.0),
            (" 30 ", 30.0),
            ("\u{FEFF}7\n", 7.0),
            ("", 0.0),
            ("   ", 0.0),
            ("+5", 5.0),
            ("-5", -5.0),
            ("1e2", 100.0),
            ("1.", 1.0),
            (".5", 0.5),
            ("0x10", 16.0),
            ("0b11", 3.0),
            ("0o7", 7.0),
            ("30.0", 30.0),
        ] {
            assert_eq!(js_number(text), expected, "{text:?}");
        }
        assert_eq!(js_number("Infinity"), f64::INFINITY);
        assert_eq!(js_number("-Infinity"), f64::NEG_INFINITY);
        for text in ["abc", "1_000", "inf", "nan", "NaN", "infinity", "0x", "-0x10", "1e", "1 2", "٣", "."] {
            assert!(js_number(text).is_nan(), "{text:?}");
        }
    }

    #[test]
    fn 정수_판정은_무한대와_소수를_뺀다() {
        assert!(is_integer(30.0));
        assert!(is_integer(-1.0));
        assert!(!is_integer(1.5));
        assert!(!is_integer(f64::INFINITY));
        assert!(!is_integer(f64::NAN));
    }

    #[test]
    fn 소수_첫째_자리는_js의_반올림을_따른다() {
        for (ms, expected) in [
            (0, "0.0"),
            (250, "0.3"),
            (750, "0.8"),
            (1250, "1.3"),
            (1750, "1.8"),
            (350, "0.3"),
            (1450, "1.4"),
            (1050, "1.1"),
            (12_345, "12.3"),
            (20_000, "20.0"),
        ] {
            assert_eq!(seconds_fixed1(ms), expected, "{ms}");
        }
    }
}
