//! 후보를 GraphQL로 읽어 해석한다.
//! 실패는 그 노드의 것으로만 남긴다. 한 묶음이 실패해도 다른 묶음의 읽기를 버리지 않는다.

use std::collections::HashMap;
use std::time::Duration;

use kairos_core::Instant;
use kairos_core::model::Outcome;
use kairos_core::ports::{CancelToken, Clock};
use serde_json::{Map, Value};

use crate::client::{CALL_TIMEOUT, GhClient, GraphqlPage};
use crate::error::GhError;
use crate::node_parser::{NodeParser, ParsedNode};
use crate::parallel::run_all;
use crate::query::{EXPANDED_QUERY, NODES_QUERY};
use crate::self_login;
use crate::truncation::truncated_connections;

/// GraphQL 읽기 단위. 응답 시간이 객체 수에 비례한다(측정: 50개 7초, 25개 5초, 15개 4초).
/// 50개씩 3병렬은 소스 상한 20초를 넘긴 틱이 있었다. 작게 나눠 한 번에 읽는다.
const NODE_BATCH: usize = 20;
const NODE_CONCURRENCY: usize = 8;
/// 잘린 노드를 다시 읽는 단위. 스레드까지 넓게 읽어 한 노드가 무겁다.
const EXPAND_BATCH: usize = 10;
/// 노드 읽기를 끝낼 시각. 수집 시작부터 잰다. 소스 상한(kairos-tick의 SOURCE_LIMIT_MS, 20초)보다 2초 앞이라
/// 호출이 시간 초과로 끝나도 결과를 만들어 돌려줄 수 있다. 그러지 못하면 모든 질의가 source_timeout이다.
pub(crate) const READ_LIMIT_MS: i64 = 18_000;
/// 실패한 묶음을 나눠 다시 읽는 호출의 한 수집 상한. 실패한 묶음은 SPLIT_WAYS 조각으로 한꺼번에 나눈다.
/// 나쁜 노드 하나를 찾는 데 20개 묶음은 10번(7조각, 남은 3개를 하나씩), 10개 묶음은 7번(5조각, 둘을 하나씩) 더 든다.
/// 둘이 한 번씩 겹쳐도 남는다. 반씩 나누면 단계가 다섯이라 느린 실패(시간 초과)는 한 수집 안에 끝까지 못 나눈다.
/// 단계는 대개 셋이라 처음 읽기가 남은 시간의 절반, 다음 단계가 그 절반을 써도 마지막 단계에 시간이 남는다.
/// 어느 단계든 노드 읽기를 끝낼 시각을 넘겨 부르지 않는다.
const SPLIT_CALLS: usize = 24;
const SPLIT_WAYS: usize = NODE_CONCURRENCY;
/// 나눠 읽기 한 단계를 시작할 남은 시간의 하한. 처음 읽기는 CALL_MIN_LEFT_MS만 있으면 부른다.
const SPLIT_MIN_LEFT_MS: i64 = 3_000;
const CALL_MIN_LEFT_MS: i64 = 1_000;
/// 혼자 읽어도 이 사유로 실패하면 다시 읽어도 같다. 출력 상한과 노드 수는 응답의 내용으로만 정해진다.
/// 형식 오류(graphql_shape, gh_bad_json)와 시간 초과는 GitHub 쪽 일시 실패에서도 나서 넣지 않는다.
const HOPELESS_CODES: [&str; 2] = ["gh_output_too_large", "node_count"];

/// 노드를 끝까지 읽지 못한 사정. 한 노드에 여럿이면 뒤의 것(다시 읽을 이유가 큰 것)이 남는다.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Fault {
    /// 넓게 읽어도 고정 창 밖에 새 활동이 남았다. 같은 since로 다시 읽어도 같다. 읽은 만큼은 쓴다.
    Cut,
    /// 혼자 읽어도 응답이 상한을 넘거나 형식이 틀렸다. 다시 읽어도 같다.
    Hopeless,
    /// 호출 실패, 오류가 붙은 응답, null이나 해석하지 못한 노드. 다음 틱에 읽힐 수 있다.
    Retry,
}

#[derive(Default)]
pub(crate) struct NodeReading {
    /// 해석한 노드. 여기 없는 후보는 faults에 사정이 있다.
    pub(crate) parsed: HashMap<String, ParsedNode>,
    /// 끝까지 읽지 못한 노드와 그 사정.
    pub(crate) faults: HashMap<String, Fault>,
    /// 노드와 무관하게 이번 읽기 전체를 믿을 수 없게 한 실패. 신원 불일치, 한도, 인증, gh 없음, 취소다.
    pub(crate) error: Option<GhError>,
}

pub(crate) struct NodeReader<'a> {
    pub(crate) gh: &'a GhClient,
    pub(crate) parser: &'a NodeParser<'a>,
    pub(crate) cancel: &'a CancelToken,
    pub(crate) clock: &'a dyn Clock,
    /// 노드 읽기를 끝낼 시각. 호출마다 이때까지 남은 시간만 준다.
    pub(crate) deadline: Instant,
}

