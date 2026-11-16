//! 설정 확인과 처음 설정. 값은 데이터 폴더의 `org.toml`, `config.toml`, `connections.toml`에 있다.
//! 이 명령은 설정이 깨져도 돈다(`kairos_main`). 깨진 설정을 고치는 길이다.

use std::fs;
use std::io::Write;
use std::path::Path;

use kairos_config::{CONNECTIONS_FILE, Layer, Loaded, ORG_FILE, USER_FILE};
use kairos_core::KairosResult;
use kairos_core::platform::CreateMode;
use kairos_core::profile::Profile;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;
use crate::reply::Reply;

const USAGE: &str = "사용법: kairos config validate [--json] | show [--origin] [--json] | path | init [--org <파일> [--sha256 <값>]] [--force]";
const OPTIONS: [OptionSpec; 5] = [flag("json"), flag("origin"), flag("force"), text("org"), text("sha256")];
/// 조직 프로필 한 개의 상한. 설정 읽기의 상한과 같다.
const ORG_MAX_BYTES: u64 = 64 * 1024;

/// 처음 만드는 사용자 설정. 모두 주석이라 기본값이 그대로 돈다.
const USER_TEMPLATE: &str = r#"# kairos 사용자 설정. 값을 바꾼 뒤 `kairos config validate`로 확인한다.
schema = 1

# 본인 식별자는 `kairos connect`가 connections.toml에 쓴다. 여기 쓰면 그 값을 덮는다.
# [identity]
# name = "홍길동"

[schedule]
# 근무 요일과 근무 시간 밖은 방해 금지다. 카드와 Mac 알림을 내지 않는다.
# 주말에도 하루 종일 받으려면 days에 sat, sun을 넣고 work와 collect를 ["00:00", "24:00"]으로, lunch를 []로 둔다.
# timezone = "Asia/Seoul"
# days = ["mon", "tue", "wed", "thu", "fri"]
# work = ["10:00", "19:30"]
# lunch = ["12:00", "13:30"]
# bundles = ["13:30", "18:00"]
# collect = ["09:00", "20:00"]

# 끈 소스는 `kairos sources off <소스>`가 connections.toml에 쓴다.
# 여기 적은 소스는 `kairos sources on`으로도 켜지지 않는다.
[sources]
# disabled = ["notion"]

[hosts]
# 켠 호스트. 순서가 알림을 눌렀을 때 패널을 여는 순서다.
# enabled = ["orca", "claude-code", "vscode"]

# Orca 저장소 ID는 컴퓨터마다 다르다. `orca repo list --json`의 id를 적는다.
# [hosts.orca.repos]
# "acme/web" = "00000000-0000-0000-0000-000000000000"

[mcp]
# `kairos mcp serve`가 에이전트에 여는 동작. 기본은 읽기만이다.
# allow_actions = ["later", "done", "keep", "add", "accept"]
"#;

pub struct Config;

impl Command for Config {
    fn name(&self) -> &'static str {
        "config"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(USAGE)
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::with_positionals(&OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let root = context.home().root();
        match parsed.first() {
            Some("validate") => Ok(validate(context, &reply, root)),
            Some("show") => show(context, &reply, root, parsed.flag("origin")),
            Some("path") => {
                context.console().print(&root.to_string_lossy());
                Ok(0)
            }
            Some("init") => {
                init(context, &reply, root, parsed.text("org"), parsed.text("sha256"), parsed.flag("force"))
            }
            _ => Ok(reply.usage(USAGE)),
        }
    }
}

fn validate(context: &AppContext, reply: &Reply<'_>, root: &Path) -> i32 {
    match kairos_config::load(root) {
        Ok(loaded) => {
            if reply.json() {
                let _ = reply.success(json!({ "files": loaded.files, "warnings": loaded.warnings }));
                return 0;
            }
            let files = if loaded.files.is_empty() { "없음(기본값)".to_owned() } else { loaded.files.join(", ") };
            context.console().print(&format!("설정 정상: {files}"));
            for warning in &loaded.warnings {
                context.console().print(&format!("주의: {warning}"));
            }
            0
        }
        Err(error) => reply.fail(error.code, &format!("설정 오류: {error}"), 1),
    }
}

fn show(context: &AppContext, reply: &Reply<'_>, root: &Path, origin: bool) -> KairosResult<i32> {
    let loaded = match kairos_config::load(root) {
        Ok(loaded) => loaded,
        Err(error) => return Ok(reply.fail(error.code, &format!("설정 오류: {error}"), 1)),
    };
    let view = profile_view(&loaded.profile);
    if reply.json() {
        let origins: serde_json::Map<String, Value> =
            loaded.origins.iter().map(|(key, layer)| ((*key).to_owned(), json!(layer.as_str()))).collect();
        return reply.success(json!({
            "profile": view,
            "origins": origins,
            "fingerprint": kairos_config::session_fingerprint(&loaded.profile),
        }));
    }
    for line in show_lines(&loaded, origin) {
        context.console().print(&line);
    }
    Ok(0)
}

