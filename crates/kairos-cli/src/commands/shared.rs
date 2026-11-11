//! 여러 명령이 쓰는 입력 검사와 줄 만들기.

use kairos_agenda::{ItemState, ItemView, is_item_id};
use kairos_core::text::sanitize;

use crate::reply::Reply;

/// DB의 값도 외부 문자열이다. 출력 직전에 다시 정제한다.
pub(super) fn shown(value: Option<&str>) -> String {
    let cleaned = sanitize(value.unwrap_or_default());
    if cleaned.is_empty() { "(제목 없음)".to_owned() } else { cleaned }
}

pub(super) fn line(view: &ItemView) -> String {
    let parked = match view.state {
        ItemState::Parked => " (대기)",
        ItemState::Accepted => " (수락)",
        _ => "",
    };
    format!("#{} {} {} {} {}{parked}", view.id, view.tier, view.reason_label, view.source_label, view.title)
}

/// 틀린 입력이면 답을 낸 뒤의 종료 코드를 `Err`로 준다.
pub(super) fn parse_ids(values: &[String], reply: Reply<'_>) -> Result<Vec<i64>, i32> {
    if values.is_empty() {
        return Err(reply.usage("항목 ID가 필요합니다"));
    }
    if !values.iter().all(|value| is_item_id(value)) {
        return Err(reply.fail("bad_id", "항목 ID는 양의 정수입니다", 2));
    }
    Ok(values.iter().filter_map(|value| value.parse().ok()).collect())
}

pub(super) fn one_id(values: &[String], reply: Reply<'_>) -> Result<i64, i32> {
    if values.len() > 1 {
        return Err(reply.usage("항목 ID는 하나만 받습니다"));
    }
    parse_ids(values, reply).map(|ids| ids[0])
}