/// 실패 없이 읽었거나 더 나누지 못한 묶음 하나.
struct Piece {
    ids: Vec<String>,
    read: Result<GraphqlPage, GhError>,
}

fn verify(page: &GraphqlPage, chunk: &[String]) -> Result<(), GhError> {
    if page.viewer.as_deref() != Some(self_login()) {
        return Err(GhError::new(Outcome::ContractBreak, "identity_mismatch"));
    }
    if page.nodes.len() != chunk.len() {
        return Err(GhError::new(Outcome::ParseError, "node_count"));
    }
    Ok(())
}

/// 특정 노드와 무관한 실패. 묶음을 나눠도 같은 실패라 이번 읽기 전체를 멈춘다.
fn is_global(error: &GhError) -> bool {
    matches!(error.outcome(), Outcome::ContractBreak | Outcome::RateLimited | Outcome::Auth | Outcome::ToolMissing)
}

/// 묶음의 어느 노드 때문일 수 있는 실패. 나눠 읽으면 그 노드로 좁혀진다. 무거운 노드는 대개 느리게 실패한다.
fn is_splittable(error: &GhError) -> bool {
    matches!(error.outcome(), Outcome::ParseError | Outcome::Timeout)
}

/// 뒤에 읽은 연결로 같은 이름의 연결을 바꾼다. 기존 키의 자리는 그대로이고 새 키는 뒤에 붙는다.
/// 부분 실패한 응답은 연결이 null로 온다. 덮으면 읽어 둔 댓글이 사라지므로 앞의 값을 지킨다.
fn merge(raw: &mut Map<String, Value>, id: &str, fresh: Value) {
    if let (Some(Value::Object(current)), Value::Object(fresh)) = (raw.get_mut(id), fresh) {
        current.extend(fresh.into_iter().filter(|(_, value)| !value.is_null()));
    }
}

fn note(faults: &mut HashMap<String, Fault>, id: &str, fault: Fault) {
    let kept = faults.entry(id.to_owned()).or_insert(fault);
    *kept = (*kept).max(fault);
}

/// errors를 path가 가리킨 노드의 것으로 남긴다. 어느 노드인지 모르는 오류는 묶음 전체의 것이다.
fn note_errors(faults: &mut HashMap<String, Fault>, page: &GraphqlPage, ids: &[String]) {
    for index in &page.errors {
        match index.and_then(|index| ids.get(index)) {
            Some(id) => note(faults, id, Fault::Retry),
            None => ids.iter().for_each(|id| note(faults, id, Fault::Retry)),
        }
    }
}

/// 더 나누지 못한 실패. 노드 하나만 읽어도 내용 때문에 실패했으면 다시 읽지 않을 노드다.
fn note_failure(faults: &mut HashMap<String, Fault>, error: &GhError, ids: &[String]) {
    let hopeless = ids.len() == 1 && HOPELESS_CODES.contains(&error.code());
    let fault = if hopeless { Fault::Hopeless } else { Fault::Retry };
    ids.iter().for_each(|id| note(faults, id, fault));
}

