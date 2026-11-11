//! 수집 한 번의 흐름: 신원과 한도 확인 -> 질의별 검색 -> 노드 읽기 -> 질의별 결과.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use kairos_core::Instant;
use kairos_core::model::{CollectContext, Outcome, SourceName, SourceResult};
use kairos_core::ports::Clock;
use kairos_core::process::ProcessRunner;

use crate::client::GhClient;
use crate::config::GithubConfig;
use crate::defs::{DEFS, OVERLAP_SECONDS, QueryDef, earliest_window, lag_behind, partial_for};
use crate::error::GhError;
use crate::node_parser::NodeParser;
use crate::node_reader::{Fault, NodeReader, NodeReading, READ_LIMIT_MS};
use crate::parallel::{run_all, try_join};
use crate::query::NODES_QUERY;
use crate::search::{Candidates, SearchBudget, search_all};
use crate::self_login;

const MIN_SEARCH_REMAINING: f64 = 8.0;
/// 끝까지 읽지 못한 노드를 넘기고 전진한 질의의 코드. 덮지 못한 구간으로 고지된다(Slack의 record_quarantined).
const NODE_QUARANTINED: &str = "node_quarantined";
/// 다시 읽으면 읽힐 수도 있는 노드라도 이 질의가 다른 질의보다 이만큼 뒤처졌으면 넘긴다(3분 주기 10틱).
/// 넘기지 않으면 watermark와 earliest_window가 멈춘 채 같은 노드를 영영 다시 읽는다.
const STALL_QUARANTINE_MS: i64 = 30 * 60 * 1000;
/// 모든 질의가 함께 멈추면 뒤처짐이 늘지 않는다. 그래도 partial이 이만큼 이어졌으면 넘긴다(3분 주기 40틱).
/// 밤사이 쉰 시간도 세지만, 그 틱에도 읽지 못한 노드만 넘기고 덮지 못한 구간으로 고지한다.
const PARTIAL_QUARANTINE_MS: i64 = 2 * 60 * 60 * 1000;

/// 질의 하나의 검색 결과.
struct Searched {
    since: Option<Instant>,
    found: Result<Candidates, GhError>,
}

pub(crate) struct Collection<'a> {
    pub(crate) runner: &'a Arc<dyn ProcessRunner>,
    pub(crate) clock: &'a dyn Clock,
    pub(crate) config: &'a GithubConfig,
    pub(crate) parser: &'a NodeParser<'a>,
    pub(crate) context: &'a CollectContext,
}

