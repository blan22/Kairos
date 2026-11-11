//! 소스 켜고 끄기. 끈 소스는 `connections.toml`의 `[sources]`에 남는다.
//! `config.toml`에서 손으로 끈 소스는 여기서 되살리지 못한다. 세션이나 Orca 터미널 안에서는 바꾸지 않는다.

use std::path::Path;

use kairos_config::Connections;
use kairos_core::KairosResult;
use kairos_core::catalog::{self, CATALOG, Need, Readiness, SourceSpec};
use kairos_core::profile::Profile;
use serde_json::{Value, json};

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag};
use crate::command::Command;
use crate::reply::Reply;

const USAGE: &str = "사용법: kairos sources [on|off <소스>...] [--json]";
const OPTIONS: [OptionSpec; 1] = [flag("json")];

pub struct Sources;

impl Command for Sources {
    fn name(&self) -> &'static str {
        "sources"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(USAGE)
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        match parsed.positionals() {
            [] => status(context, &reply),
            [action, names @ ..] if !names.is_empty() && (action == "on" || action == "off") => {
                toggle(context, &reply, action == "on", names)
            }
            _ => Ok(reply.usage(USAGE)),
        }
    }
}

/// 세션이나 Orca 터미널 안에서 부른 것인가. 거기서는 설정을 바꾸지 않는다(신뢰 범위가 바뀐다).
/// kairos 세션의 표지(`KAIROS_SESSION`)도 본다. setup이 Orca 패널 승인까지 하므로 막는 겹을 하나 더 둔다.
pub(crate) fn session_caller(context: &AppContext) -> bool {
    context.env_set(kairos_sessions::SESSION_MARKER_ENV.0)
        || context.env_set("CLAUDECODE")
        || context.env_set("ORCA_TERMINAL_HANDLE")
}

fn toggle(context: &AppContext, reply: &Reply<'_>, enabled: bool, names: &[String]) -> KairosResult<i32> {
    if session_caller(context) {
        return Ok(reply.fail("session_caller", "세션 안에서는 소스를 바꿀 수 없습니다", 1));
    }
    if let Some(unknown) = names.iter().find(|name| catalog::find(name).is_none()) {
        let known: Vec<&str> = CATALOG.iter().map(|spec| spec.name.as_str()).collect();
        return Ok(reply.fail(
            "source_unknown",
            &format!("모르는 소스입니다: {unknown}. 고를 수 있는 소스: {}", known.join(", ")),
            2,
        ));
    }
    context.home().ensure()?;
    let root = context.home().root();
    if enabled {
        let pinned = kairos_config::user_disabled_sources(root).unwrap_or_default();
        if let Some(name) = names.iter().find(|name| pinned.contains(name)) {
            return Ok(reply.fail(
                "source_pinned_off",
                &format!("{name}은 config.toml의 [sources] disabled에서 껐습니다. 그 줄을 지운 뒤 다시 켜세요"),
                1,
            ));
        }
    }
    let mut connections = match Connections::read(root) {
        Ok(connections) => connections,
        Err(error) => return Ok(reply.fail(error.code, &format!("설정 오류: {error}"), 1)),
    };
    for name in names {
        connections.set_source_enabled(name, enabled);
    }
    if let Err(error) = connections.save(root) {
        return Ok(reply.fail(error.code, &format!("저장하지 않음: {error}"), 1));
    }
    status(context, reply)
}

fn status(context: &AppContext, reply: &Reply<'_>) -> KairosResult<i32> {
    let root = context.home().root();
    let loaded = match kairos_config::load(root) {
        Ok(loaded) => loaded,
        Err(error) => return Ok(reply.fail(error.code, &format!("설정 오류: {error}"), 1)),
    };
    let pinned = kairos_config::user_disabled_sources(root).unwrap_or_default();
    if reply.json() {
        return reply.success(json!({ "sources": view(&loaded.profile, &pinned) }));
    }
    for line in lines(&loaded.profile, &pinned) {
        context.console().print(&line);
    }
    context.console().print("켜고 끄기: kairos sources on|off <소스>...");
    Ok(0)
}

fn need_view(need: Need) -> Value {
    json!({ "key": need.key(), "label": need.label(), "hint": need.hint() })
}

fn view(profile: &Profile, pinned: &[String]) -> Vec<Value> {
    CATALOG
        .iter()
        .map(|spec| {
            let readiness = spec.readiness(profile);
            let missing = match &readiness {
                Readiness::Missing(needs) => needs.iter().copied().map(need_view).collect(),
                Readiness::Off | Readiness::Ready => Vec::new(),
            };
            json!({
                "name": spec.name.as_str(),
                "label": spec.name.label(),
                "summary": spec.summary,
                "state": readiness.key(),
                "enabled": readiness != Readiness::Off,
                "pinned": pinned.iter().any(|name| name == spec.name.as_str()),
                "missing": missing,
                "optional": spec.optional.iter().copied().filter(|need| !need.met(profile)).map(need_view).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// 소스마다 한 줄과 모자란 값의 줄. `setup`도 끝에 이 표를 보여 준다.
pub(crate) fn lines(profile: &Profile, pinned: &[String]) -> Vec<String> {
    let mut lines = Vec::new();
    for spec in &CATALOG {
        let readiness = spec.readiness(profile);
        let state = if readiness == Readiness::Off && pinned.iter().any(|name| name == spec.name.as_str()) {
            "끔(config.toml)"
        } else {
            readiness.label()
        };
        lines.push(format!("{} {}: {} ({})", mark(&readiness), spec.name.as_str(), state, spec.summary));
        if let Readiness::Missing(needs) = &readiness {
            for need in needs {
                lines.push(format!("    필요: {} → {}", need.label(), need.hint()));
            }
        }
        if readiness == Readiness::Ready {
            for need in optional_missing(spec, profile) {
                lines.push(format!("    선택: {} → {}", need.label(), need.hint()));
            }
        }
    }
    lines
}

fn optional_missing(spec: &SourceSpec, profile: &Profile) -> Vec<Need> {
    spec.optional.iter().copied().filter(|need| !need.met(profile)).collect()
}

const fn mark(readiness: &Readiness) -> &'static str {
    match readiness {
        Readiness::Ready => "●",
        Readiness::Missing(_) => "◐",
        Readiness::Off => "○",
    }
}

/// 설정을 다시 읽은 프로필. 바꾼 뒤의 상태를 보여 줄 때 쓴다. 이 프로세스의 `profile::current()`는 바뀌지 않는다.
pub(crate) fn reload(root: &Path) -> Result<Profile, kairos_config::ConfigError> {
    kairos_config::load(root).map(|loaded| loaded.profile)
}
