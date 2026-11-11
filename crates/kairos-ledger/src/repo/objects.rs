use std::str::FromStr;

use kairos_core::KairosResult;
use kairos_core::model::{NormalizedObject, ObjectKind, SourceName, TrustTier};
use kairos_policy::{DeployCandidate, RuleObject};
use kairos_store::row::{enum_column, scalar, sql};
use kairos_store::{Connection, OptionalExtension, Row, params, rusqlite};

use super::required;

/// ok 스냅샷에서 이 횟수만큼 연속으로 빠지면 일정의 철회 증거를 만든다.
const WITHDRAWN_AFTER_MISSING_RUNS: i64 = 2;

const UPSERT: &str = "INSERT INTO objects (
    object_key, source, kind, external_id, permalink, actor_id, channel_id, ts, text_head, trust_tier,
    all_day, start_date, end_date, start_at, end_at, series_id, attendee_count, my_response, busy, state,
    milestone_title, assigned_to_me, authored_by_me, review_requested_me, is_draft, review_decision,
    checks_state, head_ref_oid, query, snapshot_hash, baseline, first_seen_at, last_seen_run, missing_runs
  ) VALUES (
    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20,
    ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29, ?30, ?31, ?32, ?33, ?34
  )
  ON CONFLICT (object_key) DO UPDATE SET
    source = excluded.source, kind = excluded.kind, external_id = excluded.external_id,
    permalink = excluded.permalink, actor_id = excluded.actor_id, channel_id = excluded.channel_id,
    ts = excluded.ts, text_head = excluded.text_head, trust_tier = excluded.trust_tier,
    all_day = excluded.all_day, start_date = excluded.start_date, end_date = excluded.end_date,
    start_at = excluded.start_at, end_at = excluded.end_at, series_id = excluded.series_id,
    attendee_count = excluded.attendee_count, my_response = excluded.my_response, busy = excluded.busy,
    state = excluded.state, milestone_title = excluded.milestone_title,
    assigned_to_me = excluded.assigned_to_me, authored_by_me = excluded.authored_by_me,
    review_requested_me = excluded.review_requested_me, is_draft = excluded.is_draft,
    review_decision = excluded.review_decision, checks_state = excluded.checks_state,
    head_ref_oid = excluded.head_ref_oid, query = excluded.query, snapshot_hash = excluded.snapshot_hash,
    last_seen_run = excluded.last_seen_run, missing_runs = excluded.missing_runs";

/// objects 표의 한 행.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectRow {
    pub object_key: String,
    pub source: SourceName,
    pub query: String,
    pub kind: ObjectKind,
    pub external_id: String,
    pub snapshot_hash: String,
    pub permalink: Option<String>,
    pub actor_id: Option<String>,
    pub channel_id: Option<String>,
    pub ts: Option<String>,
    pub text_head: Option<String>,
    pub trust_tier: Option<TrustTier>,
    pub all_day: Option<bool>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub series_id: Option<String>,
    pub attendee_count: Option<i64>,
    pub my_response: Option<String>,
    pub busy: Option<bool>,
    pub state: Option<String>,
    pub milestone_title: Option<String>,
    pub assigned_to_me: Option<bool>,
    pub authored_by_me: Option<bool>,
    pub review_requested_me: Option<bool>,
    pub is_draft: Option<bool>,
    pub review_decision: Option<String>,
    pub checks_state: Option<String>,
    pub head_ref_oid: Option<String>,
    pub baseline: bool,
    pub first_seen_at: String,
    pub last_seen_run: i64,
    pub missing_runs: i64,
}

/// 저장된 0과 1을 TS의 `=== 1`로 읽는다.
fn flag(row: &Row<'_>, column: &str) -> rusqlite::Result<Option<bool>> {
    Ok(row.get::<_, Option<i64>>(column)?.map(|value| value == 1))
}

fn enum_at<T: FromStr>(row: &Row<'_>, column: &str) -> rusqlite::Result<T> {
    let index = row.as_ref().column_index(column)?;
    enum_column(&row.get::<_, String>(index)?, index)
}

