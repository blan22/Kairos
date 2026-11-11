//! 지운 TS 원본이 남긴 출력과의 바이트 비교. tests/fixtures/golden은 원본을 지우기 전에 마지막으로 돌려 얼린 값이다.
//! 다시 만들 수 없으니 이 시험이 깨지면 고정값을 고치기 전에 출력이 바뀌는 것이 의도인지부터 본다.
//! plan-tick JSON, doctor 줄, watch.tsv와 틱이 쓰는 파일, 틱 보고, source_run을 이 시험이 지킨다.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_core::model::{QuerySpec, Source, SourceName};
use kairos_core::ports::{Clock, FixedClock};
use kairos_core::process::SystemProcessRunner;
use kairos_core::{KairosHome, OffDates};
use kairos_source_mcp::{CalendarSource, GmailSource, NotionSource, SlackSource};
use kairos_store::Database;
use kairos_tick::{
    ClaudeProbe, CollectReport, Doctor, DoctorConfig, HaltFlag, IngestPayload, ResponseIngestor, SystemClaudeProbe,
    TickPlanner,
};
use serde_json::{Value, json};

const NOON: &str = "2026-03-04T03:00:00.000Z";
const NIGHT: &str = "2026-03-04T12:00:00.000Z";
const LATER: &str = "2026-03-04T03:30:00.000Z";
const MODE: &str = "2026-03-04T04:00:00.000Z";
const SPEND: &str = "2026-03-04T05:00:00.000Z";

struct Script {
    expected: Vec<(String, String)>,
    cursor: usize,
    home: KairosHome,
}

impl Script {
    fn check(&mut self, name: &str, actual: &str) {
        let (expected_name, expected) = &self.expected[self.cursor];
        self.cursor += 1;
        assert_eq!(expected_name, name, "단계 순서가 다르다");
        assert_eq!(actual, expected, "{name}");
    }

    fn file(&mut self, name: &str, path: &str) {
        let text = fs::read_to_string(self.home.file(path)).unwrap();
        self.check(name, &text);
    }
}

fn fixed(now: &str) -> Arc<dyn Clock> {
    Arc::new(FixedClock(at(now)))
}

fn sources(now: &str) -> Vec<Arc<dyn Source>> {
    vec![
        Arc::new(CalendarSource::new(fixed(now))),
        unconnected(SourceName::Github, vec![QuerySpec::new("default", 180, 0)]),
        Arc::new(SlackSource::new(fixed(now))),
        Arc::new(GmailSource::new(fixed(now))),
        Arc::new(NotionSource::new(fixed(now))),
    ]
}

fn plan(script: &mut Script, name: &str, now: &str, off_dates: &OffDates, force: bool) {
    let db = Database::open(&script.home).unwrap();
    let list = sources(now);
    let refs: Vec<&dyn Source> = list.iter().map(AsRef::as_ref).collect();
    let plan = TickPlanner::new(db.connection()).plan_tick(&refs, at(now), off_dates, force).unwrap();
    script.check(name, &serde_json::to_string(&plan).unwrap());
}

fn doctor(script: &mut Script, name: &str, now: &str, login: Option<Quiet>) {
    let config = DoctorConfig {
        home: script.home.clone(),
        user_home: script.home.root().with_file_name("rust-user"),
        env: BTreeMap::new(),
    };
    let mut doctor = Doctor::new(config, fixed(now), missing_runner(), sources(now));
    if let Some(login) = login {
        doctor = doctor.with_login_probe(Arc::new(login));
    }
    let report = doctor.diagnose().unwrap();
    let text = json!({ "lines": report.lines, "exitCode": report.exit_code, "problems": report.problems }).to_string();
    let root = script.home.root().to_string_lossy().into_owned();
    script.check(name, &text.replace(&root, "<HOME>"));
}

fn put_report(script: &mut Script, name: &str, report: &CollectReport) {
    let mut value = serde_json::to_value(report).unwrap();
    if value["mode"].is_object() {
        value["mode"]["elapsedMs"] = json!(0);
    }
    script.check(name, &value.to_string());
}

fn tick_at(script: &Script, now: &str, list: Vec<Arc<dyn Source>>) -> Tick {
    tick(&script.home, now, list).user_home(&script.home.root().with_file_name("rust-user")).sleep(frozen(0))
}