impl Collection<'_> {
    pub(crate) fn run(&self) -> Vec<SourceResult> {
        let gh = match self.open() {
            Ok(gh) => gh,
            Err(results) => return results,
        };
        let searches = self.search(&gh);
        let candidates = union(&searches);
        let reader = NodeReader {
            gh: &gh,
            parser: self.parser,
            cancel: &self.context.cancel,
            clock: self.clock,
            deadline: self.context.now.plus_ms(READ_LIMIT_MS),
        };
        let since = earliest_window(self.context);
        let reading = reader.read(&candidates, since);
        let owners = owners(&searches);
        let read = Read { reading: &reading, owners: &owners, since };
        DEFS.iter().zip(&searches).map(|(def, searched)| self.result(def, searched, &read)).collect()
    }

    /// 토큰을 받고 신원과 검색 한도를 확인한다. 여기서 끝나면 질의마다 같은 실패를 낸다.
    fn open(&self) -> Result<GhClient, Vec<SourceResult>> {
        let fail_all = |outcome, code: &str, identity: Option<&str>| -> Vec<SourceResult> {
            DEFS.iter().map(|def| self.failure(def, outcome, code, identity)).collect()
        };
        let fail_with = |error: GhError| fail_all(error.outcome(), error.code(), None);
        let cancel = &self.context.cancel;
        let gh = GhClient::connect(self.runner.clone(), self.config, cancel.clone()).map_err(fail_with)?;
        let (identity, remaining) =
            try_join(|| gh.graphql(&[], &NODES_QUERY), || gh.rate_remaining(), cancel).map_err(fail_with)?;
        if identity.viewer.as_deref() != Some(self_login()) {
            return Err(fail_all(Outcome::ContractBreak, "identity_mismatch", identity.viewer.as_deref()));
        }
        if remaining < MIN_SEARCH_REMAINING {
            return Err(fail_all(Outcome::RateLimited, "search_budget", None));
        }
        Ok(gh)
    }

    fn search(&self, gh: &GhClient) -> Vec<Searched> {
        let budget = SearchBudget::new();
        let windows: Vec<_> = DEFS.iter().map(|def| (def, def.since(self.context))).collect();
        let runs = run_all(&windows, |(def, since)| search_all(gh, &def.search_text(*since), &budget));
        let unexpected = || Err(GhError::unexpected(&self.context.cancel));
        let found = windows.iter().zip(runs);
        found.map(|((_, since), run)| Searched { since: *since, found: run.unwrap_or_else(unexpected) }).collect()
    }

    fn failure(&self, def: &QueryDef, outcome: Outcome, code: &str, identity: Option<&str>) -> SourceResult {
        SourceResult {
            source: SourceName::Github,
            query: def.id.to_owned(),
            outcome,
            started_at: self.context.now.to_iso(),
            finished_at: self.clock.now().to_iso(),
            pages: 0,
            expected_count: None,
            parsed_count: 0,
            identity: identity.map(str::to_owned),
            window_from: None,
            window_to: None,
            watermark: None,
            progress_at: None,
            thread_reads: Vec::new(),
            overlap_seconds: OVERLAP_SECONDS,
            snapshot: def.snapshot,
            error_code: Some(code.to_owned()),
            objects: Vec::new(),
            events: Vec::new(),
        }
    }

    /// 질의의 결과. 노드의 실패는 그 노드를 후보로 가진 질의만 무르게 한다.
    /// 다시 읽어도 같을 노드는 넘기고 전진한다(node_quarantined). 다시 읽으면 읽힐 수 있는 노드는
    /// partial로 다음 틱에 다시 읽되, 이 질의만 오래 멈췄거나 partial이 오래 이어졌으면 넘긴다.
    fn result(&self, def: &QueryDef, searched: &Searched, read: &Read<'_>) -> SourceResult {
        let entry = match &searched.found {
            Ok(entry) => entry,
            Err(error) => return self.failure(def, error.outcome(), error.code(), Some(self_login())),
        };
        let reading = read.reading;
        let cursor = self.context.cursor(def.id);
        if let Some(error) = reading.error.as_ref().filter(|_| !entry.ids.is_empty()) {
            return self.failure(def, error.outcome(), error.code(), Some(self_login()));
        }

        let stalled = lag_behind(self.context, def.id) >= STALL_QUARANTINE_MS
            || partial_for(self.context, def.id) >= PARTIAL_QUARANTINE_MS;
        let mut objects = Vec::new();
        let mut events = Vec::new();
        let (mut retry, mut skipped, mut unread) = (false, false, false);
        for id in &entry.ids {
            match reading.parsed.get(id) {
                None => unread = true,
                Some(node) if read.owners.get(id.as_str()) == Some(&def.id) => {
                    objects.push(node.object.clone());
                    events.extend(node.events.iter().cloned());
                }
                Some(_) => {}
            }
            match reading.faults.get(id) {
                Some(Fault::Retry) if !stalled => retry = true,
                Some(_) => skipped = true,
                None => {}
            }
        }
        let partial_code = if entry.incomplete {
            Some("search_incomplete")
        } else if entry.truncated {
            Some("search_truncated")
        } else if retry {
            Some("node_unreadable")
        } else {
            None
        };
        let complete = partial_code.is_none();
        let quarantined = complete && skipped;
        let started_at = self.context.now.to_iso();
        SourceResult {
            source: SourceName::Github,
            query: def.id.to_owned(),
            outcome: if complete { Outcome::Ok } else { Outcome::Partial },
            finished_at: self.clock.now().to_iso(),
            pages: entry.pages,
            expected_count: Some(entry.total_count as i64),
            parsed_count: entry.ids.len() as i64,
            identity: Some(self_login().to_owned()),
            // 넘긴 구간을 고지하려고 스냅샷 질의에도 노드를 읽은 기준 시각부터 창을 남긴다.
            window_from: searched.since.or(read.since.filter(|_| quarantined)).map(Instant::to_iso),
            window_to: (searched.since.is_some() || quarantined).then(|| started_at.clone()),
            watermark: complete.then(|| started_at.clone()),
            // 이어진 partial의 첫 시각만 남긴다. 이미 있으면 Ledger가 앞으로 옮기므로 보내지 않는다.
            progress_at: (!complete && cursor.is_none_or(|cursor| cursor.progress_at.is_none()))
                .then(|| started_at.clone()),
            thread_reads: Vec::new(),
            overlap_seconds: OVERLAP_SECONDS,
            // 읽지 못한 후보는 objects에 없다. 스냅샷이라 하면 ok의 부재 판정이 그 객체를 빠진 것으로 센다.
            snapshot: def.snapshot && !unread,
            error_code: partial_code.or(quarantined.then_some(NODE_QUARANTINED)).map(str::to_owned),
            objects,
            events,
            started_at,
        }
    }
}

/// 노드 읽기 한 번의 결과와 질의마다 함께 보는 값.
struct Read<'a> {
    reading: &'a NodeReading,
    owners: &'a HashMap<&'a str, &'a str>,
    /// 잘림을 판정한 기준 시각.
    since: Option<Instant>,
}

/// 질의들의 후보를 앞 질의부터 겹침 없이 잇는다.
fn union(searches: &[Searched]) -> Vec<String> {
    let mut seen = HashSet::new();
    let found = searches.iter().filter_map(|searched| searched.found.as_ref().ok());
    found.flat_map(|entry| &entry.ids).filter(|id| seen.insert(id.as_str())).cloned().collect()
}

/// 후보마다 우선순위가 가장 높은 질의.
fn owners(searches: &[Searched]) -> HashMap<&str, &str> {
    let mut owners = HashMap::new();
    for (def, searched) in DEFS.iter().zip(searches) {
        for id in searched.found.iter().flat_map(|entry| &entry.ids) {
            owners.entry(id.as_str()).or_insert(def.id);
        }
    }
    owners
}