impl ObjectRow {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self> {
        let trust_tier = row.get::<_, Option<String>>("trust_tier")?;
        Ok(Self {
            object_key: row.get("object_key")?,
            source: enum_at(row, "source")?,
            query: row.get("query")?,
            kind: enum_at(row, "kind")?,
            external_id: row.get("external_id")?,
            snapshot_hash: row.get("snapshot_hash")?,
            permalink: row.get("permalink")?,
            actor_id: row.get("actor_id")?,
            channel_id: row.get("channel_id")?,
            ts: row.get("ts")?,
            text_head: row.get("text_head")?,
            trust_tier: trust_tier.map(|tier| enum_column(&tier, 0)).transpose()?,
            all_day: flag(row, "all_day")?,
            start_date: row.get("start_date")?,
            end_date: row.get("end_date")?,
            start_at: row.get("start_at")?,
            end_at: row.get("end_at")?,
            series_id: row.get("series_id")?,
            attendee_count: row.get("attendee_count")?,
            my_response: row.get("my_response")?,
            busy: flag(row, "busy")?,
            state: row.get("state")?,
            milestone_title: row.get("milestone_title")?,
            assigned_to_me: flag(row, "assigned_to_me")?,
            authored_by_me: flag(row, "authored_by_me")?,
            review_requested_me: flag(row, "review_requested_me")?,
            is_draft: flag(row, "is_draft")?,
            review_decision: row.get("review_decision")?,
            checks_state: row.get("checks_state")?,
            head_ref_oid: row.get("head_ref_oid")?,
            baseline: row.get::<_, i64>("baseline")? == 1,
            first_seen_at: row.get("first_seen_at")?,
            last_seen_run: row.get("last_seen_run")?,
            missing_runs: row.get("missing_runs")?,
        })
    }

    pub fn to_rule_object(&self) -> RuleObject {
        RuleObject {
            kind: self.kind,
            state: self.state.clone(),
            all_day: self.all_day == Some(true),
            start_date: self.start_date.clone(),
            end_date: self.end_date.clone(),
            start_at: self.start_at.clone(),
            end_at: self.end_at.clone(),
            my_response: self.my_response.clone(),
            busy: self.busy,
            assigned_to_me: self.assigned_to_me == Some(true),
            authored_by_me: self.authored_by_me == Some(true),
        }
    }
}

/// `NormalizedObject` 밖에서 저장에 더하는 값.
pub(crate) struct ObjectStamp<'a> {
    pub query: &'a str,
    pub hash: &'a str,
    pub baseline: bool,
    pub first_seen_at: &'a str,
    pub run_id: i64,
}

/// 쉬는 날을 어느 일정에서 읽을지.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DayOffScope {
    All,
    /// 다른 사람이 쓸 수 있는 공유 캘린더(crew_off 질의)를 뺀다.
    ExceptShared,
    OnlyShared,
}

#[derive(Clone, Copy, Debug)]
pub struct ObjectRepo<'a> {
    connection: &'a Connection,
}

