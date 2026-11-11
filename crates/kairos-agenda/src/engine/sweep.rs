//! 시간이 지나 바뀌는 항목과 카드. 패널 조회도 같은 단계를 먼저 돌린다.

use kairos_core::{Instant, KairosResult};
use kairos_policy::CARD_TTL_MS;
use kairos_store::Connection;

use crate::repo::{ItemRepo, SuggestionRepo, SuggestionResponse};

/// 대기 시각이 지난 항목을 연다.
pub(crate) fn wake_parked(connection: &Connection, now: &str) -> KairosResult<usize> {
    ItemRepo::new(connection).wake(now)
}

/// 응답 없이 시간이 지난 카드를 내린다. 응답 없이 넘어간 노출이 밀어내기 1회다.
pub(crate) fn expire_cards(connection: &Connection, now: Instant) -> KairosResult<usize> {
    let suggestions = SuggestionRepo::new(connection);
    let items = ItemRepo::new(connection);
    let at = now.to_iso();
    let rows = suggestions.overdue_cards(&now.minus_ms(CARD_TTL_MS).to_iso())?;
    for row in &rows {
        suggestions.respond(row.id, SuggestionResponse::Expired, &at)?;
        items.count_push(row.item_id)?;
    }
    Ok(rows.len())
}

/// 보류로 가려진 카드를 내린다. 사용자가 보지 못한 시간은 밀어내기로 세지 않는다(점심, 포커스, 일시정지, 근무 시간 밖).
/// 단건 카드는 보류가 끝나면 다시 고른다. 묶음과 넘어온 카드는 다시 고르지 않으므로 다음 묶음에 싣는다.
pub(crate) fn hold_cards(connection: &Connection, now: &str) -> KairosResult<usize> {
    let suggestions = SuggestionRepo::new(connection);
    let items = ItemRepo::new(connection);
    let rows = suggestions.hidden_cards()?;
    for row in &rows {
        suggestions.respond(row.id, SuggestionResponse::Superseded, now)?;
        if row.kind != "item" {
            items.queue_for_bundle(row.item_id)?;
        }
    }
    Ok(rows.len())
}
