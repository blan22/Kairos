use super::*;

const ORG: &str = r#"
schema = 1
name = "acme"

[github]
orgs = ["Acme"]

[trust]
email_domains = ["acme.example", "acme.io"]

[calendar]
deploy = "c_deploy@group.calendar.google.com"
leave = "acme.example_leave@group.calendar.google.com"
holidays = "ko.south_korea#holiday@group.v.calendar.google.com"
deploy_tag = "[deploy]"

[notion]
workspace = "Acme"

[[playbooks]]
owner = "acme"
repos = ["web", "web-specs"]
workspace_repo = "acme/web"
base_branch = "origin/stage"
"#;

const USER: &str = r#"
schema = 1

[identity]
name = "Jane Doe"

[schedule]
timezone = "America/New_York"
work = ["09:00", "18:00"]
lunch = ["12:00", "13:00"]
bundles = ["13:00", "17:00"]
collect = ["08:00", "19:00"]

[sources]
disabled = ["notion"]

[hosts]
enabled = ["claude-code", "orca"]

[hosts.orca.repos]
"acme/web" = "0000C0DE-0000-4000-8000-000000000001"
"#;

const CONNECTIONS: &str = r#"
schema = 1

[identity]
github = "jdoe"
google = "jane@acme.example"
slack = "U0123ABC"
name = "Connected Name"

[mcp.gmail]
server = "claude.ai Gmail"
tools = { "mail.search" = "search_threads", "mail.thread" = "get_thread" }
"#;

#[test]
fn 세_층을_합친다() {
    let loaded = load_from_texts(Some(ORG), Some(USER), Some(CONNECTIONS)).unwrap();
    let profile = &loaded.profile;
    assert_eq!(profile.org.github_orgs, ["acme"]);
    assert_eq!(profile.identity.github_login.as_deref(), Some("jdoe"));
    // 사용자 설정의 identity가 연동 결과를 덮는다.
    assert_eq!(profile.identity.display_name.as_deref(), Some("Jane Doe"));
    assert_eq!(loaded.origins["identity.name"], Layer::User);
    assert_eq!(loaded.origins["identity.github"], Layer::Connections);
    assert_eq!(profile.schedule.zone, chrono_tz::America::New_York);
    assert_eq!(profile.schedule.bundles, [780, 1020]);
    assert_eq!(profile.disabled_sources, ["notion"]);
    assert_eq!(profile.hosts.enabled, ["claude-code", "orca"]);
    let target = &profile.playbooks.targets[0];
    assert_eq!(target.orca_repo.as_deref(), Some("0000c0de-0000-4000-8000-000000000001"));
    assert_eq!(profile.bindings["gmail"].tools["mail.search"], "search_threads");
}

#[test]
fn 파일이_없으면_빈_프로필이다() {
    let loaded = load_from_texts(None, None, None).unwrap();
    assert_eq!(loaded.profile, Profile::empty());
}

#[test]
fn 모르는_키는_거부한다() {
    let error = load_from_texts(Some("schema = 1\n[trust]\nemail_domain = [\"x.com\"]\n"), None, None).unwrap_err();
    assert_eq!(error.code, "config_parse_error");
}

#[test]
fn schema가_없거나_다르면_거부한다() {
    assert_eq!(load_from_texts(Some("name = \"x\"\n"), None, None).unwrap_err().code, "config_schema_missing");
    assert_eq!(load_from_texts(Some("schema = 2\n"), None, None).unwrap_err().code, "config_schema_unsupported");
}

#[test]
fn 한_저장소는_대상_하나에만_든다() {
    let user = "schema = 1\n[[playbooks]]\nowner = \"acme\"\nrepos = [\"web\"]\nworkspace_repo = \"acme/web\"\nbase_branch = \"origin/main\"\n";
    let error = load_from_texts(Some(ORG), Some(user), None).unwrap_err();
    assert_eq!(error.code, "config_duplicate_value");
}

#[test]
fn 위험한_값은_거부한다() {
    let cases = [
        (
            "schema = 1\n[[playbooks]]\nowner = \"acme\"\nrepos = [\"web\"]\nworkspace_repo = \"acme/web\"\nbase_branch = \"origin/$(id)\"\n",
            "config_invalid_value",
        ),
        ("schema = 1\n[github]\norgs = [\"acme org\"]\n", "config_invalid_value"),
        ("schema = 1\n[notion]\nworkspace = \"ghp_0123456789\"\n", "config_secret_value"),
    ];
    for (org, code) in cases {
        assert_eq!(load_from_texts(Some(org), None, None).unwrap_err().code, code, "{org}");
    }
    let user = "schema = 1\n[mcp]\nallow_actions = [\"install\"]\n";
    assert_eq!(load_from_texts(None, Some(user), None).unwrap_err().code, "config_unknown_action");
}

#[test]
fn 바인딩은_읽기_동작만_잇는다() {
    let bind = |tools: &str| format!("schema = 1\n[mcp.gmail]\nserver = \"my gmail\"\ntools = {{ {tools} }}\n");
    let ok = load_from_texts(None, None, Some(&bind("\"mail.search\" = \"find_mail\""))).unwrap();
    assert_eq!(ok.profile.bindings["gmail"].server, "my gmail");
    let cases = [
        ("\"mail.search\" = \"send_message\"", "config_write_tool"),
        ("\"mail.delete\" = \"find_mail\"", "config_unknown_capability"),
        ("\"mail.search\" = \"find mail\"", "config_invalid_value"),
    ];
    for (tools, code) in cases {
        assert_eq!(load_from_texts(None, None, Some(&bind(tools))).unwrap_err().code, code, "{tools}");
    }
    let notion = "schema = 1\n[mcp.notion]\nserver = \"x\"\n";
    assert_eq!(load_from_texts(None, None, Some(notion)).unwrap_err().code, "config_unknown_source");
}

