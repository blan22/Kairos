use std::collections::HashSet;

use kairos_core::{KairosResult, string_enum};
use kairos_store::row::{enum_column, scalar, sql};
use kairos_store::rusqlite::{self, Params};
use kairos_store::{Connection, OptionalExtension, params};

use crate::item::{Item, ItemTier};

string_enum! {
    pub enum CardKind {
        Item => "item",
        Bundle => "bundle",
        Carryover => "carryover",
    }
}

string_enum! {
    pub enum SuggestionResponse {
        Accept => "accept",
        Later => "later",
        Done => "done",
        Drop => "drop",
        Keep => "keep",
        Ack => "ack",
        Expired => "expired",
        Superseded => "superseded",
        Closed => "closed",
    }
}

const P1_CARDS: &str = "tier = 'P1' AND kind IN ('item', 'carryover')";

/// 응답을 기다리는 노출 한 건과 그 항목.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingCardRow {
    pub item: Item,
    pub card_id: String,
    pub card_kind: CardKind,
    pub shown_at: String,
}

/// 응답 없이 떠 있는 노출.
pub(crate) struct OpenCard {
    pub id: i64,
    pub item_id: i64,
    pub kind: String,
}

/// 나간 알림 한 건의 합치기 재료.
pub(crate) struct RecentAlert {
    pub channel_id: Option<String>,
    pub object_key: String,
    pub card_id: String,
}

#[derive(Clone, Copy, Debug)]
pub struct SuggestionRepo<'a> {
    connection: &'a Connection,
}

