use std::collections::BTreeSet;

use kairos_core::model::{NormalizedEvent, Outcome, SourceName, SourceResult};
use kairos_core::text::sanitize_text;
use kairos_core::{Instant, KairosError, KairosResult, OffDates};
use kairos_store::{Connection, Database};

use crate::cleaning::{FIELD_LIMIT, canonical_time, cleaned_event};
use crate::closure_sweeper::ClosureSweeper;
use crate::contract_guard::{ContractGuard, Verdict};
use crate::cursor_keeper::CursorKeeper;
use crate::deploy_watch::{DeployWatch, upcoming_deploys};
use crate::event_recorder::{DAY_MS, EVENT_RETENTION_DAYS, EventContext, EventRecorder};
use crate::object_store::{ObjectRun, ObjectStore};
use crate::off_dates::OffDateBook;
use crate::repo::{CursorRepo, NewRun, SourceRunRepo, ThreadReadRepo};

/// 사용자가 건너뛴 구간의 source_run 코드.
pub const MANUAL_SKIP: &str = "manual_skip";
/// 이 시간만큼 앞으로 물려 메일 날짜 경계를 덮는다.
const REREAD_MARGIN_MS: i64 = 60 * 60 * 1000;

#[derive(Clone, Debug)]
pub struct IngestOptions {
    /// 판정 시각.
    pub now: Instant,
    /// day_off 일정 밖의 쉬는 날. kairos off로 지정한 날을 넘긴다.
    pub extra_off_dates: OffDates,
}