fn minutes(value: u32) -> String {
    format!("{:02}:{:02}", value / 60, value % 60)
}

/// 끈 점심은 빈 목록이다. 설정에 쓰는 모양과 같다.
fn lunch_view(schedule: &kairos_core::profile::Schedule) -> Vec<String> {
    if schedule.lunch.start >= schedule.lunch.end {
        return Vec::new();
    }
    vec![minutes(schedule.lunch.start), minutes(schedule.lunch.end)]
}

/// 설정에는 비밀이 없다(검증이 막는다). 그래도 바인딩의 도구 이름까지만 보인다.
fn profile_view(profile: &Profile) -> Value {
    let schedule = &profile.schedule;
    json!({
        "identity": {
            "github": profile.identity.github_login,
            "google": profile.identity.google_email,
            "slack": profile.identity.slack_user,
            "name": profile.identity.display_name,
        },
        "org": {
            "githubOrgs": profile.org.github_orgs,
            "trustedDomains": profile.org.trusted_domains,
            "deployCalendar": profile.org.deploy_calendar,
            "leaveCalendar": profile.org.leave_calendar,
            "holidayCalendar": profile.org.holiday_calendar,
            "deployTag": profile.org.deploy_tag,
            "notionWorkspace": profile.org.notion_workspace,
        },
        "schedule": {
            "timezone": schedule.zone.name(),
            "days": schedule.days.names(),
            "work": [minutes(schedule.work.start), minutes(schedule.work.end)],
            "lunch": lunch_view(schedule),
            "bundles": schedule.bundles.iter().map(|value| minutes(*value)).collect::<Vec<_>>(),
            "collect": [minutes(schedule.tick.start), minutes(schedule.tick.end)],
        },
        "playbooks": profile.playbooks.targets.iter().map(|target| json!({
            "owner": target.owner,
            "repos": target.repos,
            "workspaceRepo": target.workspace_repo,
            "orcaRepo": target.orca_repo,
            "baseBranch": target.base_branch,
        })).collect::<Vec<_>>(),
        "disabledSources": profile.disabled_sources,
        "hosts": profile.hosts.enabled,
        "bindings": profile.bindings.iter().map(|(source, binding)| (source.clone(), json!({
            "server": binding.server,
            "tools": binding.tools,
        }))).collect::<serde_json::Map<_, _>>(),
        "mcpActions": profile.mcp_actions,
    })
}

