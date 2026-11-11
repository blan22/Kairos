//! 지운 TS 원본이 남긴 출력과의 바이트 비교. tests/fixtures/golden은 원본을 지우기 전에 마지막으로 돌려 얼린 값이다.
//! 다시 만들 수 없으니 이 시험이 깨지면 고정값을 고치기 전에 출력이 바뀌는 것이 의도인지부터 본다.
//! 패널이 읽는 JSON과 파일 형식의 키 순서, null과 키 생략의 구분을 이 시험이 지킨다.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use common::*;
use kairos_agenda::{
    ActionContext, CardChooser, ControlFile, DisplayMode, EngineStatus, FocusTracker, ItemEngine, KairosControl,
    NextContext, NextOptions, P0Alerts, P1Alerts, P1NotifyInput, Presenter, RefreshInput, SourceHealthBook,
    StateBuilder, TriageDecision,
};
use kairos_core::model::Source;
use kairos_core::{KairosHome, OffDates};
use kairos_store::Database;
use kairos_store::rusqlite::types::ValueRef;
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};

const FRESH_ID: &str = "[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}";

/// 단계별 기대 출력을 차례로 맞춘다.
struct Script {
    expected: Vec<(String, String)>,
    cursor: usize,
}

impl Script {
    fn load() -> Self {
        let text = include_str!("fixtures/golden/expected.json");
        let rows: Vec<Value> = serde_json::from_str(text).unwrap();
        let expected = rows
            .iter()
            .map(|row| (row["name"].as_str().unwrap().to_owned(), row["text"].as_str().unwrap().to_owned()))
            .collect();
        Self { expected, cursor: 0 }
    }

    fn check(&mut self, name: &str, actual: &str) {
        let (expected_name, expected) = &self.expected[self.cursor];
        self.cursor += 1;
        assert_eq!(expected_name, name, "단계 순서가 다르다");
        assert_eq!(actual, expected, "{name}");
    }

    fn put<T: Serialize>(&mut self, name: &str, value: &T) {
        self.check(name, &serde_json::to_string(value).unwrap());
    }

    fn finish(&self) {
        assert_eq!(self.cursor, self.expected.len(), "남은 단계가 있다");
    }
}

fn dump(script: &mut Script, db: &Database, name: &str) {
    let fresh = Regex::new(FRESH_ID).unwrap();
    for table in ["items", "suggestions", "runs", "events"] {
        let mut statement = db.connection().prepare(&format!("SELECT * FROM {table} ORDER BY rowid")).unwrap();
        let columns: Vec<String> = statement.column_names().map(str::to_owned).collect();
        let mut rows = statement.query([]).unwrap();
        let mut lines = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            let mut object = Map::new();
            for (index, column) in columns.iter().enumerate() {
                let value = match row.get_ref(index).unwrap() {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(number) => json!(number),
                    ValueRef::Text(text) => json!(String::from_utf8_lossy(text)),
                    other => panic!("{other:?}"),
                };
                object.insert(column.clone(), value);
            }
            let text = serde_json::to_string(&object).unwrap();
            let replaced = fresh.replace_all(&text, |found: &regex::Captures<'_>| {
                if found[0].starts_with("00000000") { found[0].to_owned() } else { "UUID".to_owned() }
            });
            lines.push(replaced.into_owned());
        }
        script.check(&format!("{name}:{table}"), &lines.join("\n"));
    }
}

fn on() -> KairosControl {
    control_on().with_sessions(Some(BEFORE))
}

fn ctx(now: &str, control: &KairosControl, off_dates: OffDates, health: Option<String>) -> NextContext {
    NextContext { health, ..context(now, control, Over::off_dates(off_dates)) }
}

fn plain(now: &str, control: &KairosControl) -> NextContext {
    ctx(now, control, none(), None)
}

const SEEN: NextOptions = NextOptions { seen: true, ack: false };
const ACK: NextOptions = NextOptions { seen: false, ack: true };

fn alert(
    script: &mut Script,
    db: &Database,
    name: &str,
    now: &str,
    control: &KairosControl,
    resumed: Option<&str>,
    recorder: Recorder,
) {
    let mode = CardChooser::new(db.connection()).p1_alert_mode(&plain(now, control)).unwrap();
    let report =
        P1Alerts::new(db, &recorder).notify(&P1NotifyInput { now: at(now), mode, resumed_at: resumed }).unwrap();
    script.put(name, &json!({ "mode": mode, "report": report, "messages": recorder.messages() }));
}

fn file_step(script: &mut Script, name: &str, path: &Path) {
    script.check(name, &fs::read_to_string(path).unwrap());
}

fn save(script: &mut Script, file: &ControlFile<'_>, home: &KairosHome, name: &str, control: &KairosControl) {
    file.write(control).unwrap();
    file_step(script, name, &home.control_path());
}