impl NodeReader<'_> {
    pub(crate) fn read(&self, ids: &[String], since: Option<Instant>) -> NodeReading {
        let mut reading = NodeReading::default();
        if let Err(error) = self.fill(ids, since, &mut reading) {
            reading.error = Some(error);
        }
        // 소스 상한으로 취소됐으면 노드별 실패가 아니다. 이 결과는 어차피 버려진다.
        if self.cancel.is_cancelled() {
            reading.error = Some(GhError::unexpected(self.cancel));
        }
        reading
    }

    fn fill(&self, ids: &[String], since: Option<Instant>, reading: &mut NodeReading) -> Result<(), GhError> {
        let faults = &mut reading.faults;
        let mut splits = SPLIT_CALLS;
        let mut found = HashMap::new();
        for piece in self.read_pieces(ids, NODE_BATCH, &NODES_QUERY, &mut splits)? {
            match piece.read {
                Ok(page) => {
                    note_errors(faults, &page, &piece.ids);
                    found.extend(piece.ids.into_iter().zip(page.nodes));
                }
                Err(error) => note_failure(faults, &error, &piece.ids),
            }
        }
        // 삽입 순서를 지키는 맵이다(serde_json의 preserve_order). 다시 읽기의 묶음이 이 순서를 따른다.
        let mut raw: Map<String, Value> = ids.iter().filter_map(|id| Some((id.clone(), found.remove(id)?))).collect();
        self.expand(&mut raw, since, &mut splits, faults)?;
        for (id, node) in &raw {
            match self.parser.parse(node) {
                Some(parsed) => {
                    reading.parsed.insert(id.clone(), parsed);
                }
                None => note(faults, id, Fault::Retry),
            }
        }
        Ok(())
    }

    /// 창이 잘린 노드를 넓게 다시 읽어 raw를 고친다. 그래도 잘린 노드는 Cut으로 남긴다.
    /// 다시 읽기의 부분 실패도 그 노드의 사정으로 남긴다. 놓치면 ok로 watermark가 넘어가 그 댓글을 다시 읽지 못한다.
    fn expand(
        &self,
        raw: &mut Map<String, Value>,
        since: Option<Instant>,
        splits: &mut usize,
        faults: &mut HashMap<String, Fault>,
    ) -> Result<(), GhError> {
        let truncated = |node: &Value| !truncated_connections(node, since).is_empty();
        let targets: Vec<String> = raw.iter().filter(|(_, node)| truncated(node)).map(|(id, _)| id.clone()).collect();
        for piece in self.read_pieces(&targets, EXPAND_BATCH, &EXPANDED_QUERY, splits)? {
            match piece.read {
                Ok(page) => {
                    note_errors(faults, &page, &piece.ids);
                    for (id, fresh) in piece.ids.iter().zip(page.nodes) {
                        merge(raw, id, fresh);
                    }
                }
                Err(error) => note_failure(faults, &error, &piece.ids),
            }
        }
        for id in targets.iter().filter(|id| raw.get(id.as_str()).is_some_and(truncated)) {
            note(faults, id, Fault::Cut);
        }
        Ok(())
    }

    /// ids를 batch씩 읽는다. 형식 오류나 시간 초과로 실패한 묶음은 조각으로 나눠 다시 읽어 실패를 그 노드로 좁힌다.
    /// 나눠 읽기는 splits 호출과 남은 시간 안에서만 한다. 더 나누지 못한 묶음은 실패한 채로 돌려준다.
    /// 노드와 무관한 실패만 읽기 전체의 실패다.
    fn read_pieces(
        &self,
        ids: &[String],
        batch: usize,
        query: &str,
        splits: &mut usize,
    ) -> Result<Vec<Piece>, GhError> {
        let mut level: Vec<Vec<String>> = ids.chunks(batch).map(<[String]>::to_vec).collect();
        let mut min_left_ms = CALL_MIN_LEFT_MS;
        let mut pieces = Vec::new();
        while !level.is_empty() {
            let mut parts = Vec::new();
            for group in level.chunks(NODE_CONCURRENCY) {
                if self.cancel.is_cancelled() {
                    return Err(GhError::unexpected(self.cancel));
                }
                let Some(reads) = self.call_all(group, query, min_left_ms, *splits) else {
                    let late = || Err(GhError::new(Outcome::Timeout, "gh_timeout"));
                    pieces.extend(group.iter().map(|chunk| Piece { ids: chunk.clone(), read: late() }));
                    continue;
                };
                for (chunk, read) in group.iter().zip(reads) {
                    let read = read.and_then(|page| verify(&page, chunk).map(|()| page));
                    match read {
                        Err(error) if is_global(&error) => return Err(error),
                        Err(error) if is_splittable(&error) && chunk.len() > 1 && *splits >= 2 => {
                            let size = chunk.len().div_ceil(SPLIT_WAYS.min(*splits));
                            let before = parts.len();
                            parts.extend(chunk.chunks(size).map(<[String]>::to_vec));
                            *splits -= parts.len() - before;
                        }
                        read => pieces.push(Piece { ids: chunk.clone(), read }),
                    }
                }
            }
            level = parts;
            min_left_ms = SPLIT_MIN_LEFT_MS;
        }
        Ok(pieces)
    }

    /// 묶음들을 동시에 읽는다. 호출마다 노드 읽기를 끝낼 시각까지 남은 시간만 준다.
    /// 실패하면 나눠 읽을 묶음이 있으면 남은 시간의 절반만 준다. 시간 초과로 끝나도 나머지 절반으로
    /// 같은 수집 안에서 나눠 읽어, 느린 노드와 같은 묶음이던 노드도 이번에 읽힌다.
    /// 남은 시간이 min_left_ms보다 적으면 부르지 않고 None이다. 그 묶음은 더 나누지 않는다.
    fn call_all(
        &self,
        group: &[Vec<String>],
        query: &str,
        min_left_ms: i64,
        splits: usize,
    ) -> Option<Vec<Result<GraphqlPage, GhError>>> {
        let left_ms = self.deadline.ms_since(self.clock.now());
        if left_ms < min_left_ms {
            return None;
        }
        let splittable = splits >= 2 && group.iter().any(|chunk| chunk.len() > 1);
        let budget_ms = if splittable { left_ms / 2 } else { left_ms };
        let timeout = Duration::from_millis(u64::try_from(budget_ms).unwrap_or(0)).min(CALL_TIMEOUT);
        let reads = run_all(group, |chunk| self.gh.graphql_within(chunk, query, timeout));
        Some(reads.into_iter().map(|read| read.unwrap_or_else(|| Err(GhError::unexpected(self.cancel)))).collect())
    }
}
