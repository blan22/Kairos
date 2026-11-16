//! 수집 훅의 허용 목록. 설치본의 플러그인 폴더에 이 컴퓨터의 바인딩으로 쓴다.

use std::fs;
use std::path::Path;

use kairos_core::KairosResult;
use kairos_core::platform::set_mode;

/// 수집 훅의 허용 목록 파일. 설치본의 플러그인 폴더에 바인딩으로 다시 쓴다.
const ALLOWED_CALLS_FILE: &str = "hooks/allowed.ts";

/// 이 컴퓨터의 바인딩으로 만든 허용 목록. 값은 JSON 문자열로 넣어 TS 문자열 리터럴이 된다.
pub fn collector_allowlist() -> String {
    use kairos_core::connector::{CALENDAR_EVENTS, CAPABILITIES, MAIL_SEARCH, MAIL_THREAD, SLACK_SEARCH};
    let quote = |text: &str| serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned());
    let tools: Vec<String> =
        CAPABILITIES.iter().map(|capability| format!("  {},", quote(&capability.qualified()))).collect();
    format!(
        "/**\n * 수집이 부를 수 있는 MCP 읽기 호출. `kairos install --apply`가 이 컴퓨터의 바인딩(connections.toml)으로 다시 쓴다.\n * 저장소의 이 파일은 바인딩이 없을 때의 값이다. 손으로 고치지 않는다. 설치본의 이 파일은 세션이 고치지 못하는 설치 폴더에 있다.\n */\n\
export const CALENDAR = {{ server: {}, list: {} }} as const\n\
export const GMAIL = {{ server: {}, search: {}, thread: {} }} as const\n\
export const SLACK = {{ server: {}, search: {} }} as const\n\
/** tool.check가 보는 이름. 후속 읽기(thread)도 든다. */\n\
export const TOOLS: readonly string[] = [\n{}\n]\n",
        quote(CALENDAR_EVENTS.server()),
        quote(CALENDAR_EVENTS.tool()),
        quote(MAIL_SEARCH.server()),
        quote(MAIL_SEARCH.tool()),
        quote(MAIL_THREAD.tool()),
        quote(SLACK_SEARCH.server()),
        quote(SLACK_SEARCH.tool()),
        tools.join("\n"),
    )
}

/// 설치본의 허용 목록을 바인딩으로 다시 쓴다. 임시 파일을 rename한다.
pub(super) fn write_allowlist(plugin_dir: &Path) -> KairosResult<()> {
    let path = plugin_dir.join(ALLOWED_CALLS_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    kairos_core::home::write_file_atomic(&path, &collector_allowlist())?;
    set_mode(&path, 0o644)?;
    Ok(())
}
