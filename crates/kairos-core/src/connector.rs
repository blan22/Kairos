//! 커넥터가 부르는 MCP 읽기 동작의 목록. 커넥터는 동작 이름만 알고, 실제 서버와 도구 이름은 이 컴퓨터의 바인딩
//! (`connections.toml`의 `[mcp.<소스>]`)에서 온다. 바인딩이 없으면 claude.ai 커넥터의 이름을 쓴다.
//!
//! 이 목록이 수집이 부를 수 있는 동작의 전부다. 설치기는 바인딩을 풀어 수집 훅의 허용 목록을 만든다.
//! 바인딩은 이 목록에 있는 동작만 잇고, 쓰기 동사가 든 도구 이름은 받지 않는다(kairos-config).

use crate::profile;

/// 읽기 동작 하나.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Capability {
    /// 바인딩을 찾는 소스 이름. Notion은 Gmail 바인딩을 쓴다.
    pub binding: &'static str,
    pub name: &'static str,
    pub default_server: &'static str,
    pub default_tool: &'static str,
    /// 계획이 직접 부르지 않고 같은 틱의 검색 결과로만 부르는 후속 읽기.
    pub follow_up: bool,
}

pub const CALENDAR_EVENTS: Capability = Capability {
    binding: "calendar",
    name: "events.list",
    default_server: "claude.ai Google Calendar",
    default_tool: "list_events",
    follow_up: false,
};
pub const MAIL_SEARCH: Capability = Capability {
    binding: "gmail",
    name: "mail.search",
    default_server: "claude.ai Gmail",
    default_tool: "search_threads",
    follow_up: false,
};
pub const MAIL_THREAD: Capability = Capability {
    binding: "gmail",
    name: "mail.thread",
    default_server: "claude.ai Gmail",
    default_tool: "get_thread",
    follow_up: true,
};
pub const SLACK_SEARCH: Capability = Capability {
    binding: "slack",
    name: "messages.search",
    default_server: "plugin:slack:slack",
    default_tool: "slack_search_public_and_private",
    follow_up: false,
};

pub const CAPABILITIES: [Capability; 4] = [CALENDAR_EVENTS, MAIL_SEARCH, MAIL_THREAD, SLACK_SEARCH];

/// 바인딩을 받는 소스.
pub const BINDABLE_SOURCES: [&str; 3] = ["calendar", "gmail", "slack"];

/// 도구 이름에 들어 있으면 쓰기 도구로 보는 낱말. 겹일 뿐이고 경계는 동작 목록이다.
pub const WRITE_WORDS: [&str; 16] = [
    "send", "create", "update", "delete", "trash", "label", "reply", "forward", "post", "write", "draft", "remove",
    "add", "set", "move", "schedule",
];

impl Capability {
    /// 이 컴퓨터에서 부를 서버 이름.
    pub fn server(self) -> &'static str {
        profile::current().bindings.get(self.binding).map_or(self.default_server, |binding| binding.server.as_str())
    }

    /// 이 컴퓨터에서 부를 도구 이름.
    pub fn tool(self) -> &'static str {
        profile::current()
            .bindings
            .get(self.binding)
            .and_then(|binding| binding.tools.get(self.name))
            .map_or(self.default_tool, String::as_str)
    }

    /// claude가 권한 검사에 쓰는 도구 이름. `mcp__<서버>__<도구>`이고 서버 이름의 글자 밖 문자는 `_`가 된다.
    pub fn qualified(self) -> String {
        format!("mcp__{}__{}", qualified_part(self.server()), self.tool())
    }
}

fn qualified_part(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

/// 소스에 있는 동작인가.
pub fn find(binding: &str, name: &str) -> Option<Capability> {
    CAPABILITIES.into_iter().find(|capability| capability.binding == binding && capability.name == name)
}

/// 쓰기 동사가 든 도구 이름인가. 단어 경계(`_`, `-`, 대소문자 전환)로 자른 조각을 본다.
pub fn looks_like_write(tool: &str) -> bool {
    let mut words = Vec::new();
    let mut current = String::new();
    for c in tool.chars() {
        if c == '_' || c == '-' || c == '.' || (c.is_ascii_uppercase() && !current.is_empty()) {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            if c == '_' || c == '-' || c == '.' {
                continue;
            }
        }
        current.push(c.to_ascii_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words.iter().any(|word| WRITE_WORDS.contains(&word.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 기본_이름은_지금_커넥터와_같다() {
        assert_eq!(MAIL_THREAD.qualified(), "mcp__claude_ai_Gmail__get_thread");
        assert_eq!(SLACK_SEARCH.qualified(), "mcp__plugin_slack_slack__slack_search_public_and_private");
        assert_eq!(CALENDAR_EVENTS.qualified(), "mcp__claude_ai_Google_Calendar__list_events");
    }

    #[test]
    fn 쓰기_동사를_알아본다() {
        for tool in ["send_message", "createDraft", "trash_thread", "update-event", "label_message"] {
            assert!(looks_like_write(tool), "{tool}");
        }
        for tool in ["search_threads", "get_thread", "list_events", "slack_search_public_and_private"] {
            assert!(!looks_like_write(tool), "{tool}");
        }
    }
}
