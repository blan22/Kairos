/**
 * 수집이 부를 수 있는 MCP 읽기 호출. `kairos install --apply`가 이 컴퓨터의 바인딩(connections.toml)으로 다시 쓴다.
 * 저장소의 이 파일은 바인딩이 없을 때의 값이다. 손으로 고치지 않는다. 설치본의 이 파일은 세션이 고치지 못하는 설치 폴더에 있다.
 */
export const CALENDAR = { server: "claude.ai Google Calendar", list: "list_events" } as const
export const GMAIL = { server: "claude.ai Gmail", search: "search_threads", thread: "get_thread" } as const
export const SLACK = { server: "plugin:slack:slack", search: "slack_search_public_and_private" } as const
/** tool.check가 보는 이름. 후속 읽기(thread)도 든다. */
export const TOOLS: readonly string[] = [
  "mcp__claude_ai_Google_Calendar__list_events",
  "mcp__claude_ai_Gmail__search_threads",
  "mcp__claude_ai_Gmail__get_thread",
  "mcp__plugin_slack_slack__slack_search_public_and_private",
]
