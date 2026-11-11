//! halt.json. 수집기가 모델을 불러 비용이 생긴 경우 등, 사람이 확인하기 전까지 수집을 멈추는 표식.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use kairos_agenda::pretty_json;
use kairos_core::home::write_file_atomic;
use kairos_core::ports::Clock;
use kairos_core::{KairosHome, KairosResult, js};
use serde_json::{Number, Value, json};

/// 읽지 못한 표식의 사유.
const UNREADABLE: &str = "halt_unreadable";

/// `String(number)`. 금액과 턴 수를 줄에 찍을 때 쓴다. -0은 0이다.
pub fn js_number_text(value: f64) -> String {
    format!("{}", value + 0.0)
}

/// `JSON.stringify`의 숫자. 정수 값은 소수점 없이 나간다.
pub(crate) fn js_number(value: Option<f64>) -> Value {
    let Some(value) = value else {
        return Value::Null;
    };
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        return Value::from(value as i64);
    }
    Number::from_f64(value).map_or(Value::Null, Value::Number)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Halt {
    /// 다른 프로그램이 쓴 파일이면 없을 수 있다.
    pub at: Option<String>,
    pub reason: String,
    pub num_turns: Option<f64>,
    pub cost_usd: Option<f64>,
}

/// 정지 표식의 읽기와 쓰기.
#[derive(Clone)]
pub struct HaltFlag {
    path: PathBuf,
    clock: Arc<dyn Clock>,
}

impl HaltFlag {
    pub fn new(home: &KairosHome, clock: Arc<dyn Clock>) -> Self {
        Self { path: home.file("halt.json"), clock }
    }

    fn unreadable(&self) -> Halt {
        Halt { at: Some(self.clock.now().to_iso()), reason: UNREADABLE.to_owned(), num_turns: None, cost_usd: None }
    }

    /// 파일이 없을 때만 정지가 아니다. 읽지 못한 표식은 정지로 본다. 모델 호출을 막는 장치는 닫힌 쪽으로 실패해야 한다.
    pub fn read(&self) -> Option<Halt> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(_) => return Some(self.unreadable()),
        };
        let parsed = js::parse_json(&String::from_utf8_lossy(&bytes)).ok();
        let reason = parsed.as_ref().and_then(|halt| halt.get("reason")).and_then(Value::as_str);
        let Some(reason) = reason else {
            return Some(self.unreadable());
        };
        let number = |key: &str| parsed.as_ref().and_then(|halt| halt.get(key)).and_then(Value::as_f64);
        Some(Halt {
            at: parsed.as_ref().and_then(|halt| halt.get("at")).and_then(Value::as_str).map(str::to_owned),
            reason: reason.to_owned(),
            num_turns: number("numTurns"),
            cost_usd: number("costUsd"),
        })
    }

    pub fn write(&self, halt: &Halt) -> KairosResult<()> {
        let value = json!({
            "at": halt.at,
            "reason": halt.reason,
            "numTurns": js_number(halt.num_turns),
            "costUsd": js_number(halt.cost_usd),
        });
        write_file_atomic(&self.path, &pretty_json(&value)?)
    }

    /// 표식을 지운다. 있었으면 참이다.
    pub fn clear(&self) -> KairosResult<bool> {
        let existed = self.path.exists();
        match fs::remove_file(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(existed),
        }
    }
}