fn show_lines(loaded: &Loaded, origin: bool) -> Vec<String> {
    let profile = &loaded.profile;
    let from = |key: &str| {
        if !origin {
            return String::new();
        }
        let layer = loaded.origins.get(key).copied().unwrap_or(Layer::Default);
        format!("  ({})", layer.as_str())
    };
    let or_none = |value: &Option<String>| value.clone().unwrap_or_else(|| "없음".to_owned());
    let list = |values: &[String]| if values.is_empty() { "없음".to_owned() } else { values.join(", ") };
    let schedule = &profile.schedule;
    let mut lines = vec![
        format!("GitHub 로그인: {}{}", or_none(&profile.identity.github_login), from("identity.github")),
        format!("Google 주소: {}{}", or_none(&profile.identity.google_email), from("identity.google")),
        format!("Slack 사용자: {}{}", or_none(&profile.identity.slack_user), from("identity.slack")),
        format!("이름: {}{}", or_none(&profile.identity.display_name), from("identity.name")),
        format!("GitHub 조직: {}{}", list(&profile.org.github_orgs), from("org.github_orgs")),
        format!("신뢰 도메인: {}{}", list(&profile.org.trusted_domains), from("org.trusted_domains")),
        format!("배포 캘린더: {}{}", or_none(&profile.org.deploy_calendar), from("org.deploy_calendar")),
        format!("연차 캘린더: {}{}", or_none(&profile.org.leave_calendar), from("org.leave_calendar")),
        format!("공휴일 캘린더: {}{}", or_none(&profile.org.holiday_calendar), from("org.holiday_calendar")),
        format!("시간대: {}{}", schedule.zone.name(), from("schedule.timezone")),
        format!("근무 요일: {}{}", schedule.days.names().join(", "), from("schedule.days")),
        format!("근무: {}~{}{}", minutes(schedule.work.start), minutes(schedule.work.end), from("schedule.work")),
        format!(
            "점심: {}{}",
            match lunch_view(schedule).as_slice() {
                [start, end] => format!("{start}~{end}"),
                _ => "없음".to_owned(),
            },
            from("schedule.lunch")
        ),
        format!(
            "묶음: {}{}",
            schedule.bundles.iter().map(|value| minutes(*value)).collect::<Vec<_>>().join(", "),
            from("schedule.bundles")
        ),
        format!("수집: {}~{}{}", minutes(schedule.tick.start), minutes(schedule.tick.end), from("schedule.collect")),
        format!("끈 소스: {}{}", list(&profile.disabled_sources), from("sources.disabled")),
        format!("호스트: {}{}", list(&profile.hosts.enabled), from("hosts.enabled")),
        format!("MCP로 연 동작: {}{}", list(&profile.mcp_actions), from("mcp.allow_actions")),
    ];
    for target in &profile.playbooks.targets {
        lines.push(format!(
            "플레이북: {}/{{{}}} → {} ({}, Orca {}){}",
            target.owner,
            target.repos.join(","),
            target.workspace_repo,
            target.base_branch,
            target.orca_repo.as_deref().unwrap_or("원격 주소로 찾음"),
            from("playbooks")
        ));
    }
    for (source, binding) in &profile.bindings {
        lines.push(format!("MCP 바인딩 {source}: {}{}", binding.server, from("bindings")));
    }
    lines.push(format!("세션 지문: {}", &kairos_config::session_fingerprint(profile)[..16]));
    lines
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 파일을 새로 만든다. 이미 있으면 `force`일 때만 바꾼다. 같은 폴더의 임시 파일을 rename한다.
fn write_config(root: &Path, name: &str, content: &str, force: bool) -> KairosResult<bool> {
    let path = root.join(name);
    if path.exists() && !force {
        return Ok(false);
    }
    let temporary = root.join(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).create_mode(0o644).open(&temporary)?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    fs::rename(&temporary, &path)?;
    Ok(true)
}

/// 조직 프로필을 검증해 `org.toml`로 들인다. 성공하면 SHA256이다. `setup`도 쓴다.
pub(crate) fn import_org(
    root: &Path,
    org: &str,
    sha256: Option<&str>,
    force: bool,
) -> KairosResult<Result<String, (&'static str, String)>> {
    let unreadable = || Err(("org_unreadable", "조직 프로필을 읽을 수 없습니다".to_owned()));
    let Ok(metadata) = fs::metadata(org) else {
        return Ok(unreadable());
    };
    if !metadata.is_file() || metadata.len() > ORG_MAX_BYTES {
        return Ok(unreadable());
    }
    let Ok(content) = fs::read_to_string(org) else {
        return Ok(unreadable());
    };
    let digest = hex(&Sha256::digest(content.as_bytes()));
    if let Some(expected) = sha256
        && !expected.eq_ignore_ascii_case(&digest)
    {
        return Ok(Err(("org_sha256_mismatch", "조직 프로필의 SHA256이 다릅니다".to_owned())));
    }
    if let Err(error) = kairos_config::load_from_texts(Some(&content), None, None) {
        return Ok(Err((error.code, format!("조직 프로필 오류: {error}"))));
    }
    if !write_config(root, ORG_FILE, &content, force)? {
        return Ok(Err(("org_exists", "org.toml이 이미 있습니다. 바꾸려면 --force".to_owned())));
    }
    Ok(Ok(digest))
}

/// 사용자 설정 틀이 없으면 만든다. 만들었으면 true.
pub(crate) fn ensure_user_template(root: &Path) -> KairosResult<bool> {
    write_config(root, USER_FILE, USER_TEMPLATE, false)
}

/// 조직 프로필을 들이고 사용자 설정 틀을 만든다. 조직 프로필은 들이기 전에 검증하고, `--sha256`을 주면 맞아야 한다.
fn init(
    context: &AppContext,
    reply: &Reply<'_>,
    root: &Path,
    org: Option<&str>,
    sha256: Option<&str>,
    force: bool,
) -> KairosResult<i32> {
    context.home().ensure()?;
    let mut written = Vec::new();
    if let Some(org) = org {
        match import_org(root, org, sha256, force)? {
            Ok(digest) => {
                written.push(ORG_FILE);
                context.console().print(&format!("조직 프로필 SHA256: {digest}"));
            }
            Err((code, message)) => return Ok(reply.fail(code, &message, 1)),
        }
    }
    if ensure_user_template(root)? {
        written.push(USER_FILE);
    }
    let status = kairos_config::load(root);
    if reply.json() {
        return reply.success(json!({ "written": written, "valid": status.is_ok() }));
    }
    for name in &written {
        context.console().print(&format!("만듦: {}", root.join(name).to_string_lossy()));
    }
    match status {
        Ok(_) => context
            .console()
            .print(&format!("다음: kairos connect로 계정을 잇습니다. {CONNECTIONS_FILE}에 본인 식별자가 기록됩니다.")),
        Err(error) => context.console().print(&format!("설정 오류: {error}")),
    }
    Ok(0)
}
