//! 시각 경계의 TS 원본과의 비교. 원본 시험은 경계 시각을 정확히 찍지 않아 `<`와 `<=`를 뒤집어도 통과한다.
//! tests/fixtures/golden_bounds.json의 사례를 돌려, 지운 TS 원본이 남긴 출력(golden/bounds_expected.json)과 바이트로 맞춘다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::Path;

use common::*;
use kairos_agenda::{
    ActionContext, CardChooser, ControlFile, DisplayStart, FocusTracker, ItemRepo, KairosControl, NextOptions,
    P1Alerts, P1NotifyInput, Presenter, SourceHealthBook, earliest_t0,
};
use kairos_core::KairosHome;
use kairos_core::model::{Source, SourceName};
use kairos_store::Database;
use kairos_store::rusqlite::types::ValueRef;
use serde_json::{Map, Value, json};

fn rows(db: &Database, sql: &str) -> Value {
    let mut statement = db.connection().prepare(sql).unwrap();
    let columns: Vec<String> = statement.column_names().map(str::to_owned).collect();
    let mut cursor = statement.query([]).unwrap();
    let mut out = Vec::new();
    while let Some(row) = cursor.next().unwrap() {
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
        out.push(Value::Object(object));
    }
    Value::Array(out)
}

/// 사례의 제어 값을 파일로 왕복한다. Node도 같은 길로 읽는다.
fn control_of(home: &KairosHome, step: &Value) -> KairosControl {
    let text = step.get("control").map_or_else(
        || r#"{"pausedUntil":null,"offDates":[],"displayMode":"on"}"#.to_owned(),
        |value| serde_json::to_string(value).unwrap(),
    );
    home.ensure().unwrap();
    fs::write(home.control_path(), text).unwrap();
    ControlFile::new(home).read()
}

fn run_step(db: &Database, home: &KairosHome, step: &Value) -> Value {
    let control = control_of(home, step);
    let text = |key: &str| step[key].as_str();
    let now = text("now").unwrap_or(T0);
    let context = context(now, &control, Over::default());
    let (github, slack_source) = (github_source(), FakeSource { name: SourceName::Slack, ..github_source() });
    let none = none();
    match text("op").unwrap() {
        "next" => json!(Presenter::new(db).next_response(&context, NextOptions::default()).unwrap()),
        "list" => json!(Presenter::new(db).list_response(&context, false).unwrap()),
        "counts" => json!(ItemRepo::new(db.connection()).counts(at(now)).unwrap()),
        "focus" => json!(FocusTracker::new(db.connection()).view(&control, at(now)).unwrap()),
        "health" => {
            let sources: [&dyn Source; 1] = [&github];
            json!(
                SourceHealthBook::new(db.connection())
                    .source_health(&sources, at(now), &none, text("resumedAt"))
                    .unwrap()
            )
        }
        "suspects" => json!(SourceHealthBook::new(db.connection()).notion_format_suspects(at(now)).unwrap()),
        "start" => json!(DisplayStart::compute(text("t0"), at(now), &none).unwrap()),
        "t0" => {
            let sources: [&dyn Source; 2] = [&github, &slack_source];
            json!(earliest_t0(db.connection(), &sources).unwrap())
        }
        "paused" => json!(control.is_paused(at(now))),
        "keep" => {
            let ids: Vec<i64> = step["ids"].as_array().unwrap().iter().map(|id| id.as_i64().unwrap()).collect();
            json!(actions(db).keep(&ids, &ActionContext { now: at(now), off_dates: &none }, true).unwrap())
        }
        "rows" => rows(db, text("sql").unwrap()),
        "p1" => {
            let recorder = Recorder::new();
            let mode = CardChooser::new(db.connection()).p1_alert_mode(&context).unwrap();
            let input = P1NotifyInput { now: at(now), mode, resumed_at: text("resumedAt") };
            let report = P1Alerts::new(db, &recorder).notify(&input).unwrap();
            json!({ "mode": mode, "report": report, "messages": recorder.messages() })
        }
        other => panic!("{other}"),
    }
}

#[test]
fn 시각_경계의_판정이_원본과_같다() {
    let dir = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let expected: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(fixtures.join("golden/bounds_expected.json")).unwrap()).unwrap();
    let cases: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(fixtures.join("golden_bounds.json")).unwrap()).unwrap();
    let mut remaining = expected.iter();
    for (index, case) in cases.iter().enumerate() {
        let name = case["name"].as_str().unwrap();
        let home = KairosHome::at(dir.path().join(format!("rust-{index}")));
        let db = Database::open(&home).unwrap();
        for text in case["sql"].as_array().unwrap() {
            db.connection().execute_batch(text.as_str().unwrap()).unwrap();
        }
        for (number, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let want = remaining.next().unwrap();
            let label = format!("{name} #{number} {}", step["op"].as_str().unwrap());
            assert_eq!(want["name"].as_str().unwrap(), label);
            let actual = serde_json::to_string(&run_step(&db, &home, step)).unwrap();
            assert_eq!(actual, want["text"].as_str().unwrap(), "{label}");
        }
    }
    assert!(remaining.next().is_none(), "남은 단계가 있다");
}