fn control_steps(script: &mut Script, home: &KairosHome) {
    let file = ControlFile::new(home);
    save(script, &file, home, "control-default", &file.read());
    let off_dates = ["2026-03-05".to_owned(), "2026-03-06".to_owned()];
    save(script, &file, home, "control-off", &file.read().with_off_dates(&off_dates));
    save(script, &file, home, "control-mode", &file.read().with_display_mode(DisplayMode::On));
    save(script, &file, home, "control-opened", &file.read().with_display_opened_at(&kst(MON, "14:32")));
    save(script, &file, home, "control-sessions", &file.read().with_sessions(Some(&kst(MON, "14:33"))));
    save(script, &file, home, "control-notify", &file.read().with_p1_notify(false));
    save(script, &file, home, "control-focus", &FocusTracker::start(file.read(), 90, at(&kst(MON, "14:34"))));
    save(script, &file, home, "control-focus-end", &FocusTracker::end(file.read(), at(&kst(MON, "14:35"))));
    save(script, &file, home, "control-pause", &file.read().with_paused_until(Some("indefinite")));
    save(script, &file, home, "control-sessions-again", &file.read().with_sessions(Some(&kst(MON, "14:36"))));
    save(script, &file, home, "control-notify-on", &file.read().with_p1_notify(true));
    save(script, &file, home, "control-read-back", &file.read());
}

fn state_steps(script: &mut Script, db: &Database, home: &KairosHome, sources: &[&dyn Source], notion: &dyn Source) {
    let builder = StateBuilder::new(db.connection());
    let now = at(&kst(MON, "14:30"));
    StateBuilder::write_state(&builder.build(sources, now, &on(), Some(&kst(MON, "14:10"))).unwrap(), home).unwrap();
    file_step(script, "state.json", &home.state_path());
    StateBuilder::write_state(&builder.build(&[notion], now, &KairosControl::default(), None).unwrap(), home).unwrap();
    file_step(script, "state-empty.json", &home.state_path());
    let failed = EngineStatus { at: kst(MON, "14:30"), ok: false, error_code: Some("event_skipped".into()) };
    StateBuilder::write_engine_status(&failed, home).unwrap();
    file_step(script, "engine.json", &home.engine_status_path());
    let ok = EngineStatus { at: kst(MON, "14:31"), ok: true, error_code: None };
    StateBuilder::write_engine_status(&ok, home).unwrap();
    file_step(script, "engine-ok.json", &home.engine_status_path());
}