impl<'a> SuggestionRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    fn run(&self, text: &str, values: impl Params) -> KairosResult<usize> {
        self.connection.execute(text, values).map_err(sql)
    }

    fn open_cards(&self, text: &str, values: impl Params) -> KairosResult<Vec<OpenCard>> {
        let mut statement = self.connection.prepare(text).map_err(sql)?;
        let rows = statement
            .query_map(values, |row| {
                Ok(OpenCard { id: row.get("id")?, item_id: row.get("item_id")?, kind: row.get("kind")? })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 미응답 노출에 응답을 적는다.
    pub(crate) fn answer(&self, item_id: i64, response: SuggestionResponse, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE suggestions SET response = ?, response_at = ? WHERE item_id = ? AND response IS NULL",
            params![response.as_str(), now, item_id],
        )?;
        Ok(())
    }

    pub(crate) fn overdue_cards(&self, cutoff: &str) -> KairosResult<Vec<OpenCard>> {
        self.open_cards(
            "SELECT id, item_id, kind FROM suggestions WHERE seen = 1 AND response IS NULL AND tier != 'P0' AND shown_at <= ?",
            params![cutoff],
        )
    }

    pub(crate) fn hidden_cards(&self) -> KairosResult<Vec<OpenCard>> {
        self.open_cards(
            "SELECT id, item_id, kind FROM suggestions WHERE seen = 1 AND response IS NULL AND tier != 'P0'",
            [],
        )
    }

    pub(crate) fn respond(&self, id: i64, response: SuggestionResponse, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE suggestions SET response = ?, response_at = ? WHERE id = ?",
            params![response.as_str(), now, id],
        )?;
        Ok(())
    }

    /// 카드 수로 세는 표시 건수. 같은 카드의 줄은 한 번이다.
    fn cards(&self, condition: &str, since: &str) -> KairosResult<i64> {
        scalar(
            self.connection,
            &format!("SELECT count(DISTINCT card_id) FROM suggestions WHERE seen = 1 AND {condition}"),
            params![since],
        )
    }

    pub(crate) fn p1_cards_after(&self, since: &str) -> KairosResult<i64> {
        self.cards(&format!("{P1_CARDS} AND shown_at > ?"), since)
    }

    pub(crate) fn p1_cards_since(&self, since: &str) -> KairosResult<i64> {
        self.cards(&format!("{P1_CARDS} AND shown_at >= ?"), since)
    }

    pub(crate) fn bundles_since(&self, since: &str) -> KairosResult<i64> {
        self.cards("kind = 'bundle' AND shown_at >= ?", since)
    }

    pub(crate) fn shown_since(&self, since: &str) -> KairosResult<i64> {
        self.cards("shown_at >= ?", since)
    }

    /// 응답을 기다리는 노출. 항목이 닫히거나 대기로 가면 그 쪽에서 응답을 적으므로 여기에 나오지 않는다.
    pub(crate) fn pending(&self) -> KairosResult<Vec<PendingCardRow>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT i.*, s.card_id, s.kind AS card_kind, s.shown_at FROM suggestions s JOIN items i ON i.id = s.item_id
                 WHERE s.seen = 1 AND s.response IS NULL ORDER BY s.id",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                let index = row.as_ref().column_index("card_kind")?;
                Ok(PendingCardRow {
                    item: Item::from_row(row)?,
                    card_id: row.get("card_id")?,
                    card_kind: enum_column(&row.get::<_, String>(index)?, index)?,
                    shown_at: row.get("shown_at")?,
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 오늘 이미 표면에 오른 항목. 넘어온 카드는 하루 한 번이다.
    pub(crate) fn exposed_since(&self, since: &str) -> KairosResult<HashSet<i64>> {
        let mut statement = self
            .connection
            .prepare("SELECT DISTINCT item_id FROM suggestions WHERE seen = 1 AND shown_at >= ?")
            .map_err(sql)?;
        let rows = statement.query_map(params![since], |row| row.get(0)).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn bundle_seen_since(&self, slot: &str) -> KairosResult<bool> {
        self.connection
            .query_row(
                "SELECT 1 FROM suggestions WHERE seen = 1 AND kind = 'bundle' AND shown_at >= ? LIMIT 1",
                params![slot],
                |_| Ok(()),
            )
            .optional()
            .map(|found| found.is_some())
            .map_err(sql)
    }

    pub(crate) fn last_p1_shown(&self) -> KairosResult<Option<String>> {
        self.connection
            .query_row(&format!("SELECT max(shown_at) FROM suggestions WHERE seen = 1 AND {P1_CARDS}"), [], |row| {
                row.get(0)
            })
            .map_err(sql)
    }

    /// P0가 기다리던 카드를 밀어낸 경우다. 사용자가 무시한 것이 아니므로 밀어내기로 세지 않는다.
    pub(crate) fn supersede_waiting(&self, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE suggestions SET response = 'superseded', response_at = ? WHERE seen = 1 AND response IS NULL",
            params![now],
        )?;
        Ok(())
    }

    pub(crate) fn insert_seen(
        &self,
        item_id: i64,
        card_id: &str,
        kind: CardKind,
        tier: ItemTier,
        now: &str,
    ) -> KairosResult<()> {
        self.run(
            "INSERT INTO suggestions (item_id, card_id, kind, tier, shown_at, surface, seen) VALUES (?, ?, ?, ?, ?, 'panel', 1)",
            params![item_id, card_id, kind.as_str(), tier.as_str(), now],
        )?;
        Ok(())
    }

    pub(crate) fn ack_cards(&self, now: &str) -> KairosResult<()> {
        self.run(
            "UPDATE suggestions SET response = 'ack', response_at = ? WHERE seen = 1 AND response IS NULL AND kind IN ('bundle', 'carryover')",
            params![now],
        )?;
        Ok(())
    }

    /// seen은 0이다. 알림 전달은 사용자가 봤다는 증거가 아니다(설계 6.1).
    pub(crate) fn insert_p0_alert(&self, item_id: i64, now: &str) -> KairosResult<()> {
        self.run(
            "INSERT INTO suggestions (item_id, card_id, kind, tier, shown_at, surface) VALUES (?, ?, 'p0_alert', 'P0', ?, 'osascript')",
            params![item_id, format!("alert-{item_id}"), now],
        )?;
        Ok(())
    }

    pub(crate) fn insert_p1_alert(&self, item_id: i64, card: &str, now: &str) -> KairosResult<()> {
        self.run(
            "INSERT INTO suggestions (item_id, card_id, kind, tier, shown_at, surface) VALUES (?, ?, 'item', 'P1', ?, 'osascript')",
            params![item_id, card, now],
        )?;
        Ok(())
    }

    /// 구간 안에 실제로 나간 알림만 본다. 합쳐진 건은 suggestions에 없어서 구간을 늘리지 않고, 실패한 알림은 구간을 만들지 않는다.
    pub(crate) fn recent_alerts(&self, since: &str) -> KairosResult<Vec<RecentAlert>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT i.channel_id, i.object_key, s.card_id FROM suggestions s JOIN items i ON i.id = s.item_id
                 WHERE s.surface = 'osascript' AND s.tier = 'P1' AND s.shown_at >= ?",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![since], |row| {
                Ok(RecentAlert { channel_id: row.get(0)?, object_key: row.get(1)?, card_id: row.get(2)? })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 패널이 세션 카드를 그렸다는 표시. run 상태가 아니라 노출의 기록이라 여기에 둔다.
    pub(crate) fn mark_ready_seen(&self, run_id: &str, now: &str) -> KairosResult<()> {
        self.run("UPDATE runs SET ready_seen_at = ? WHERE id = ?", params![now, run_id])?;
        Ok(())
    }
}
