use kairos_agenda::{ActionStatus, ItemRepo, ItemTransitions, ReadyKind};
use kairos_core::{Instant, KairosResult};
use kairos_store::Database;

use super::{FailCode, RunRepo};

/// 트랜잭션을 여는 run의 끝. 항목과 함께 움직인다.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunLifecycle<'a> {
    database: &'a Database,
}

impl<'a> RunLifecycle<'a> {
    pub(crate) const fn new(database: &'a Database) -> Self {
        Self { database }
    }

    /// run을 실패로 내리고 항목을 연다(4.2의 accepted -> open). 다시 제안되도록 다음 묶음에 싣는다.
    /// 이미 내려간 run이면 아무것도 하지 않는다. 그 항목은 지금 새 run이 쓰고 있을 수 있다. 활성 run은 항목마다 하나라 다른 run을 볼 필요가 없다.
    /// `fail_code`는 사용자가 응답을 받지 못하는 경로에서 준다. 실패 카드 1회가 된다.
    pub(crate) fn fail(&self, id: &str, item_id: i64, now: Instant, fail_code: Option<FailCode>) -> KairosResult<()> {
        let at = now.to_iso();
        self.database.transaction(|connection| {
            let runs = RunRepo::new(connection);
            if runs.mark_failed(id, &at)? == 0 {
                return Ok(());
            }
            if let Some(code) = fail_code {
                runs.record_failure(id, code, &at)?;
            }
            if ItemTransitions::new(connection).release(item_id, now)?.status == ActionStatus::Changed {
                ItemRepo::new(connection).queue_for_bundle(item_id)?;
            }
            Ok(())
        })
    }

    pub(crate) fn end(&self, id: &str, now: Instant) -> KairosResult<()> {
        let at = now.to_iso();
        self.database.transaction(|connection| {
            let runs = RunRepo::new(connection);
            if runs.mark_ended(id, &at)? > 0 {
                runs.mark_ready(id, ReadyKind::Ended, &at)?;
            }
            Ok(())
        })
    }
}
