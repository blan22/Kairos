//! 계정 연동. 본인 식별자와 MCP 바인딩을 `connections.toml`에, 세션 토큰을 키체인에 쓴다.
//! 쓰기 전에 세 설정 파일을 합쳐 검증한다. 세션이나 Orca 터미널 안에서는 쓰지 않는다(신뢰 범위가 바뀐다).

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use kairos_config::{Connections, IdentityField};
use kairos_core::connector::{BINDABLE_SOURCES, CAPABILITIES};
use kairos_core::process::{ProcessEnd, ProcessRequest};
use kairos_core::profile::Profile;
use kairos_core::{KairosError, KairosResult};
use kairos_ops::find_binary;
use kairos_sessions::SessionToken;
use serde_json::json;

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::reply::Reply;

const USAGE: &str = "사용법: kairos connect [github | google <주소> | slack <사용자 ID> | name <이름> | mcp <calendar|gmail|slack> (<서버> [동작=도구]... | --reset) | session-token] [--json]";
const OPTIONS: [OptionSpec; 3] = [flag("json"), flag("reset"), text("gh")];
const GH_TIMEOUT: Duration = Duration::from_secs(15);
/// 세션 토큰의 상한. 파일 검사와 같다.
const TOKEN_MAX_BYTES: u64 = 512;

pub struct Connect;

impl Command for Connect {
    fn name(&self) -> &'static str {
        "connect"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(USAGE)
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let positionals = parsed.positionals();
        let Some(target) = positionals.first().map(String::as_str) else {
            return status(context, &reply);
        };
        if super::sources::session_caller(context) {
            return Ok(reply.fail("session_caller", "세션 안에서는 연동을 바꿀 수 없습니다", 1));
        }
        context.home().ensure()?;
        let rest = &positionals[1..];
        match (target, rest) {
            ("github", []) => github(context, &reply),
            ("google", [address]) => identity(context, &reply, IdentityField::Google, address),
            ("slack", [user]) => identity(context, &reply, IdentityField::Slack, user),
            ("name", [name]) => identity(context, &reply, IdentityField::Name, name),
            ("mcp", [source]) if parsed.flag("reset") => reset_binding(context, &reply, source),
            ("mcp", [source, server, tools @ ..]) => binding(context, &reply, source, server, tools),
            ("session-token", []) => session_token(context, &reply),
            _ => Ok(reply.usage(USAGE)),
        }
    }
}

fn save(context: &AppContext, reply: &Reply<'_>, connections: &Connections, done: &str) -> KairosResult<i32> {
    match connections.save(context.home().root()) {
        Ok(()) => {
            if reply.json() {
                return reply.success(json!({ "message": done }));
            }
            context.console().print(done);
            context.console().print("세션을 켜 두었다면 설정 지문이 바뀌어 잠깁니다. 차단 시험 뒤 kairos sessions on");
            Ok(0)
        }
        Err(error) => Ok(reply.fail(error.code, &format!("저장하지 않음: {error}"), 1)),
    }
}

fn read_connections(context: &AppContext) -> KairosResult<Connections> {
    Connections::read(context.home().root()).map_err(|error| KairosError::new(error.code))
}

fn identity(context: &AppContext, reply: &Reply<'_>, field: IdentityField, value: &str) -> KairosResult<i32> {
    let mut connections = read_connections(context)?;
    connections.set_identity(field, value);
    save(context, reply, &connections, "본인 식별자를 기록했습니다")
}

/// gh의 로그인 사용자를 본인으로 기록한다. 조직 소속이 활성인지도 본다(기록은 막지 않는다).
fn github(context: &AppContext, reply: &Reply<'_>) -> KairosResult<i32> {
    let login = match gh_login(context, &kairos_core::profile::current().org.github_orgs) {
        Ok(login) => login,
        Err((code, message)) => return Ok(reply.fail(code, message, 1)),
    };
    let mut connections = read_connections(context)?;
    connections.set_identity(IdentityField::Github, &login);
    save(context, reply, &connections, &format!("GitHub 로그인 {}을 기록했습니다", login.to_ascii_lowercase()))
}

/// gh의 로그인 사용자. `setup`도 쓴다. 조직 소속을 확인하지 못하면 표준 오류에 알리고 넘어간다.
pub(crate) fn gh_login(context: &AppContext, orgs: &[String]) -> Result<String, (&'static str, &'static str)> {
    let path = context.env("PATH").unwrap_or("/usr/bin:/bin");
    let Some(gh) = context.env("KAIROS_GH").map(str::to_owned).or_else(|| find_binary("gh", path)) else {
        return Err(("gh_missing", "gh를 찾지 못했습니다. gh auth login 뒤 다시 실행하세요"));
    };
    let env = BTreeMap::from([
        ("PATH".to_owned(), path.to_owned()),
        ("HOME".to_owned(), context.env("HOME").unwrap_or_default().to_owned()),
        ("GH_PROMPT_DISABLED".to_owned(), "1".to_owned()),
        ("GH_NO_UPDATE_NOTIFIER".to_owned(), "1".to_owned()),
        ("NO_COLOR".to_owned(), "1".to_owned()),
    ]);
    let run = |args: &[&str]| {
        let request =
            ProcessRequest::new(gh.clone(), args.iter().copied(), GH_TIMEOUT).env(env.clone()).max_output(4096);
        let output = context.runner().run(&request);
        matches!(output.end, ProcessEnd::Exited(0)).then(|| output.stdout.trim().to_owned())
    };
    let Some(login) = run(&["api", "user", "--jq", ".login"]).filter(|login| !login.is_empty()) else {
        return Err(("gh_not_logged_in", "gh 로그인 사용자를 읽지 못했습니다. gh auth login 뒤 다시 실행하세요"));
    };
    for org in orgs {
        let state = run(&["api", &format!("user/memberships/orgs/{org}"), "--jq", ".state"]);
        if state.as_deref() != Some("active") {
            context.console().error(&format!("주의: {org} 조직 소속을 확인하지 못했습니다(gh 권한 read:org 필요)"));
        }
    }
    Ok(login)
}

