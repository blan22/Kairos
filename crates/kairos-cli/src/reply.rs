//! 명령의 답. `--json`을 주면 표준 출력은 언제나 JSON 한 덩어리다.
//! 종료 코드는 0 성공, 1 실행 실패(없는 ID 포함), 2 사용법 오류다. 형식은 `CONTRACT_VERSION`을 따른다.

use kairos_agenda::{ActionResult, CONTRACT_VERSION};
use kairos_core::KairosResult;
use kairos_sessions::SessionOutcome;
use serde_json::{Map, Value, json};

use crate::app::AppContext;

const OK_STATUSES: [&str; 3] = ["changed", "unchanged", "already_closed"];

fn is_ok(status: &str) -> bool {
    OK_STATUSES.contains(&status)
}

/// `{ schema, ok, ...fields }`.
fn body(ok: bool, fields: Value) -> Map<String, Value> {
    let mut body = Map::new();
    body.insert("schema".to_owned(), Value::from(CONTRACT_VERSION));
    body.insert("ok".to_owned(), Value::from(ok));
    if let Value::Object(fields) = fields {
        body.extend(fields);
    }
    body
}

/// 답의 형식을 쥔다.
#[derive(Clone, Copy)]
pub struct Reply<'a> {
    context: &'a AppContext,
    json: bool,
}

impl<'a> Reply<'a> {
    pub const fn new(context: &'a AppContext, json: bool) -> Self {
        Self { context, json }
    }

    pub const fn json(&self) -> bool {
        self.json
    }

    pub fn fail(&self, code: &str, message: &str, exit_code: i32) -> i32 {
        if self.json {
            let answer = body(false, json!({ "error": { "code": code, "message": message } }));
            // 문자열과 숫자뿐이라 직렬화가 실패하지 않는다.
            let _ = self.context.console().emit(&answer);
        } else {
            self.context.console().print(message);
        }
        exit_code
    }

    pub fn usage(&self, message: &str) -> i32 {
        self.fail("usage", message, 2)
    }

    /// `{ schema, ok: true, ...fields }`를 찍는다.
    pub fn success(&self, fields: Value) -> KairosResult<i32> {
        self.context.console().emit(&body(true, fields))?;
        Ok(0)
    }

    pub fn actions(&self, results: &[ActionResult]) -> KairosResult<i32> {
        self.context.refresh_state()?;
        let failed = results.iter().find(|result| !is_ok(result.status.as_str()));
        if self.json {
            let mut answer = body(failed.is_none(), json!({ "results": results }));
            if let Some(failed) = failed {
                answer.insert("error".to_owned(), json!({ "code": failed.status, "message": failed.message }));
            }
            self.context.console().emit(&answer)?;
        } else {
            for result in results {
                self.context.console().print(&format!("#{} {}", result.id, result.message));
            }
        }
        Ok(i32::from(failed.is_some()))
    }

    pub fn session(&self, value: &SessionOutcome) -> KairosResult<i32> {
        self.context.refresh_state()?;
        let SessionOutcome { result, run, url, warning, launch } = value;
        let ok = is_ok(result.status.as_str());
        if self.json {
            let mut fields = json!({ "results": [result], "run": run, "url": url, "warning": warning });
            // git 세션 호스트에만 있다. v1 필드는 그대로다.
            if let Some(launch) = launch {
                fields["launch"] = json!(launch);
            }
            let mut answer = body(ok, fields);
            if !ok {
                answer.insert("error".to_owned(), json!({ "code": result.status, "message": result.message }));
            }
            self.context.console().emit(&answer)?;
        } else {
            self.context.console().print(&format!("#{} {}", result.id, result.message));
            if let Some(warning) = warning.as_deref().filter(|warning| !warning.is_empty()) {
                self.context.console().print(warning);
            }
            if let Some(launch) = launch {
                self.context.console().print(&launch.command);
            }
        }
        Ok(i32::from(!ok))
    }
}
