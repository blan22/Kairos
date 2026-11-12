//! 사용자가 카드와 목록에서 하는 조작.

use kairos_core::text::sanitize_head;
use kairos_core::{Instant, KairosResult, OffDates, string_enum};
use kairos_ledger::{EventRepo, upcoming_deploys};
use kairos_policy::{DeployLinkStatus, ItemReason, WorkSchedule, work_start_minutes};
use kairos_store::{Connection, Database};
use serde::Serialize;
use uuid::Uuid;

use crate::engine::{answer, close_item};
use crate::item::{Item, ItemCloseReason, ItemState, ParkTrigger};
use crate::repo::{ItemRepo, ReadOnlyRunRepo, SuggestionResponse};

pub const TRIAGE_BATCH: usize = 10;

/// 이력 행의 dedupe_key를 유일하게 만드는 값. 시험이 고정값을 넣을 수 있게 분리한다.
pub trait IdSource: Send + Sync {
    fn unique(&self) -> String;
}

/// `randomUUID()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomIds;

impl IdSource for RandomIds {
    fn unique(&self) -> String {
        Uuid::new_v4().to_string()
    }
}

static RANDOM_IDS: RandomIds = RandomIds;

string_enum! {
    /// changed 바꿨다. unchanged 이미 그 상태다. already_closed 이미 닫힌 항목이다. 오래된 카드의 정상 경로다.
    /// not_found 없는 ID다. invalid_state 이 상태에서 할 수 없다. no_deploy 연결된 배포가 없다.
    /// deploy_imminent 이미 배포 시점이라 기다릴 수 없다.
    pub enum ActionStatus {
        Changed => "changed",
        Unchanged => "unchanged",
        AlreadyClosed => "already_closed",
        NotFound => "not_found",
        InvalidState => "invalid_state",
        NoDeploy => "no_deploy",
        DeployImminent => "deploy_imminent",
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ActionResult {
    pub id: i64,
    pub status: ActionStatus,
    pub state: Option<ItemState>,
    pub message: String,
}

impl ActionResult {
    fn of(item: &Item, status: ActionStatus, state: ItemState, message: impl Into<String>) -> Self {
        Self { id: item.id, status, state: Some(state), message: message.into() }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ActionContext<'a> {
    pub now: Instant,
    pub off_dates: &'a OffDates,
}

const HOUR_MS: i64 = 3_600_000;

string_enum! {
    /// 나중에의 때. next_bundle 다음 묶음, hour 한 시간 뒤, tomorrow 다음 근무일의 근무 시작.
    pub enum LaterWhen {
        NextBundle => "next_bundle",
        Hour => "hour",
        Tomorrow => "tomorrow",
    }
}

impl LaterWhen {
    /// 다시 열 시각. 깨어난 항목은 다음 묶음에 실린다.
    fn until(self, context: &ActionContext<'_>) -> Instant {
        match self {
            Self::NextBundle => WorkSchedule::new(context.off_dates).next_bundle_at(context.now),
            Self::Hour => context.now.plus_ms(HOUR_MS),
            Self::Tomorrow => {
                Instant::at_local(context.off_dates.next_workday(context.now.local_date()), work_start_minutes())
            }
        }
    }

    const fn message(self) -> &'static str {
        match self {
            Self::NextBundle => "다음 묶음에 다시 보여 줍니다",
            Self::Hour => "1시간 뒤에 다시 보여 줍니다",
            Self::Tomorrow => "다음 근무일 아침에 다시 보여 줍니다",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TriageDecision {
    Keep,
    Drop,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TriageBatch {
    pub rows: Vec<Item>,
    /// 이 묶음 뒤에 남은 건수.
    pub remaining: usize,
}

/// 트랜잭션을 열지 않는 상태 전이. 호출자가 다른 표의 쓰기와 묶는다.
#[derive(Clone, Copy)]
pub struct ItemTransitions<'a> {
    connection: &'a Connection,
    ids: &'a dyn IdSource,
}

impl<'a> ItemTransitions<'a> {
    pub fn new(connection: &'a Connection) -> Self {
        Self::with_ids(connection, &RANDOM_IDS)
    }

    pub fn with_ids(connection: &'a Connection, ids: &'a dyn IdSource) -> Self {
        Self { connection, ids }
    }

    const fn items(&self) -> ItemRepo<'a> {
        ItemRepo::new(self.connection)
    }

    fn record_change(&self, id: i64, change: &str, now: &str) -> KairosResult<()> {
        EventRepo::new(self.connection).record_item_change(id, change, &self.ids.unique(), now)
    }

    fn guarded(&self, id: i64, act: impl FnOnce(&Item) -> KairosResult<ActionResult>) -> KairosResult<ActionResult> {
        let Some(item) = self.items().get(id)? else {
            return Ok(ActionResult {
                id,
                status: ActionStatus::NotFound,
                state: None,
                message: "항목 없음".to_owned(),
            });
        };
        if item.state == ItemState::Closed {
            return Ok(ActionResult::of(&item, ActionStatus::AlreadyClosed, ItemState::Closed, "이미 처리됨"));
        }
        act(&item)
    }

    /// 4.2의 open -> accepted. 호출자가 runs 쓰기와 같은 트랜잭션에서 쓰도록 트랜잭션을 열지 않는다.
    pub fn accept(&self, id: i64, now: Instant) -> KairosResult<ActionResult> {
        self.guarded(id, |item| {
            if item.state != ItemState::Open {
                let message = "열린 항목만 수락할 수 있습니다";
                return Ok(ActionResult::of(item, ActionStatus::InvalidState, item.state, message));
            }
            let at = now.to_iso();
            self.items().accept(item.id)?;
            answer(self.connection, item.id, SuggestionResponse::Accept, &at)?;
            self.record_change(item.id, "accept", &at)?;
            Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Accepted, "수락했습니다"))
        })
    }

    /// 4.2의 accepted -> open. run 실패나 사용자 취소에 쓴다.
    pub fn release(&self, id: i64, now: Instant) -> KairosResult<ActionResult> {
        self.guarded(id, |item| {
            if item.state != ItemState::Accepted {
                let message = "수락한 항목이 아닙니다";
                return Ok(ActionResult::of(item, ActionStatus::InvalidState, item.state, message));
            }
            self.items().reopen(item.id)?;
            self.record_change(item.id, "release", &now.to_iso())?;
            Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Open, "되돌렸습니다"))
        })
    }

    fn later(&self, id: i64, context: &ActionContext<'_>, when: LaterWhen) -> KairosResult<ActionResult> {
        self.guarded(id, |item| {
            if item.state == ItemState::Accepted {
                return Ok(ActionResult::of(item, ActionStatus::InvalidState, item.state, "세션이 진행 중입니다"));
            }
            if item.state == ItemState::Parked {
                return Ok(ActionResult::of(item, ActionStatus::Unchanged, item.state, "이미 대기 중입니다"));
            }
            let at = context.now.to_iso();
            self.items().park_for_bundle(item.id, &when.until(context).to_iso())?;
            answer(self.connection, item.id, SuggestionResponse::Later, &at)?;
            self.record_change(item.id, "later", &at)?;
            Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Parked, when.message()))
        })
    }

    fn done(&self, id: i64, now: Instant, drop: bool) -> KairosResult<ActionResult> {
        self.guarded(id, |item| {
            let at = now.to_iso();
            let (reason, response, change, message) = if drop {
                (ItemCloseReason::UserDropped, SuggestionResponse::Drop, "drop", "버렸습니다")
            } else {
                (ItemCloseReason::UserDone, SuggestionResponse::Done, "done", "끝냈습니다")
            };
            close_item(self.connection, item.id, reason, None, &at, response)?;
            self.record_change(item.id, change, &at)?;
            let live = ReadOnlyRunRepo::new(self.connection).has_live_session(item.id)?;
            let message = if live { format!("{message}. 세션은 열려 있습니다") } else { message.to_owned() };
            Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Closed, message))
        })
    }

    fn keep_until_deploy(&self, item: &Item, context: &ActionContext<'_>) -> KairosResult<ActionResult> {
        let deploys = upcoming_deploys(self.connection, context.now)?;
        let deploy = deploys.iter().find(|deploy| {
            deploy.link.status == DeployLinkStatus::Linked && deploy.link.object_keys.contains(&item.object_key)
        });
        let Some(deploy) = deploy else {
            return Ok(ActionResult::of(item, ActionStatus::NoDeploy, item.state, "연결된 배포가 없습니다"));
        };
        let until = Instant::at_local(context.off_dates.previous_workday(deploy.date), work_start_minutes());
        if until <= context.now {
            return Ok(ActionResult::of(item, ActionStatus::DeployImminent, item.state, "이미 배포 시점입니다"));
        }
        let at = context.now.to_iso();
        self.items().park_for_deploy(item.id, &until.to_iso())?;
        answer(self.connection, item.id, SuggestionResponse::Keep, &at)?;
        self.record_change(item.id, "keep_deploy", &at)?;
        Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Parked, "배포 전 근무일에 다시 보여 줍니다"))
    }

    /// 남김: 답으로 끝나지 않는 일을 work_owed로 남기고 원래 항목은 증거가 올 때까지 조용히 둔다.
    fn keep_as_work(&self, item: &Item, now: Instant) -> KairosResult<ActionResult> {
        if matches!(item.reason, ItemReason::WorkOwed | ItemReason::Manual) {
            return Ok(ActionResult::of(item, ActionStatus::Unchanged, item.state, "이미 할 일입니다"));
        }
        let at = now.to_iso();
        if self.items().live(&item.object_key, ItemReason::WorkOwed)?.is_none() {
            self.items().insert_work_owed(item, &at)?;
        }
        self.items().park_kept(item.id)?;
        answer(self.connection, item.id, SuggestionResponse::Keep, &at)?;
        self.record_change(item.id, "keep", &at)?;
        Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Parked, "할 일로 남겼습니다"))
    }

    fn keep(&self, id: i64, context: &ActionContext<'_>, until_deploy: bool) -> KairosResult<ActionResult> {
        self.guarded(id, |item| {
            if item.state != ItemState::Open {
                let message = "열린 항목만 남길 수 있습니다";
                return Ok(ActionResult::of(item, ActionStatus::InvalidState, item.state, message));
            }
            if until_deploy { self.keep_until_deploy(item, context) } else { self.keep_as_work(item, context.now) }
        })
    }

    fn triage_decide(&self, id: i64, decision: TriageDecision, now: Instant) -> KairosResult<ActionResult> {
        self.guarded(id, |item| {
            if item.park_trigger != Some(ParkTrigger::Triage) {
                let message = "기준선 대기 항목이 아닙니다";
                return Ok(ActionResult::of(item, ActionStatus::InvalidState, item.state, message));
            }
            let at = now.to_iso();
            if decision == TriageDecision::Drop {
                let reason = ItemCloseReason::UserDropped;
                close_item(self.connection, item.id, reason, None, &at, SuggestionResponse::Drop)?;
                self.record_change(item.id, "triage_drop", &at)?;
                return Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Closed, "버렸습니다"));
            }
            // 유지한 건은 목록에 남지만 baseline이라 새 사건 전에는 제안하지 않는다.
            self.items().keep_triaged(item.id)?;
            self.record_change(item.id, "triage_keep", &at)?;
            Ok(ActionResult::of(item, ActionStatus::Changed, ItemState::Open, "유지합니다"))
        })
    }
}

