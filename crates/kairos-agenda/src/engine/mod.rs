//! 항목 엔진과 상태 기계(설계 4.2~4.4). 외부 명령이나 모델을 부르지 않고 저장된 events만 읽는다.
//!
//! 항목과 이벤트의 관계
//! - 처분이 opened이고 baseline이 아닌 이벤트 하나가 (object_key, reason) 항목에 붙는다. 객체가 없으면 thread_key가 키다.
//! - 종료 판정은 ledger의 `ClosureSweeper`가 ok 실행에서 이벤트별로 내린다. 항목은 붙은 이벤트가 모두 닫히면 닫힌다.
//!   그래서 outcome=ok 조건과 양성 증거 조건을 다시 구현하지 않는다.
//! - 붙은 이벤트가 없는 기준선 항목만 `ClosureJudge`를 직접 부른다.
//!
//! 등급은 규칙에서만 온다. 이 모듈의 어떤 함수도 모델 출력을 입력으로 받지 않고, 사용자 입력으로 P0를 만들 수 없다.

mod assign;
mod close;
mod sweep;

use std::collections::BTreeSet;

use kairos_core::{Instant, KairosResult, OffDates};
use kairos_store::{Connection, Database};
use serde::Serialize;

pub(crate) use close::{answer, close_item};
pub(crate) use sweep::{expire_cards, hold_cards, wake_parked};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub struct EngineReport {
    pub opened: usize,
    pub folded: usize,
    pub closed: usize,
    pub woken: usize,
    pub lifted: usize,
    pub expired: usize,
    /// 예외로 건너뛴 이벤트. 다음 틱에 다시 시도한다.
    pub skipped: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct RefreshInput<'a> {
    pub now: Instant,
    pub off_dates: &'a OffDates,
    /// 이번 틱에 ok가 있는 소스. 이벤트 종료를 거치지 않는 종료의 조건이다.
    pub ok_sources: &'a BTreeSet<String>,
    /// 지금 카드가 보류로 가려져 있다. 켜지면 시간 만료 대신 카드를 내린다.
    pub cards_held: bool,
}

/// 저장된 이벤트로 항목을 갱신한다.
#[derive(Clone, Copy, Debug)]
pub struct ItemEngine<'a> {
    database: &'a Database,
}

impl<'a> ItemEngine<'a> {
    pub const fn new(database: &'a Database) -> Self {
        Self { database }
    }

    /// 한 트랜잭션이고 다시 불러도 같은 결과다.
    pub fn refresh(&self, input: &RefreshInput<'_>) -> KairosResult<EngineReport> {
        self.database.transaction(|connection| refresh(connection, input))
    }
}

fn refresh(connection: &Connection, input: &RefreshInput<'_>) -> KairosResult<EngineReport> {
    let now = input.now.to_iso();
    let mut report = EngineReport { woken: wake_parked(connection, &now)?, ..EngineReport::default() };
    assign::assign_events(connection, &now, input.ok_sources, &mut report)?;
    report.lifted += assign::lift_deploy_linked(connection, input.now, input.off_dates)?;
    report.closed += close::close_resolved(connection, input.now, input.ok_sources)?;
    if input.cards_held {
        hold_cards(connection, &now)?;
    } else {
        report.expired = expire_cards(connection, input.now)?;
    }
    Ok(report)
}