#[test]
fn 지문은_세션_자격_값이_바뀔_때만_바뀐다() {
    let base = load_from_texts(Some(ORG), Some(USER), Some(CONNECTIONS)).unwrap().profile;
    let mut schedule_only = base.clone();
    schedule_only.schedule.bundles = vec![600];
    assert_eq!(session_fingerprint(&base), session_fingerprint(&schedule_only));
    let mut wider = base.clone();
    wider.org.trusted_domains.push("evil.com".to_owned());
    assert_ne!(session_fingerprint(&base), session_fingerprint(&wider));
}

#[test]
fn 남이_쓸_수_있는_파일은_읽지_않는다() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(ORG_FILE);
    std::fs::write(&path, ORG).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(load(dir.path()).unwrap_err().code, "config_unsafe");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(load(dir.path()).unwrap().files, [ORG_FILE]);
    std::os::unix::fs::symlink(&path, dir.path().join(USER_FILE)).unwrap();
    assert_eq!(load(dir.path()).unwrap_err().code, "config_unsafe");
}

#[test]
fn 연동_결과는_검증을_통과해야_쓴다() {
    let dir = tempfile::tempdir().unwrap();
    let mut connections = Connections::read(dir.path()).unwrap();
    connections.set_identity(IdentityField::Github, "JDoe");
    connections.set_binding("gmail", "my gmail", BTreeMap::new(), None);
    connections.save(dir.path()).unwrap();
    let loaded = load(dir.path()).unwrap();
    assert_eq!(loaded.profile.identity.github_login.as_deref(), Some("jdoe"));
    assert_eq!(loaded.profile.bindings["gmail"].server, "my gmail");
    let mut bad = Connections::read(dir.path()).unwrap();
    bad.set_identity(IdentityField::Slack, "not-an-id");
    assert_eq!(bad.save(dir.path()).unwrap_err().code, "config_invalid_value");
    assert!(load(dir.path()).unwrap().profile.identity.slack_user.is_none());
}

#[test]
fn 끈_소스는_두_파일을_합친다() {
    let user = "schema = 1\n[sources]\ndisabled = [\"notion\"]\n";
    let connections = "schema = 1\n[sources]\ndisabled = [\"slack\", \"notion\"]\n";
    let loaded = load_from_texts(None, Some(user), Some(connections)).unwrap();
    assert_eq!(loaded.profile.disabled_sources, ["slack", "notion"]);
    assert_eq!(loaded.origins["sources.disabled"], Layer::User);
    let unknown = "schema = 1\n[sources]\ndisabled = [\"jira\"]\n";
    assert_eq!(load_from_texts(None, None, Some(unknown)).unwrap_err().code, "config_unknown_source");
}

#[test]
fn 소스를_끄고_켠_결과를_쓴다() {
    let dir = tempfile::tempdir().unwrap();
    let mut connections = Connections::read(dir.path()).unwrap();
    assert!(connections.set_source_enabled("slack", false));
    assert!(!connections.set_source_enabled("slack", false));
    connections.save(dir.path()).unwrap();
    assert_eq!(load(dir.path()).unwrap().profile.disabled_sources, ["slack"]);
    let mut connections = Connections::read(dir.path()).unwrap();
    assert!(connections.set_source_enabled("slack", true));
    connections.save(dir.path()).unwrap();
    assert!(load(dir.path()).unwrap().profile.disabled_sources.is_empty());
    // 빈 목록은 키째 빠진다.
    assert!(!std::fs::read_to_string(dir.path().join(CONNECTIONS_FILE)).unwrap().contains("[sources]"));
    let mut bad = Connections::read(dir.path()).unwrap();
    bad.set_source_enabled("jira", false);
    assert_eq!(bad.save(dir.path()).unwrap_err().code, "config_unknown_source");
}

#[test]
fn 근무_요일과_하루_종일_근무와_점심_끄기를_받는다() {
    use kairos_core::profile::{MinuteRange, WorkDays};
    let user = "schema = 1\n[schedule]\ndays = [\"mon\", \"tue\", \"wed\", \"thu\", \"fri\", \"sat\", \"sun\"]\nwork = [\"00:00\", \"24:00\"]\nlunch = []\ncollect = [\"00:00\", \"24:00\"]\n";
    let loaded = load_from_texts(None, Some(user), None).unwrap();
    let schedule = &loaded.profile.schedule;
    assert_eq!(schedule.days, WorkDays::ALL);
    assert_eq!(schedule.work, MinuteRange::new(0, 24 * 60));
    assert!(!schedule.lunch.contains(0) && !schedule.lunch.contains(12 * 60));
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
}

#[test]
fn 근무_요일은_이름만_받고_비거나_겹치면_거부한다() {
    for (days, code) in [
        ("[]", "config_no_days"),
        ("[\"Mon\"]", "config_invalid_day"),
        ("[\"mon\", \"mon\"]", "config_duplicate_value"),
    ] {
        let user = format!("schema = 1\n[schedule]\ndays = {days}\n");
        assert_eq!(load_from_texts(None, Some(&user), None).unwrap_err().code, code, "{days}");
    }
    let user = "schema = 1\n[schedule]\nlunch = [\"12:00\"]\n";
    assert_eq!(load_from_texts(None, Some(user), None).unwrap_err().code, "config_invalid_range");
}