fn replay(script: &mut Script, db: &Database, dir: &Path) {
    let presenter = Presenter::new(db);
    let auto = KairosControl::default();
    let none = none();
    let next = |context: &NextContext, options| presenter.next_response(context, options).unwrap();
    let list = |context: &NextContext, all| presenter.list_response(context, all).unwrap();

    script.put("refresh", &refresh(db, &kst(MON, "10:05")));
    dump(script, db, "after-refresh");
    db.connection().execute_batch(include_str!("fixtures/golden_runs.sql")).unwrap();

    script.put("peek", &next(&plain(&kst(MON, "10:10"), &on()), NextOptions::default()));
    script.put("list", &list(&plain(&kst(MON, "10:10"), &on()), false));
    script.put("list-closed", &list(&plain(&kst(MON, "10:10"), &auto), false));
    script.put("list-all", &list(&plain(&kst(MON, "10:10"), &auto), true));
    script.put("next-closed", &next(&plain(&kst(MON, "10:10"), &auto), NextOptions::default()));
    script.put("see-p0", &next(&plain(&kst(MON, "10:11"), &on()), SEEN));
    script.put("peek-pending", &next(&plain(&kst(MON, "10:12"), &on()), NextOptions::default()));
    let gate = plain(&kst(MON, "10:12"), &on()).gate;
    for (name, recorder) in [("p0-fail", Recorder::failing()), ("p0", Recorder::new())] {
        let sent = P0Alerts::new(db, &recorder).notify(at(&kst(MON, "10:12")), &gate).unwrap();
        script.put(name, &json!({ "report": sent, "messages": recorder.messages() }));
    }

    let acts = actions(db);
    let when = at(&kst(MON, "10:13"));
    let batch = acts.triage_batch(when).unwrap();
    let views = presenter.triage_views(&batch.rows, when).unwrap();
    script.put("triage", &json!({ "items": views, "remaining": batch.remaining }));
    script.put("triage-keep", &acts.triage_decide(&[batch.rows[0].id], TriageDecision::Keep, when).unwrap());
    script.put("triage-drop", &acts.triage_decide(&[batch.rows[1].id, 999], TriageDecision::Drop, when).unwrap());

    let title = "합성 할 일 ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    script.put("add", &acts.add(title, at(&kst(MON, "10:14"))).unwrap());
    script.put("done-p0", &acts.done(&[1, 999], at(&kst(MON, "10:14")), false).unwrap());
    script.put("done-newest", &acts.done(&[21, 22], at(&kst(MON, "10:14")), false).unwrap());
    script.put("see-next", &next(&plain(&kst(MON, "10:15"), &on()), SEEN));
    let acting = ActionContext { now: at(&kst(MON, "10:16")), off_dates: &none };
    script.put("later", &acts.later(&[2, 3, 5], &acting).unwrap());
    script.put("keep", &acts.keep(&[4, 5], &acting, false).unwrap());
    script.put("keep-deploy", &acts.keep(&[7, 9], &acting, true).unwrap());
    script.put("drop", &acts.done(&[11], acting.now, true).unwrap());
    script.put("peek-after", &next(&plain(&kst(MON, "10:20"), &on()), NextOptions::default()));

    alert(script, db, "p1-fail", &kst(MON, "10:21"), &on(), None, Recorder::failing());
    alert(script, db, "p1-send", &kst(MON, "10:24"), &on(), Some(&kst(MON, "10:10")), Recorder::new());
    alert(script, db, "p1-lunch", &kst(MON, "12:30"), &on(), None, Recorder::new());

    script.put("peek-lunch", &next(&plain(&kst(MON, "12:30"), &on()), NextOptions::default()));
    script.put("see-bundle", &next(&plain(&kst(MON, "13:31"), &on()), SEEN));
    script.put("ack", &next(&plain(&kst(MON, "13:40"), &on()), ACK));

    let focused = FocusTracker::start(on(), 50, at(&kst(MON, "14:00")));
    script.put("focus-control", &focused);
    script.put("peek-focus", &next(&plain(&kst(MON, "14:05"), &focused), NextOptions::default()));
    script.put("list-focus", &list(&plain(&kst(MON, "14:05"), &focused), false));
    let ended = FocusTracker::end(focused, at(&kst(MON, "14:10")));
    script.put("focus-ended", &ended);
    script.put("focus-view", &FocusTracker::new(db.connection()).view(&ended, at(&kst(MON, "14:15"))).unwrap());

    let (github, slack_source, notion) = (
        github_source(),
        FakeSource { name: kairos_core::model::SourceName::Slack, ..github_source() },
        notion_source(),
    );
    let sources: [&dyn Source; 3] = [&github, &slack_source, &notion];
    let book = SourceHealthBook::new(db.connection());
    let resumed = kst(MON, "14:10");
    let health = book.source_health(&sources, at(&kst(MON, "14:20")), &none, Some(&resumed)).unwrap();
    let line = SourceHealthBook::health_line(&health);
    script.put("health", &health);
    script.put("health-line", &line);
    script.put("notices", &book.notice_lines(at(&kst(MON, "14:20"))).unwrap());
    let gaps = book.coverage_gaps(at(&kst(MON, "14:20"))).unwrap();
    script.put("gaps", &gaps.iter().map(SourceHealthBook::gap_line).collect::<Vec<_>>());
    script.put("peek-health", &next(&ctx(&kst(MON, "14:21"), &on(), none.clone(), line), NextOptions::default()));
    script.put("peek-off", &next(&ctx(&kst(MON, "14:22"), &on(), off(&[MON]), None), NextOptions::default()));
    let paused = on().with_paused_until(Some(&kst(MON, "15:00")));
    script.put("peek-paused", &next(&plain(&kst(MON, "14:23"), &paused), NextOptions::default()));

    let files = KairosHome::at(dir.join("rust-files"));
    files.ensure().unwrap();
    state_steps(script, db, &files, &sources, &notion);
    control_steps(script, &files);

    refresh_with(db, &kst(TUE, "10:00"), &none, &["github", "slack"]);
    script.put("carry", &next(&plain(&kst(TUE, "10:05"), &on()), SEEN));
    script.put("carry-ack", &next(&plain(&kst(TUE, "10:06"), &on()), ACK));
    let held =
        RefreshInput { now: at(&kst(TUE, "19:40")), off_dates: &none, ok_sources: &BTreeSet::new(), cards_held: true };
    script.put("held-refresh", &ItemEngine::new(db).refresh(&held).unwrap());
    dump(script, db, "final");
}

#[test]
fn 원본과_같은_시나리오의_출력과_표가_바이트까지_같다() {
    let dir = tempfile::tempdir().unwrap();
    let mut script = Script::load();
    let db = Database::open(&KairosHome::at(dir.path().join("rust"))).unwrap();
    db.connection().execute_batch(include_str!("fixtures/golden/seed.sql")).unwrap();
    replay(&mut script, &db, dir.path());
    script.finish();
}
