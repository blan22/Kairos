use serde::{Deserialize, Serialize};

use super::{NormalizedEvent, NormalizedObject, Outcome, SourceName};
use crate::string_enum;

string_enum! {
    /// read 읽어서 처리함, failed 읽지 못함, quarantined 반복 실패로 격리해 더 읽지 않음.
    pub enum ThreadReadOutcome {
        Read => "read",
        Failed => "failed",
        Quarantined => "quarantined",
    }
}

/// 후속 읽기 대상 하나의 기록. message_count는 저장한 메시지 수이고 아직 못 읽었으면 None이다.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadState {
    pub external_id: String,
    pub message_count: Option<i64>,
    pub failures: i64,
    pub first_failed_at: Option<String>,
    pub quarantined_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ThreadReadReport {
    pub external_id: String,
    pub outcome: ThreadReadOutcome,
    pub message_count: Option<i64>,
}

/// 질의 1회의 결과. 소스 1개가 질의마다 하나씩 낸다. 시각은 모두 ISO 8601이고 오프셋이 있어야 한다.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceResult {
    pub source: SourceName,
    /// 질의 식별자. `QuerySpec::id`와 같다. cursor와 source_run의 키다.
    pub query: String,
    pub outcome: Outcome,
    pub started_at: String,
    pub finished_at: String,
    pub pages: i64,
    /// 응답이 밝힌 전체 건수. 모르면 None. ok인데 parsed_count와 다르면 Ledger가 parse_error로 바꾼다.
    pub expected_count: Option<i64>,
    pub parsed_count: i64,
    /// 응답에서 실제로 확인한 신원. `SourceName::self_identity`와 다르면 Ledger가 contract_break로 바꾼다.
    pub identity: Option<String>,
    pub window_from: Option<String>,
    pub window_to: Option<String>,
    /// ok일 때만 쓰이는 다음 watermark 후보. 과거로는 움직이지 않는다.
    pub watermark: Option<String>,
    /// partial에서 전진하는 `CursorState::progress_at` 후보.
    pub progress_at: Option<String>,
    /// 후속 읽기의 결과. ok와 partial에서 Ledger가 thread_read에 남긴다.
    pub thread_reads: Vec<ThreadReadReport>,
    pub overlap_seconds: i64,
    /// 참이면 objects가 이 질의의 전체 스냅샷이다. 빠진 객체의 missing_runs가 ok 실행에서만 늘어난다.
    pub snapshot: bool,
    /// 로그용 오류 코드. 외부 문자열을 넣지 않는다.
    pub error_code: Option<String>,
    pub objects: Vec<NormalizedObject>,
    pub events: Vec<NormalizedEvent>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CursorState {
    pub source: SourceName,
    pub query: String,
    pub watermark: Option<String>,
    pub overlap_seconds: i64,
    /// 이 질의의 기준 시각. 저장한 첫 실행의 시작이고 첫 ok에 세운다. 이전 항목은 baseline=1이다.
    pub t0: Option<String>,
    /// 저장한 첫 실행(partial 포함)의 시작 시각. 첫 ok 전의 창과 baseline의 기준이다.
    pub first_run_at: Option<String>,
    /// partial 중에 여기까지는 다 읽었다는 표시. watermark와 별개이고 ok가 지운다.
    pub progress_at: Option<String>,
}

impl CursorState {
    /// 조회 시작 시각. watermark - overlap이고, 첫 실행이면 None이다.
    pub fn window_start(&self) -> Option<crate::Instant> {
        let watermark = crate::Instant::parse(self.watermark.as_deref()?)?;
        Some(watermark.minus_ms(self.overlap_seconds * 1000))
    }
}