/// 트랜잭션을 여는 사용자 조작. 여러 ID는 한 트랜잭션이다.
#[derive(Clone, Copy)]
pub struct ItemActions<'a> {
    database: &'a Database,
    ids: &'a dyn IdSource,
}

impl<'a> ItemActions<'a> {
    pub fn new(database: &'a Database) -> Self {
        Self::with_ids(database, &RANDOM_IDS)
    }

    pub fn with_ids(database: &'a Database, ids: &'a dyn IdSource) -> Self {
        Self { database, ids }
    }

    fn each(
        &self,
        ids: &[i64],
        act: impl Fn(ItemTransitions<'_>, i64) -> KairosResult<ActionResult>,
    ) -> KairosResult<Vec<ActionResult>> {
        self.database.transaction(|connection| {
            let transitions = ItemTransitions::with_ids(connection, self.ids);
            ids.iter().map(|id| act(transitions, *id)).collect()
        })
    }

    /// 나중에: 다음 묶음으로 보낸다. 응답한 노출이어도 밀어내기 1회로 센다.
    pub fn later(&self, ids: &[i64], context: &ActionContext<'_>) -> KairosResult<Vec<ActionResult>> {
        self.later_when(ids, context, LaterWhen::NextBundle)
    }

    /// 나중에의 때를 고른다. 어느 때든 밀어내기 1회로 세고, 깨어나면 다음 묶음에 실린다.
    pub fn later_when(
        &self,
        ids: &[i64],
        context: &ActionContext<'_>,
        when: LaterWhen,
    ) -> KairosResult<Vec<ActionResult>> {
        self.each(ids, |transitions, id| transitions.later(id, context, when))
    }

    /// `drop`이면 끝이 아니라 버리기다.
    pub fn done(&self, ids: &[i64], now: Instant, drop: bool) -> KairosResult<Vec<ActionResult>> {
        self.each(ids, |transitions, id| transitions.done(id, now, drop))
    }

    /// `until_deploy`면 할일로 남김 대신 배포 때 다시.
    pub fn keep(
        &self,
        ids: &[i64],
        context: &ActionContext<'_>,
        until_deploy: bool,
    ) -> KairosResult<Vec<ActionResult>> {
        self.each(ids, |transitions, id| transitions.keep(id, context, until_deploy))
    }

    /// 사용자가 쓴 제목을 저장만 한다. 어떤 명령이나 세션에도 넘기지 않는다.
    pub fn add(&self, title: &str, now: Instant) -> KairosResult<ActionResult> {
        self.database.transaction(|connection| {
            let at = now.to_iso();
            let key = format!("manual:{}", self.ids.unique());
            let id = ItemRepo::new(connection).insert_manual(&key, &sanitize_head(title), &at)?;
            ItemTransitions::with_ids(connection, self.ids).record_change(id, "add", &at)?;
            Ok(ActionResult {
                id,
                status: ActionStatus::Changed,
                state: Some(ItemState::Open),
                message: "추가했습니다".to_owned(),
            })
        })
    }

    /// 기준선 객체를 항목 ID가 있는 대기 행으로 만들어 10건씩 낸다. 고를 때까지 어디에도 표시되지 않는다.
    pub fn triage_batch(&self, now: Instant) -> KairosResult<TriageBatch> {
        self.database.transaction(|connection| {
            let items = ItemRepo::new(connection);
            let at = now.to_iso();
            let candidates = items.triage_candidates()?;
            let room = TRIAGE_BATCH.saturating_sub(items.pending_triage()?.len());
            for candidate in candidates.iter().take(room) {
                items.insert_triage(candidate, &at)?;
            }
            Ok(TriageBatch { rows: items.pending_triage()?, remaining: candidates.len().saturating_sub(room) })
        })
    }

    pub fn triage_decide(
        &self,
        ids: &[i64],
        decision: TriageDecision,
        now: Instant,
    ) -> KairosResult<Vec<ActionResult>> {
        self.each(ids, |transitions, id| transitions.triage_decide(id, decision, now))
    }
}