impl<'a> ObjectRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    /// 빈 키는 없는 객체다.
    pub fn get(&self, key: &str) -> KairosResult<Option<ObjectRow>> {
        if key.is_empty() {
            return Ok(None);
        }
        self.connection
            .query_row("SELECT * FROM objects WHERE object_key = ?", params![key], ObjectRow::from_row)
            .optional()
            .map_err(sql)
    }

    pub fn count(&self) -> KairosResult<i64> {
        scalar(self.connection, "SELECT count(*) FROM objects", [])
    }

    pub fn count_baseline(&self) -> KairosResult<i64> {
        scalar(self.connection, "SELECT count(*) FROM objects WHERE baseline = 1", [])
    }

    /// 빠지지 않은 객체의 종류별 (종류, 전체, 기준선) 건수. kairos list의 보드 줄이다.
    pub fn kind_counts(&self) -> KairosResult<Vec<(String, i64, i64)>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT kind, count(*), sum(baseline) FROM objects
                 WHERE missing_runs = 0 GROUP BY kind ORDER BY kind",
            )
            .map_err(sql)?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)
    }

    /// 포커스를 끝낼 수 있는 회의의 (시작, 처음 본 시각). 거절, 취소, 한가함 표시, 종일 일정과 빠진 일정은 뺀다.
    pub fn focus_stopping_meetings(&self) -> KairosResult<Vec<(String, String)>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT start_at, first_seen_at FROM objects
                 WHERE kind = 'meeting' AND start_at IS NOT NULL AND coalesce(all_day, 0) = 0 AND missing_runs = 0
                   AND coalesce(state, '') != 'cancelled' AND coalesce(my_response, '') != 'declined' AND coalesce(busy, 1) != 0",
            )
            .map_err(sql)?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    pub(crate) fn upsert(&self, object: &NormalizedObject, stamp: &ObjectStamp<'_>) -> KairosResult<()> {
        self.connection
            .execute(
                UPSERT,
                params![
                    required(&object.object_key),
                    object.source.as_str(),
                    object.kind.as_str(),
                    required(&object.external_id),
                    object.permalink,
                    object.actor_id,
                    object.channel_id,
                    object.ts,
                    object.text_head,
                    object.trust_tier.map(TrustTier::as_str),
                    object.all_day,
                    object.start_date,
                    object.end_date,
                    object.start_at,
                    object.end_at,
                    object.series_id,
                    object.attendee_count,
                    object.my_response,
                    object.busy,
                    object.state,
                    object.milestone_title,
                    object.assigned_to_me,
                    object.authored_by_me,
                    object.review_requested_me,
                    object.is_draft,
                    object.review_decision,
                    object.checks_state,
                    object.head_ref_oid,
                    stamp.query,
                    stamp.hash,
                    stamp.baseline,
                    stamp.first_seen_at,
                    stamp.run_id,
                    0,
                ],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// 이 실행에서 보지 못한 객체의 부재 횟수를 올린다.
    pub(crate) fn count_missing(&self, source: SourceName, query: &str, run_id: i64) -> KairosResult<()> {
        self.connection
            .execute(
                "UPDATE objects SET missing_runs = missing_runs + 1 WHERE source = ? AND query = ? AND last_seen_run != ?",
                params![source.as_str(), query, run_id],
            )
            .map_err(sql)?;
        Ok(())
    }

    /// 철회 기준에 막 닿은 일정.
    pub(crate) fn newly_missing_schedules(&self, source: SourceName, query: &str) -> KairosResult<Vec<ObjectRow>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT * FROM objects WHERE source = ? AND query = ? AND missing_runs = ?
                 AND kind IN ('meeting', 'deploy') AND coalesce(state, '') != 'cancelled'",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map(params![source.as_str(), query, WITHDRAWN_AFTER_MISSING_RUNS], ObjectRow::from_row)
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// day_off 일정의 시작과 끝. 삭제된 연차는 ok 스냅샷 2회 부재로 빠진다.
    pub(crate) fn day_off_spans(&self, scope: DayOffScope) -> KairosResult<Vec<(Option<String>, Option<String>)>> {
        let filter = match scope {
            DayOffScope::All => "1",
            DayOffScope::ExceptShared => "query != 'crew_off'",
            DayOffScope::OnlyShared => "query = 'crew_off'",
        };
        let text = format!(
            "SELECT start_date, end_date FROM objects WHERE kind = 'day_off' AND all_day = 1
             AND missing_runs < ? AND coalesce(state, '') != 'cancelled' AND {filter}"
        );
        let mut statement = self.connection.prepare(&text).map_err(sql)?;
        let rows = statement
            .query_map(params![WITHDRAWN_AFTER_MISSING_RUNS], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 최근 스냅샷에 있는 GitHub 객체만 쓴다. 빠진 객체를 열린 건으로 세지 않는다.
    pub(crate) fn deploy_candidates(&self) -> KairosResult<Vec<DeployCandidate>> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT object_key, kind, state, text_head, milestone_title, assigned_to_me, authored_by_me
                 FROM objects WHERE source = 'github' AND missing_runs = 0",
            )
            .map_err(sql)?;
        let rows = statement
            .query_map([], |row| {
                Ok(DeployCandidate {
                    object_key: row.get("object_key")?,
                    kind: enum_at(row, "kind")?,
                    state: row.get("state")?,
                    text_head: row.get("text_head")?,
                    milestone_title: row.get("milestone_title")?,
                    assigned_to_me: flag(row, "assigned_to_me")? == Some(true),
                    authored_by_me: flag(row, "authored_by_me")? == Some(true),
                })
            })
            .map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }

    /// 최근 스냅샷에 있고 취소되지 않은 [배포] 일정.
    pub(crate) fn live_deploys(&self) -> KairosResult<Vec<ObjectRow>> {
        let mut statement = self
            .connection
            .prepare("SELECT * FROM objects WHERE kind = 'deploy' AND coalesce(state, '') != 'cancelled' AND missing_runs = 0")
            .map_err(sql)?;
        let rows = statement.query_map([], ObjectRow::from_row).map_err(sql)?;
        rows.collect::<rusqlite::Result<_>>().map_err(sql)
    }
}
