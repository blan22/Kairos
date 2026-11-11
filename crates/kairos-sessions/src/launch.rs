//! run ID와 터미널 명령. `--command` 값은 Orca가 셸에 입력하는 유일한 문자열이다.
//! run ID는 정규식을 통과한 값만, 경로는 작은따옴표로 감싼 값만 들어간다.

use std::sync::LazyLock;

use kairos_core::{KairosError, KairosResult, js};
use regex::Regex;

pub static RUN_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$").expect("상수 정규식")
});

const SHORT_ID: usize = 8;

fn short_id(run_id: &str) -> String {
    js::slice_utf16(run_id, SHORT_ID)
}

pub fn worktree_name(run_id: &str) -> String {
    format!("t-{}", short_id(run_id))
}

pub fn terminal_title(run_id: &str) -> String {
    format!("kairos-{}", short_id(run_id))
}

/// 상수만 받는다. 셸에 입력되는 문자열이라 따옴표로 묶는다.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// `run_binary`는 설치본의 `bin/kairos-run` 절대 경로다. 호출자가 넘긴다.
pub fn launch_command(run_binary: &str, run_id: &str) -> KairosResult<String> {
    if !RUN_ID.is_match(run_id) {
        return Err(KairosError::new("invalid_run_id"));
    }
    Ok(format!("{} {run_id}", shell_quote(run_binary)))
}