fn binding(context: &AppContext, reply: &Reply<'_>, source: &str, server: &str, pairs: &[String]) -> KairosResult<i32> {
    if !BINDABLE_SOURCES.contains(&source) {
        return Ok(reply.usage(USAGE));
    }
    let mut tools = BTreeMap::new();
    for pair in pairs {
        let Some((capability, tool)) = pair.split_once('=') else {
            return Ok(reply.usage(USAGE));
        };
        tools.insert(capability.to_owned(), tool.to_owned());
    }
    let mut connections = read_connections(context)?;
    connections.set_binding(source, server, tools, Some(context.now().to_iso()));
    let code = save(context, reply, &connections, &format!("{source} 바인딩을 기록했습니다"))?;
    if code == 0 && !reply.json() {
        context.console().print("수집 훅의 허용 목록에 반영하려면 kairos install --apply를 다시 실행하세요");
    }
    Ok(code)
}

fn reset_binding(context: &AppContext, reply: &Reply<'_>, source: &str) -> KairosResult<i32> {
    let mut connections = read_connections(context)?;
    if !connections.remove_binding(source) {
        return Ok(reply.fail("binding_missing", "그 소스의 바인딩이 없습니다", 1));
    }
    save(context, reply, &connections, &format!("{source} 바인딩을 지웠습니다. 기본 커넥터 이름을 씁니다"))
}

/// stdin의 토큰을 키체인에 넣는다. 인자로 받지 않는다(셸 기록과 ps에 남는다).
fn session_token(context: &AppContext, reply: &Reply<'_>) -> KairosResult<i32> {
    let mut text = String::new();
    std::io::stdin()
        .take(TOKEN_MAX_BYTES)
        .read_to_string(&mut text)
        .map_err(|_| KairosError::new("token_unreadable"))?;
    let token = match SessionToken::parse(&text) {
        Ok(token) => token,
        Err(_) => {
            return Ok(reply.fail(
                "session_token_invalid",
                "fine-grained 토큰(github_pat_)만 받습니다. classic과 gh 로그인 토큰은 쓰기 범위가 있어 받지 않습니다",
                1,
            ));
        }
    };
    context.keychain().write(&token)?;
    // 키체인 하나만 남긴다. 파일이 있으면 지운다.
    let file = context.home().file(kairos_sessions::SESSION_GH_TOKEN_FILE);
    let removed = std::fs::remove_file(&file).is_ok();
    let message = if removed {
        "세션 토큰을 키체인에 넣고 토큰 파일을 지웠습니다"
    } else {
        "세션 토큰을 키체인에 넣었습니다"
    };
    if reply.json() {
        return reply.success(json!({ "message": message }));
    }
    context.console().print(message);
    Ok(0)
}

fn status(context: &AppContext, reply: &Reply<'_>) -> KairosResult<i32> {
    let profile: &Profile = kairos_core::profile::current();
    let identity = &profile.identity;
    let token = match context.keychain().read() {
        Ok(kairos_sessions::KeychainRead::Found(_)) => "키체인",
        Ok(_) | Err(_) => match SessionToken::read(context.home()) {
            Ok(Some(_)) => "파일",
            Ok(None) => "없음",
            Err(_) => "파일(형식이나 권한 오류)",
        },
    };
    let bindings: BTreeMap<&str, (String, bool)> = BINDABLE_SOURCES
        .iter()
        .map(|source| {
            let capability = CAPABILITIES.iter().find(|capability| capability.binding == *source);
            let server = capability.map_or_else(String::new, |capability| capability.server().to_owned());
            (*source, (server, profile.bindings.contains_key(*source)))
        })
        .collect();
    if reply.json() {
        return reply.success(json!({
            "identity": {
                "github": identity.github_login,
                "google": identity.google_email,
                "slack": identity.slack_user,
                "name": identity.display_name,
            },
            "bindings": bindings.iter().map(|(source, (server, custom))| ((*source).to_owned(), json!({ "server": server, "custom": custom }))).collect::<serde_json::Map<_, _>>(),
            "sessionToken": token,
        }));
    }
    let or_none = |value: &Option<String>, hint: &str| value.clone().unwrap_or_else(|| format!("없음 ({hint})"));
    let console = context.console();
    console.print(&format!("GitHub: {}", or_none(&identity.github_login, "kairos connect github")));
    console.print(&format!("Google: {}", or_none(&identity.google_email, "kairos connect google <주소>")));
    console.print(&format!("Slack: {}", or_none(&identity.slack_user, "kairos connect slack <사용자 ID>")));
    console.print(&format!("이름: {}", or_none(&identity.display_name, "kairos connect name <이름>")));
    for (source, (server, custom)) in &bindings {
        console.print(&format!("MCP {source}: {server}{}", if *custom { "" } else { " (기본)" }));
    }
    console.print(&format!("세션 토큰: {token}"));
    Ok(0)
}