fn source_runs(home: &KairosHome) -> String {
    let db = Database::open(home).unwrap();
    let mut statement = db
        .connection()
        .prepare(
            "SELECT source, query, started_at, finished_at, outcome, identity, error_code FROM source_run ORDER BY id",
        )
        .unwrap();
    let rows: Vec<Value> = statement
        .query_map([], |row| {
            let text = |index: usize| row.get::<_, Option<String>>(index);
            Ok(json!({
                "source": text(0)?, "query": text(1)?, "started_at": text(2)?, "finished_at": text(3)?,
                "outcome": text(4)?, "identity": text(5)?, "error_code": text(6)?,
            }))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    serde_json::to_string(&rows).unwrap()
}

fn replay(script: &mut Script, dir: &Path) {
    let quiet_login = || Some(Quiet::default());
    plan(script, "plan-noon", NOON, &none(), false);
    plan(script, "plan-night", NIGHT, &none(), false);
    plan(script, "plan-off", NOON, &off(&["2026-03-04"]), false);
    plan(script, "plan-force", LATER, &none(), true);
    doctor(script, "doctor-seed", NOON, quiet_login());
    let soon = Quiet { login: Some("2026-03-08T00:00:00.000Z".into()), ..Quiet::default() };
    doctor(script, "doctor-login", NOON, Some(soon));
    let fake = dir.join("fake-claude").to_string_lossy().into_owned();
    let probe = SystemClaudeProbe::new(Arc::new(SystemProcessRunner), Some(fake), BTreeMap::new());
    let print = probe.fingerprint().unwrap().unwrap();
    let (path, rest) = print.split_once('|').unwrap();
    let (stamp, version) = rest.split_once('|').unwrap();
    assert_eq!(path, dir.join("fake-claude").to_string_lossy());
    assert!(stamp.parse::<f64>().is_ok(), "{stamp}");
    assert_eq!(version, "9.9.9 (합성)");
    script.check("login-expiry", &probe.login_expiry().unwrap().unwrap());

    let login = Quiet { login: Some("2026-04-30T00:00:00.000Z".into()), ..Quiet::default() };
    let report = tick_at(script, LATER, sources(LATER)).probe(login).run();
    put_report(script, "tick-missing", &report);
    script.file("watch.tsv", "watch.tsv");
    script.file("auth.json", "auth.json");
    script.file("state.json", "state.json");
    doctor(script, "doctor-tick", LATER, None);

    let plugin = script.home.file("plugin").to_string_lossy().into_owned();
    let inner = script.home.clone();
    let mode = runner(move |_| {
        let db = Database::open(&inner).unwrap();
        let responses = vec![
            serde_json::from_value(
                json!({ "id": "gmail:recent", "isError": false, "content": [{ "type": "text", "text": "{}" }] }),
            )
            .unwrap(),
        ];
        let payload = IngestPayload { now: at(MODE), source: SourceName::Gmail, responses };
        let gmail = GmailSource::new(fixed(MODE));
        ResponseIngestor::new(&db, fixed(MODE)).ingest_responses(&gmail, &payload, &off(&["2026-03-20"])).unwrap();
        dropped(json!({}))
    });
    let mail: Arc<dyn Source> = Arc::new(GmailSource::new(fixed(MODE)));
    let report = tick_at(script, MODE, vec![mail]).claude(&plugin, mode).fingerprint("v2").run();
    put_report(script, "tick-contract", &report);
    script.file("collector.json", "collector.json");
    script.file("watch-mail.tsv", "watch.tsv");

    let spend = drop_runner(json!({ "num_turns": 2, "total_cost_usd": 0.36 }));
    let report = tick_at(script, SPEND, sources(SPEND)).claude(&plugin, spend).run();
    put_report(script, "tick-spend", &report);
    script.file("halt.json", "halt.json");
    doctor(script, "doctor-halted", SPEND, quiet_login());
    let report = tick_at(script, SPEND, sources(SPEND)).claude(&plugin, missing_runner()).run();
    put_report(script, "tick-halted", &report);
    HaltFlag::new(&script.home, fixed(SPEND)).clear().unwrap();
    fs::set_permissions(script.home.state_path(), fs::Permissions::from_mode(0o644)).unwrap();
    doctor(script, "doctor-permissions", SPEND, quiet_login());
    let rows = source_runs(&script.home);
    script.check("source_run", &rows);
}

/// 원본 시험의 claude 대역.
const FAKE_CLAUDE: &str = "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo \" 9.9.9 (합성)\"; else echo '{\"loginExpiresAt\":\"2026-04-30T00:00:00.000Z\"}'; fi\n";

fn private(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// 시나리오 전의 홈을 고정값에서 만든다.
fn seed(dir: &Path) -> KairosHome {
    let home = KairosHome::at(dir.join("rust"));
    let db = Database::open(&home).unwrap();
    db.connection().execute_batch(&fixture("golden/seed.sql")).unwrap();
    drop(db);
    let control = home.file("control.json");
    fs::write(&control, fixture("golden/control.json")).unwrap();
    private(&control, 0o600);
    let plugin = dir.join("rust/plugin");
    fs::create_dir(&plugin).unwrap();
    private(&plugin, 0o700);
    let fake = dir.join("fake-claude");
    fs::write(&fake, FAKE_CLAUDE).unwrap();
    private(&fake, 0o700);
    home
}

#[test]
fn 원본과_같은_시나리오의_출력과_파일이_바이트까지_같다() {
    let dir = tempfile::tempdir().unwrap();
    let home = seed(dir.path());
    let rows: Vec<Value> = serde_json::from_str(&fixture("golden/expected.json")).unwrap();
    let expected = rows
        .iter()
        .map(|row| (row["name"].as_str().unwrap().to_owned(), row["text"].as_str().unwrap().to_owned()))
        .collect();
    let mut script = Script { expected, cursor: 0, home };
    replay(&mut script, dir.path());
    assert_eq!(script.cursor, script.expected.len(), "남은 단계가 있다");
}