impl IngestOptions {
    pub fn at(now: Instant) -> Self {
        Self { now, extra_off_dates: OffDates::new() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestReport {
    pub run_id: i64,
    /// 검증 뒤 실제로 기록된 값. 소스가 낸 값과 다를 수 있다.
    pub outcome: Outcome,
    pub error_code: Option<String>,
    pub objects_stored: usize,
    pub events_inserted: usize,
    pub events_duplicate: usize,
    pub closed: usize,
    pub watermark_advanced: bool,
    pub t0_established: bool,
}

fn insert_run(connection: &Connection, result: &SourceResult, verdict: &Verdict, now: Instant) -> KairosResult<i64> {
    let clean = |text: &str| sanitize_text(text, FIELD_LIMIT);
    let query = clean(&result.query);
    let identity = result.identity.as_deref().map(clean);
    let error_code = verdict.error_code.as_deref().map(clean);
    let at = now.to_iso();
    let stamp = |value: &str| if ContractGuard::is_timestamp(value) { value.to_owned() } else { at.clone() };
    let window = |value: &Option<String>| value.clone().filter(|text| ContractGuard::is_timestamp(text));
    let (started_at, finished_at) = (stamp(&result.started_at), stamp(&result.finished_at));
    let (window_from, window_to) = (window(&result.window_from), window(&result.window_to));
    SourceRunRepo::new(connection).insert(&NewRun {
        source: result.source,
        query: &query,
        started_at: &started_at,
        finished_at: &finished_at,
        outcome: verdict.outcome,
        pages: result.pages,
        expected_count: result.expected_count,
        parsed_count: result.parsed_count,
        identity: identity.as_deref(),
        window_from: window_from.as_deref(),
        window_to: window_to.as_deref(),
        error_code: error_code.as_deref(),
    })
}

/// 결과 하나의 저장. 한 트랜잭션 안에서 돈다.
fn store(
    connection: &Connection,
    result: &SourceResult,
    verdict: &Verdict,
    now: Instant,
    extra_off_dates: &OffDates,
) -> KairosResult<IngestReport> {
    let run_id = insert_run(connection, result, verdict, now)?;
    let mut report = IngestReport {
        run_id,
        outcome: verdict.outcome,
        error_code: verdict.error_code.clone(),
        objects_stored: 0,
        events_inserted: 0,
        events_duplicate: 0,
        closed: 0,
        watermark_advanced: false,
        t0_established: false,
    };
    if !verdict.store {
        return Ok(report);
    }

    let source = result.source;
    let query = result.query.as_str();
    let is_ok = verdict.outcome == Outcome::Ok;
    let keeper = CursorKeeper::new(connection);
    let cursor = CursorRepo::new(connection).get(source, query)?;
    let prior_t0 = cursor.as_ref().and_then(|cursor| cursor.t0.clone());
    let first_run_at = cursor.as_ref().and_then(|cursor| cursor.first_run_at.clone());
    let before_ok = prior_t0.is_none();
    let baseline_run = before_ok && first_run_at.is_none();
    let started_at = canonical_time(&result.started_at)?;
    // 첫 ok가 늦어도 기준은 저장한 첫 실행이다. 그 뒤에 온 사건은 첫 ok 전이어도 새 사건이다.
    let cutoff = prior_t0.or(first_run_at).unwrap_or_else(|| started_at.clone());
    if before_ok {
        keeper.mark_first_run(result, &started_at)?;
    }

    let objects = ObjectStore::new(connection);
    let run = ObjectRun { query, baseline: before_ok, run_id, now };
    let mut pending: Vec<NormalizedEvent> = Vec::new();
    for raw in &result.objects {
        pending.extend(objects.store(raw, &run)?);
        report.objects_stored += 1;
    }
    if is_ok && result.snapshot {
        pending.extend(objects.mark_missing(source, query, run_id, now)?);
    }
    pending.extend(result.events.iter().map(cleaned_event));
    pending.sort_by_cached_key(|event| Instant::parse(&event.occurred_at));

    let deploy_linked: BTreeSet<String> =
        upcoming_deploys(connection, now)?.into_iter().flat_map(|deploy| deploy.link.object_keys).collect();
    let context = EventContext { run_id, now, baseline_run, before_ok, t0: Instant::parse(&cutoff), deploy_linked };
    let recorder = EventRecorder::new(connection);
    for event in &pending {
        if recorder.record(event, &context)? {
            report.events_inserted += 1;
        } else {
            report.events_duplicate += 1;
        }
    }
    keeper.record_thread_reads(result, &now.to_iso())?;
    if !is_ok {
        keeper.advance_progress(result)?;
    }

    if is_ok {
        report.t0_established = before_ok;
        report.watermark_advanced = keeper.advance(result, &cutoff)?;
        let off_dates = OffDateBook::new(connection).load_off_dates(extra_off_dates)?;
        report.events_inserted += DeployWatch::new(connection).evaluate(source, &context, &off_dates)?;
        let sweeper = ClosureSweeper::new(connection);
        report.closed = sweeper.sweep(source, now)?;
        // 배포 위험은 calendar 행이다. GitHub ok가 낸 deploy_cleared도 여기서 닫는다.
        if source == SourceName::Github {
            report.closed += sweeper.sweep(SourceName::Calendar, now)?;
        }
    }
    Ok(report)
}

/// 수집 기록의 지휘자.
#[derive(Clone, Copy, Debug)]
pub struct Ledger<'a> {
    database: &'a Database,
}

impl<'a> Ledger<'a> {
    pub const fn new(database: &'a Database) -> Self {
        Self { database }
    }

    /// `SourceResult` 하나를 한 트랜잭션으로 저장한다.
    /// - ok만 watermark를 전진하고 T0를 세우고 종료를 판정한다.
    /// - partial은 읽은 만큼 저장한다. dedupe_key가 있어 다음 재수집이 멱등이다. progress_at만 전진한다.
    /// - 나머지는 source_run만 남긴다.
    ///
    /// 저장이 실패하면 전부 되돌리고 실패 행만 남긴다. 호출자는 다음 소스를 계속 진행할 수 있다.
    pub fn ingest(&self, result: SourceResult, options: &IngestOptions) -> KairosResult<IngestReport> {
        let now = options.now;
        let verdict = ContractGuard::judge(&result);
        let stored =
            self.database.transaction(|connection| store(connection, &result, &verdict, now, &options.extra_off_dates));
        stored.or_else(|_| {
            let failed = Verdict::fail(Outcome::ContractBreak, "ingest_failed");
            self.database.transaction(|connection| store(connection, &result, &failed, now, &OffDates::new()))
        })
    }

    /// 사용자가 고른 시각까지 질의의 커서를 옮긴다. 다음 조회는 until부터 시작한다.
    /// ok가 아니라 partial 행으로 남겨 건강을 좋게 꾸미지 않고, 건너뛴 구간은 doctor에 드러난다.
    pub fn skip_cursor(&self, source: SourceName, query: &str, until: Instant, now: Instant) -> KairosResult<bool> {
        self.database.transaction(|connection| {
            let cursors = CursorRepo::new(connection);
            let cursor = cursors.get(source, query)?;
            let Some(cursor) = cursor.filter(|cursor| cursor.watermark.as_deref().is_some_and(|text| !text.is_empty()))
            else {
                return Ok(false);
            };
            if cursor.watermark.as_deref().and_then(Instant::parse).is_some_and(|watermark| until <= watermark) {
                return Ok(false);
            }
            let at = now.to_iso();
            let run = SourceResult {
                source,
                query: query.to_owned(),
                outcome: Outcome::Partial,
                started_at: at.clone(),
                finished_at: at,
                pages: 0,
                expected_count: None,
                parsed_count: 0,
                identity: None,
                window_from: cursor.watermark.clone(),
                window_to: Some(until.to_iso()),
                watermark: None,
                progress_at: None,
                thread_reads: Vec::new(),
                overlap_seconds: cursor.overlap_seconds,
                snapshot: false,
                error_code: Some(MANUAL_SKIP.to_owned()),
                objects: Vec::new(),
                events: Vec::new(),
            };
            let verdict = Verdict { outcome: Outcome::Partial, error_code: Some(MANUAL_SKIP.to_owned()), store: false };
            insert_run(connection, &run, &verdict, now)?;
            let watermark = until.plus_ms(cursor.overlap_seconds * 1000).to_iso();
            cursors.set_watermark(source, query, &watermark)?;
            Ok(true)
        })
    }

    /// 파서를 고친 뒤 부르는 복구. 형식을 몰라 저장한 메일과 격리한 메일을 다음 틱부터 다시 읽게 한다.
    /// 매 틱 자동으로 다시 읽지 않는 이유: 파서가 그대로면 결과도 같고, 그동안 읽기 몫만 쓴다.
    pub fn reread_notion(&self, now: Instant) -> KairosResult<usize> {
        self.database.transaction(|connection| {
            let reads = ThreadReadRepo::new(connection);
            let targets = reads.notion_reread_targets()?;
            if targets.is_empty() {
                return Ok(0);
            }
            for (external_id, _) in &targets {
                reads.forget_notion(external_id)?;
            }
            // 메일 검색의 창을 넘겨 물리면 다시 읽지도 못하면서 window_capped만 남는다.
            let floor = now.minus_ms((EVENT_RETENTION_DAYS - 1) * DAY_MS);
            let times: Option<Vec<Instant>> =
                targets.iter().map(|(_, at)| at.as_deref().map_or(Some(now), Instant::parse)).collect();
            // 시각 하나라도 해석할 수 없으면 TS에서는 NaN이 퍼져 toISOString이 던진다.
            let oldest =
                times.and_then(|times| times.into_iter().min()).ok_or_else(|| KairosError::new("invalid_time"))?;
            let from = oldest.minus_ms(REREAD_MARGIN_MS).max(floor).to_iso();
            CursorRepo::new(connection).rewind_notion(&from)?;
            Ok(targets.len())
        })
    }
}
