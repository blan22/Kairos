// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::*;
use kairos_agenda::{
    ControlFile, HealthStatus, INDEFINITE, KairosControl, SourceHealth, SourceHealthBook, StateBuilder,
};
use kairos_core::OffDates;
use kairos_core::home::write_file_atomic;
use kairos_core::model::{NormalizedEvent, NormalizedObject, Outcome, Source, SourceName, SourceResult};
use kairos_policy::Tier;
use kairos_store::Database;
use serde_json::json;

fn minute(minutes: i64) -> String {
    plus_minutes(T0, minutes)
}

fn health(db: &Database, sources: &[&dyn Source], now: &str, off_dates: &OffDates) -> Vec<SourceHealth> {
    SourceHealthBook::new(db.connection()).source_health(sources, at(now), off_dates, None).unwrap()
}

fn built(db: &Database, sources: &[&dyn Source], now: &str) -> kairos_agenda::KairosState {
    StateBuilder::new(db.connection()).build(sources, at(now), &KairosControl::default(), None).unwrap()
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).unwrap().map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn state_json은_원자적으로_교체되고_임시_파일을_남기지_않는다() {
    let (dir, home) = temp_home();
    let db = test_db();
    let source = github_source();
    let sources: [&dyn Source; 1] = [&source];
    let builder = StateBuilder::new(db.connection());
    let control = ControlFile::new(&home);
    StateBuilder::write_state(&builder.build(&sources, at(T0), &control.read(), None).unwrap(), &home).unwrap();
    StateBuilder::write_state(&builder.build(&sources, at(T1), &control.read(), None).unwrap(), &home).unwrap();
    assert_eq!(StateBuilder::read_state(&home).unwrap()["generatedAt"], T1);
    assert_eq!(names_in(dir.path()), ["state.json"]);
    assert_eq!(fs::metadata(home.state_path()).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn 쓰기가_실패하면_기존_파일이_그대로고_임시_파일이_지워진다() {
    let (dir, home) = temp_home();
    write_file_atomic(&home.state_path(), r#"{"kept":true}"#).unwrap();
    assert!(write_file_atomic(&dir.path().join("missing").join("state.json"), "{}").is_err());
    assert_eq!(names_in(dir.path()), ["state.json"]);
}

#[test]
fn 깨진_state_json은_null이다() {
    let (_dir, home) = temp_home();
    assert_eq!(StateBuilder::read_state(&home), None);
    fs::write(home.state_path(), "{").unwrap();
    assert_eq!(StateBuilder::read_state(&home), None);
}

#[test]
fn state에_건수_소스_건강_생성_시각이_들어간다() {
    let db = test_db();
    ingest(&db, SourceResult { objects: vec![issue(1)], ..result() }, T0);
    let run = SourceResult {
        started_at: T1.into(),
        finished_at: T1.into(),
        watermark: Some(T1.into()),
        events: vec![comment("1", T1)],
        ..result()
    };
    ingest(&db, run, T1);
    let (github, notion) = (github_source(), notion_source());
    let state = built(&db, &[&github, &notion], T1);
    assert_eq!(state.generated_at.to_iso(), T1);
    assert_eq!(state.counts.objects, 1);
    assert_eq!(state.counts.baseline_objects, 1);
    assert_eq!(state.counts.events, 1);
    let shown = state.counts.would_show;
    assert_eq!((shown.p0, shown.p1, shown.p2), (0, 0, 1));
    let rows: Vec<_> = state.sources.iter().map(|row| (row.source, row.status)).collect();
    assert_eq!(rows, [(SourceName::Github, HealthStatus::Ok), (SourceName::Notion, HealthStatus::NotConnected)]);
}

#[test]
fn state에는_본문이나_제목이_들어가지_않는다() {
    let db = test_db();
    let object = NormalizedObject { text_head: Some("합성 비밀 제목".into()), ..issue(1) };
    ingest(&db, SourceResult { objects: vec![object], ..result() }, T0);
    let github = github_source();
    let state = built(&db, &[&github], T0);
    assert!(!serde_json::to_string(&state).unwrap().contains("합성 비밀 제목"));
}

#[test]
fn 건강_3분_주기는_10분까지_정상이고_넘으면_지연이다() {
    let db = test_db();
    ingest(&db, result(), T0);
    let source = github_source();
    let row = |minutes| health(&db, &[&source], &minute(minutes), &none()).remove(0);
    assert_eq!(row(10).status, HealthStatus::Ok);
    assert_eq!(row(11).status, HealthStatus::Delayed);
    assert_eq!(row(12).delay_minutes, Some(12));
}

#[test]
fn 건강_60분_주기는_3시간_기준이다() {
    let db = test_db();
    let first = SourceResult { source: SourceName::Gmail, query: "recent".into(), ..result() };
    ingest(&db, first, T0);
    let source = gmail_source();
    let status = |minutes| health(&db, &[&source], &minute(minutes), &none()).remove(0).status;
    assert_eq!(status(180), HealthStatus::Ok);
    assert_eq!(status(181), HealthStatus::Delayed);
}

#[test]
fn 건강_0건_ok는_정상이고_실패만_이어지면_지연이다() {
    let db = test_db();
    ingest(&db, result(), T0);
    for minutes in [3, 6, 9, 12, 15] {
        let when = minute(minutes);
        let failed =
            SourceResult { outcome: Outcome::Auth, started_at: when.clone(), finished_at: when.clone(), ..result() };
        ingest(&db, failed, &when);
    }
    let source = github_source();
    let row = health(&db, &[&source], &minute(15), &none()).remove(0);
    assert_eq!(row.status, HealthStatus::Delayed);
    assert_eq!(row.last_outcome, Some(Outcome::Auth));
    assert_eq!(row.last_ok_at.as_deref(), Some(T0));
}

#[test]
fn 건강_수집_시간_밖과_쉬는_날은_판정하지_않는다() {
    let db = test_db();
    ingest(&db, result(), T0);
    let source = github_source();
    let status = |now: &str, off_dates: &OffDates| health(&db, &[&source], now, off_dates).remove(0).status;
    assert_eq!(status("2026-03-02T12:00:00.000Z", &none()), HealthStatus::Quiet);
    assert_eq!(status("2026-03-07T02:00:00.000Z", &none()), HealthStatus::Quiet);
    assert_eq!(status(&minute(60), &off(&["2026-03-02"])), HealthStatus::Quiet);
}

#[test]
fn 건강_밤사이_공백은_다음날_09시부터_센다() {
    let db = test_db();
    ingest(&db, SourceResult { finished_at: "2026-03-02T10:55:00.000Z".into(), ..result() }, T0);
    let source = github_source();
    let row = |now: &str| health(&db, &[&source], now, &none()).remove(0);
    assert_eq!(row("2026-03-03T00:05:00.000Z").status, HealthStatus::Ok);
    let late = row("2026-03-03T00:12:00.000Z");
    assert_eq!((late.status, late.delay_minutes), (HealthStatus::Delayed, Some(12)));
}

#[test]
fn 건강_한_줄은_0건이_아니라_지연과_미연결을_말한다() {
    let db = test_db();
    ingest(&db, result(), T0);
    let (github, notion) = (github_source(), notion_source());
    let rows = health(&db, &[&github, &notion], &minute(12), &none());
    assert_eq!(SourceHealthBook::health_line(&rows).as_deref(), Some("GitHub 12분 지연, Notion 미연결"));
    assert_eq!(SourceHealthBook::health_line(&[]), None);
}

fn health_row(
    source: SourceName,
    query: &str,
    status: HealthStatus,
    last_ok_at: Option<&str>,
    delay: Option<i64>,
) -> SourceHealth {
    SourceHealth {
        source,
        query: query.to_owned(),
        last_outcome: None,
        last_error_code: None,
        last_ok_at: last_ok_at.map(str::to_owned),
        delay_minutes: delay,
        note: None,
        status,
    }
}

#[test]
fn 소스별_요약은_가장_나쁜_상태와_그_상태를_정한_질의의_시각을_쓴다() {
    let row = health_row;
    let rows = [
        row(SourceName::Github, "tick", HealthStatus::Ok, Some(T1), None),
        row(SourceName::Slack, "tick", HealthStatus::Quiet, Some(T0), None),
        row(SourceName::Github, "slow", HealthStatus::Delayed, Some(T0), Some(12)),
        row(SourceName::Notion, "tick", HealthStatus::NotConnected, None, None),
    ];
    let summary = SourceHealthBook::summary(&rows);
    let shape: Vec<_> = summary
        .iter()
        .map(|status| {
            (status.source, status.label.as_str(), status.status, status.last_ok_at.as_deref(), status.delay_minutes)
        })
        .collect();
    assert_eq!(
        shape,
        [
            (SourceName::Github, "GitHub", HealthStatus::Delayed, Some(T0), Some(12)),
            (SourceName::Slack, "Slack", HealthStatus::Quiet, Some(T0), None),
            (SourceName::Notion, "Notion", HealthStatus::NotConnected, None, None),
        ]
    );
    assert_eq!(
        serde_json::to_value(&summary[0]).unwrap(),
        json!({ "source": "github", "label": "GitHub", "status": "delayed", "lastOkAt": T0, "delayMinutes": 12 })
    );
    assert!(SourceHealthBook::summary(&[]).is_empty());
}

#[test]
fn 소스별_요약은_주기가_다른_질의의_시각을_섞지_않는다() {
    let one = |rows: &[SourceHealth]| {
        let summary = SourceHealthBook::summary(rows);
        assert_eq!(summary.len(), 1);
        (summary[0].status, summary[0].last_ok_at.clone(), summary[0].delay_minutes)
    };
    let row = |query: &str, status: HealthStatus, last_ok_at: Option<&str>, delay: Option<i64>| {
        health_row(SourceName::Calendar, query, status, last_ok_at, delay)
    };
    // 하루에 한 번 도는 질의가 정상이면 가장 최근에 성공한 질의의 시각을 쓴다. 순서와 무관하다.
    let fresh = Some(T1.to_owned());
    assert_eq!(
        one(&[row("daily", HealthStatus::Ok, Some(T0), None), row("tick", HealthStatus::Ok, Some(T1), None)]),
        (HealthStatus::Ok, fresh.clone(), None)
    );
    assert_eq!(
        one(&[row("tick", HealthStatus::Ok, Some(T1), None), row("daily", HealthStatus::Ok, Some(T0), None)]),
        (HealthStatus::Ok, fresh, None)
    );
    // 늦는 질의가 여럿이면 가장 오래 밀린 시각과 가장 긴 지연을 쓴다.
    assert_eq!(
        one(&[
            row("tick", HealthStatus::Delayed, Some(T1), Some(5)),
            row("slow", HealthStatus::Delayed, Some(T0), Some(40)),
            row("daily", HealthStatus::Ok, Some(T1), None),
        ]),
        (HealthStatus::Delayed, Some(T0.to_owned()), Some(40))
    );
    // 수집 기록이 없는 질의가 상태를 정하면 다른 질의의 시각과 지연을 붙이지 않는다.
    assert_eq!(
        one(&[row("tick", HealthStatus::Ok, Some(T1), None), row("new", HealthStatus::Never, None, None)]),
        (HealthStatus::Never, None, None)
    );
    assert_eq!(
        one(&[row("new", HealthStatus::Never, None, None), row("tick", HealthStatus::Ok, Some(T1), None)]),
        (HealthStatus::Never, None, None)
    );
}

#[test]
fn 지연_분을_모르면_지연_사실만_쓴다() {
    assert_eq!(SourceHealthBook::delay_note("Slack", Some(12)), "Slack 12분 지연");
    assert_eq!(SourceHealthBook::delay_note("Slack", None), "Slack 지연");
}

#[test]
fn 표시_후보는_등급순이고_닫힌_건과_기준선은_빠진다() {
    let db = test_db();
    let first = SourceResult { objects: vec![issue(1)], events: vec![comment("old", T0)], ..result() };
    ingest(&db, first, T0);
    let direct = NormalizedEvent {
        mentions_me: Some(true),
        mention_count: Some(1),
        thread_key: Some("other".into()),
        ..comment("p1", T2)
    };
    let second = SourceResult {
        started_at: T1.into(),
        finished_at: T1.into(),
        watermark: Some(T1.into()),
        events: vec![comment("p2", T1), direct],
        ..result()
    };
    ingest(&db, second, T2);
    let rows = StateBuilder::new(db.connection()).would_show_events().unwrap();
    assert_eq!(rows.iter().map(|row| row.tier).collect::<Vec<_>>(), [Tier::P1, Tier::P2]);
}

#[test]
fn 일시정지_판정() {
    let control = KairosControl::default();
    assert!(!control.is_paused(at(T0)));
    assert!(control.clone().with_paused_until(Some(INDEFINITE)).is_paused(at(T0)));
    assert!(control.clone().with_paused_until(Some(T1)).is_paused(at(T0)));
    assert!(!control.with_paused_until(Some(T0)).is_paused(at(T1)));
}

#[test]
fn 제어_파일은_왕복되고_없으면_기본값이다() {
    let (_dir, home) = temp_home();
    let file = ControlFile::new(&home);
    assert_eq!(file.read(), KairosControl::default());
    let control =
        KairosControl::default().with_paused_until(Some(INDEFINITE)).with_off_dates(&["2026-03-02".to_owned()]);
    file.write(&control).unwrap();
    assert_eq!(file.read(), control);
}

// 아래는 원본에 없는 시험이다. 파일 형식의 경계를 고정한다.

#[test]
fn 제어_파일의_모르는_키는_버리고_빈_파일과_객체가_아닌_값은_기본값이다() {
    let (_dir, home) = temp_home();
    let file = ControlFile::new(&home);
    for broken in ["", "null", "[]", "5", "\"x\"", "{"] {
        fs::write(home.control_path(), broken).unwrap();
        assert_eq!(file.read(), KairosControl::default(), "{broken:?}");
    }
    fs::write(home.control_path(), r#"{"unknown":1,"displayMode":"weird","focus":null,"displayOpenedAt":""}"#).unwrap();
    let read = file.read();
    assert_eq!(read, KairosControl::default());
    assert_eq!(serde_json::to_value(&read).unwrap(), json!({ "pausedUntil": null, "offDates": [] }));
}

#[test]
fn 제어_파일의_키_순서는_저장한_순서를_지킨다() {
    let (_dir, home) = temp_home();
    let file = ControlFile::new(&home);
    let opened = KairosControl::default().with_display_opened_at(T0).with_sessions(Some(T0));
    let focused = opened.with_focus(T0, T1).with_p1_notify(false).with_sessions(Some(T1));
    file.write(&focused).unwrap();
    let text = fs::read_to_string(home.control_path()).unwrap();
    let keys: Vec<&str> = text.lines().filter(|line| line.starts_with("  \"")).map(|line| line.trim()).collect();
    assert_eq!(
        keys[..3],
        [r#""pausedUntil": null,"#, r#""offDates": [],"#, format!(r#""displayOpenedAt": "{T0}","#).as_str()]
    );
    assert!(text.ends_with("}\n"));
    assert!(text.find("focus").unwrap() < text.find("p1NotifyOff").unwrap());
    assert!(text.find("p1NotifyOff").unwrap() < text.find("sessionTestPassedAt").unwrap());
    // 읽으면 정해진 순서로 돌아간다.
    let read = file.read();
    let names: Vec<String> = serde_json::to_value(&read).unwrap().as_object().unwrap().keys().cloned().collect();
    let expected = ["pausedUntil", "offDates", "focus", "displayOpenedAt", "sessionTestPassedAt", "p1NotifyOff"];
    assert_eq!(names, expected);
}

#[test]
fn 깨진_시각의_표시_판단은_nan처럼_거짓이다() {
    let control = KairosControl::default().with_paused_until(Some("언젠가"));
    assert!(!control.is_paused(at(T0)));
}

#[test]
fn 표시가_처음_열리는_순간을_남기고_저장_직전에_다시_읽는다() {
    let (_dir, home) = temp_home();
    let db = test_db();
    let first = SourceResult { started_at: BEFORE.into(), finished_at: BEFORE.into(), ..result() };
    ingest(&db, first, BEFORE);
    let source = github_source();
    let sources: [&dyn Source; 1] = [&source];
    let file = ControlFile::new(&home);
    let stale = KairosControl::default();
    let early = at("2026-03-02T10:59:59.999Z");
    assert_eq!(
        file.latch_display(db.connection(), &sources, early, stale.clone()).unwrap(),
        stale,
        "시작 전에는 쓰지 않는다"
    );
    assert!(!home.control_path().exists());
    // 틱 동안 사용자가 바꾼 값을 덮지 않는다.
    file.write(&KairosControl::default().with_p1_notify(false)).unwrap();
    let now = at("2026-03-02T11:00:00.000Z");
    let opened = file.latch_display(db.connection(), &sources, now, stale).unwrap();
    assert!(opened.display_opened());
    assert!(!opened.p1_notify_on());
    assert_eq!(file.read(), opened);
    assert_eq!(
        file.latch_display(db.connection(), &sources, at("2026-03-03T00:00:00.000Z"), opened.clone()).unwrap(),
        opened
    );
}
