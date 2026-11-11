use kairos_agenda::EngineReport;
use kairos_core::model::{Outcome, SourceName};
use kairos_ops::{SLEEP_INTERRUPTED, ToolResultsSweep};
use serde::{Serialize, Serializer};

use crate::halt::js_number;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectStatus {
    Ran,
    Busy,
    Halted,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutcomeRow {
    pub source: SourceName,
    pub query: String,
    pub outcome: Outcome,
    pub error_code: Option<String>,
}

impl OutcomeRow {
    /// 시간 초과만 잠자기 코드로 바꾼다. outcome은 그대로라 cursor와 재시도 판정이 달라지지 않는다.
    pub(super) fn mark_slept(&mut self) {
        if self.outcome == Outcome::Timeout {
            self.error_code = Some(SLEEP_INTERRUPTED.to_owned());
        }
    }
}

fn number<S: Serializer>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
    js_number(*value).serialize(serializer)
}

/// claude 실행 1회의 판정.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeReport {
    #[serde(serialize_with = "number")]
    pub num_turns: Option<f64>,
    #[serde(serialize_with = "number")]
    pub cost_usd: Option<f64>,
    pub elapsed_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectReport {
    pub status: CollectStatus,
    pub reason: Option<String>,
    /// 이 프로세스가 기록한 소스 질의별 outcome.
    pub outcomes: Vec<OutcomeRow>,
    /// 수집 모드가 `kairos ingest`로 직접 기록한 소스 질의별 outcome.
    pub mode_outcomes: Vec<OutcomeRow>,
    /// claude 실행 1회의 판정. 이번 틱에 띄우지 않았으면 None이다.
    pub mode: Option<ModeReport>,
    pub contract_test: bool,
    pub halted: bool,
    /// 끝에서 지운 큰 응답 파일과 남은 파일 수. 정리를 못 돌렸으면 None이다.
    pub tool_results: Option<ToolResultsSweep>,
    /// 항목 엔진의 결과. 실패했거나 돌지 않았으면 None이다.
    pub items: Option<EngineReport>,
    /// 이번 틱에 전달한 P0 알림 건수.
    pub notified: usize,
    /// 이번 틱에 전달한 P1 알림에 든 항목 건수.
    #[serde(rename = "notifiedP1")]
    pub notified_p1: usize,
    pub panel_opened: bool,
    /// 앞선 알림에 묶여 따로 울리지 않은 P1.
    #[serde(rename = "mergedP1")]
    pub merged_p1: usize,
    /// 알리지 않기로 해 표시만 남긴 P1.
    #[serde(rename = "droppedP1")]
    pub dropped_p1: usize,
    /// 전달에 실패해 다시 시도할 P1.
    #[serde(rename = "failedP1")]
    pub failed_p1: usize,
    /// 이번 틱이 Mac 잠자기에 끊겼다. 그 틱의 timeout은 고장이 아니다.
    pub slept: bool,
}

impl CollectReport {
    pub(super) const fn new() -> Self {
        Self {
            status: CollectStatus::Ran,
            reason: None,
            outcomes: Vec::new(),
            mode_outcomes: Vec::new(),
            mode: None,
            contract_test: false,
            halted: false,
            tool_results: None,
            items: None,
            notified: 0,
            notified_p1: 0,
            panel_opened: false,
            merged_p1: 0,
            dropped_p1: 0,
            failed_p1: 0,
            slept: false,
        }
    }

    pub(super) fn mark_slept(&mut self) {
        self.outcomes.iter_mut().chain(&mut self.mode_outcomes).for_each(OutcomeRow::mark_slept);
    }
}
