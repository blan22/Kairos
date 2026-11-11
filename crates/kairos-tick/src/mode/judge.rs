use kairos_core::js;
use kairos_core::model::Outcome;
use serde_json::Value;

use super::ClaudeRun;

pub const DROP_RESULT: &str = "Prompt dropped by a hook: collected-without-model";

#[derive(Clone, Debug, PartialEq)]
pub struct RunVerdict {
    pub ok: bool,
    pub outcome: Outcome,
    pub error_code: Option<String>,
    /// 모델이 불렸거나 비용이 생겼다. 다음 틱부터 수집을 멈춘다.
    pub spend: bool,
    pub num_turns: Option<f64>,
    pub cost_usd: Option<f64>,
}

fn failed(outcome: Outcome, code: &str) -> RunVerdict {
    RunVerdict { ok: false, outcome, error_code: Some(code.to_owned()), spend: false, num_turns: None, cost_usd: None }
}

/// 스파이크 결과: is_error=false, num_turns=0, 비용 0, drop 문구일 때만 정상이다.
/// 출력이 JSON `null`이어도 값이 없는 출력과 같이 지출로 본다. 차단은 닫힌 쪽으로 실패해야 한다.
pub fn judge_claude_run(run: &ClaudeRun) -> RunVerdict {
    if run.missing {
        return failed(Outcome::ToolMissing, "claude_missing");
    }
    if run.timed_out {
        return failed(Outcome::Timeout, "claude_timeout");
    }
    let parsed = match js::parse_json(&run.stdout) {
        Ok(parsed) => parsed,
        // JS는 범위를 넘는 숫자를 Infinity로 읽어 지출로 본다. 파싱 실패로 두면 정지 표식 없이 지나간다.
        Err(error) if error.to_string().contains("number out of range") => {
            let code = Some("model_called".to_owned());
            let outcome = Outcome::ContractBreak;
            return RunVerdict { ok: false, outcome, error_code: code, spend: true, num_turns: None, cost_usd: None };
        }
        Err(_) => return failed(Outcome::ContractBreak, "claude_output_unparsable"),
    };
    let num_turns = parsed.get("num_turns").and_then(Value::as_f64);
    let cost_usd = parsed.get("total_cost_usd").and_then(Value::as_f64);
    // 값이 없거나 형식이 다르면 모델이 불리지 않았다고 확신할 수 없으므로 지출 쪽으로 본다.
    let spend = num_turns != Some(0.0) || cost_usd != Some(0.0);
    let verdict = |ok: bool, outcome: Outcome, code: Option<&str>| RunVerdict {
        ok,
        outcome,
        error_code: code.map(str::to_owned),
        spend,
        num_turns,
        cost_usd,
    };
    if spend {
        return verdict(false, Outcome::ContractBreak, Some("model_called"));
    }
    let dropped = parsed.get("is_error") == Some(&Value::Bool(false))
        && parsed.get("result").and_then(Value::as_str) == Some(DROP_RESULT)
        && run.exit_code == Some(0);
    if !dropped {
        return verdict(false, Outcome::ContractBreak, Some("drop_contract_broken"));
    }
    verdict(true, Outcome::Ok, None)
}
