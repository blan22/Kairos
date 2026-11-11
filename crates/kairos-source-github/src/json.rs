//! 외부 JSON을 읽는 도구. 키 조회는 `Value::get`이 객체가 아니면 None을 주므로 원본의 `obj(x)?.key`와 같다.

use serde_json::Value;

/// 비어 있지 않은 문자열만 값으로 친다.
pub(crate) fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|text| !text.is_empty())
}

/// 연결의 `nodes`에서 객체만 고른다. 다른 모양은 빈 목록이다.
pub(crate) fn nodes(connection: Option<&Value>) -> Vec<&Value> {
    let items = connection.and_then(|value| value.get("nodes")).and_then(Value::as_array);
    items.map_or_else(Vec::new, |items| items.iter().filter(|item| item.is_object()).collect())
}

/// 낱말 목록과 견주려는 `String(value)`. 문자열과 원소 하나짜리 배열만 낱말이 될 수 있다.
pub(crate) fn word(value: Option<&Value>) -> Option<&str> {
    match value? {
        Value::String(word) => Some(word),
        Value::Array(items) if items.len() == 1 => word(items.first()),
        _ => None,
    }
}

/// 숫자 값의 `String(number)`. 숫자가 아니면 None이다. dedupe 키에 들어가므로 JS 표기를 따른다.
pub(crate) fn number_text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_f64).map(js_number)
}

fn js_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    if (1e-6..1e21).contains(&value.abs()) {
        return value.to_string();
    }
    let scientific = format!("{value:e}");
    if scientific.contains("e-") { scientific } else { scientific.replacen('e', "e+", 1) }
}
