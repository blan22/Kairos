//! state.json과 engine.json.

use std::fs;

use kairos_core::home::write_file_atomic;
use kairos_core::model::Source;
use kairos_core::{Instant, KairosError, KairosHome, KairosResult, js};
use kairos_ledger::{EventRepo, ObjectRepo, OffDateBook, WouldShowEvent, check_invariants};
use kairos_policy::{Tier, tier_of};
use kairos_store::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{DisplayGate, DisplayStart, KairosControl, SourceHealth, SourceHealthBook};
use crate::repo::{ItemCounts, ItemRepo, ReadOnlyRunRepo};

pub const STATE_VERSION: i64 = 2;

/// `JSON.stringify(x, null, 2)` 뒤에 줄바꿈 하나. 파일 형식을 여기 한곳에서 정한다.
pub fn pretty_json<T: Serialize>(value: &T) -> KairosResult<String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|_| KairosError::new("json_error"))?;
    text.push('\n');
    Ok(text)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct WouldShowCounts {
    #[serde(rename = "P0")]
    pub p0: i64,
    #[serde(rename = "P1")]
    pub p1: i64,
    #[serde(rename = "P2")]
    pub p2: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CountsView {
    pub objects: i64,
    pub baseline_objects: i64,
    pub events: i64,
    pub would_show: WouldShowCounts,
    pub unclassified: i64,
}

/// 건수만 있고 제목은 없다. badge는 게이트가 닫히면 0이다. 상태 표시줄은 next --json에서 같은 값을 계산한다.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct ItemSummary {
    #[serde(flatten)]
    pub counts: ItemCounts,
    pub badge: i64,
}

/// active는 활성 run, ready는 아직 그리지 않은 세션 카드다. 패널은 같은 값을 next --json의 runs로 받는다.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct RunCounts {
    pub active: i64,
    pub ready: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KairosState {
    pub version: i64,
    pub generated_at: Instant,
    pub paused: bool,
    pub counts: CountsView,
    pub sources: Vec<SourceHealth>,
    /// 표시 시작 시점의 재료. 표면은 gate를 읽는다.
    pub display: DisplayStart,
    /// 지금 표시해도 되는가. 패널의 값은 kairos next가 주고 여기는 감시와 조회용이다.
    pub gate: DisplayGate,
    pub items: ItemSummary,
    pub runs: RunCounts,
}

/// 표시가 켜져 있었다면 나갔을 이벤트와 그 등급.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WouldShowRow {
    pub event: WouldShowEvent,
    pub tier: Tier,
}

/// state.json을 만든다.
#[derive(Clone, Copy, Debug)]
pub struct StateBuilder<'a> {
    connection: &'a Connection,
}

impl<'a> StateBuilder<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 표시가 켜져 있었다면 나갔을 이벤트. T0 이후에 열렸고 아직 닫히지 않은 건만 든다. 등급순이고 같으면 시각순이다.
    pub fn would_show_events(&self) -> KairosResult<Vec<WouldShowRow>> {
        let rows = EventRepo::new(self.connection).would_show()?;
        let mut shown: Vec<WouldShowRow> =
            rows.into_iter().map(|event| WouldShowRow { tier: tier_of(&event.rule_id), event }).collect();
        shown.sort_by_key(|row| row.tier);
        Ok(shown)
    }

    pub fn write_state(state: &KairosState, home: &KairosHome) -> KairosResult<()> {
        home.ensure()?;
        write_file_atomic(&home.state_path(), &pretty_json(state)?)
    }

    /// 파일이 없거나 깨졌으면 None이다. 읽는 쪽은 이를 '모름'으로 다룬다. 내용은 검증하지 않는다.
    pub fn read_state(home: &KairosHome) -> Option<Value> {
        read_json(&home.state_path())
    }

    pub fn write_engine_status(status: &EngineStatus, home: &KairosHome) -> KairosResult<()> {
        write_file_atomic(&home.engine_status_path(), &pretty_json(status)?)
    }

    pub fn read_engine_status(home: &KairosHome) -> Option<EngineStatus> {
        serde_json::from_value(read_json(&home.engine_status_path())?).ok()
    }

    pub fn build(
        &self,
        sources: &[&dyn Source],
        now: Instant,
        control: &KairosControl,
        resumed_at: Option<&str>,
    ) -> KairosResult<KairosState> {
        let invariants = check_invariants(self.connection)?;
        let extra = control.off_date_set();
        let watch_off_dates = OffDateBook::new(self.connection).load_watch_off_dates(&extra)?;
        let mut would_show = WouldShowCounts::default();
        for row in self.would_show_events()? {
            match row.tier {
                Tier::P0 => would_show.p0 += 1,
                Tier::P1 => would_show.p1 += 1,
                Tier::P2 => would_show.p2 += 1,
                Tier::Record => {}
            }
        }
        let display = DisplayStart::load(self.connection, sources, now, control)?;
        let gate = DisplayGate::evaluate(&display, control, now);
        let objects = ObjectRepo::new(self.connection);
        let counts = ItemRepo::new(self.connection).counts(now)?;
        let runs = ReadOnlyRunRepo::new(self.connection);
        Ok(KairosState {
            version: STATE_VERSION,
            generated_at: now,
            paused: control.is_paused(now),
            counts: CountsView {
                objects: objects.count()?,
                baseline_objects: objects.count_baseline()?,
                events: EventRepo::new(self.connection).count()?,
                would_show,
                unclassified: invariants.unclassified,
            },
            sources: SourceHealthBook::new(self.connection).source_health(
                sources,
                now,
                &watch_off_dates,
                resumed_at,
            )?,
            display,
            items: ItemSummary { counts, badge: if gate.open { counts.open_total() } else { 0 } },
            runs: RunCounts { active: runs.active_count()?, ready: runs.unseen_ready_count(gate.open)? },
            gate,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub at: String,
    pub ok: bool,
    /// 코드만 둔다. 외부 문자열을 넣지 않는다.
    pub error_code: Option<String>,
}

fn read_json(path: &std::path::Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    js::parse_json(&String::from_utf8_lossy(&bytes)).ok().filter(|value| !value.is_null())
}
