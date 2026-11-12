//! 항목 명령의 프로세스 시험.
//! 같은 파일의 수집과 doctor 시험은 kairos-tick, 알림 시험은 kairos-agenda가 맡는다.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::Instant;
use kairos_core::model::{NormalizedObject, ObjectKind, SourceName};
use serde_json::{Value, json};

#[test]
fn help에_새_명령이_나온다() {
    let help = Cli::new().run(&["help"]);
    for name in ["next", "accept", "later", "done", "keep", "add", "triage", "focus", "display", "notify"] {
        assert!(help.out.contains(name), "{name}");
    }
}

#[test]
fn next_json_수집_전에는_게이트_사유를_준다() {
    let done = Cli::new().json(&["next"]);
    let body = done.body();
    assert_eq!(done.status, 0);
    assert_eq!((&body["schema"], &body["ok"], &body["card"]), (&json!(1), &json!(true), &Value::Null));
    assert_eq!((&body["display"]["open"], &body["display"]["reason"]), (&json!(false), &json!("no_collection")));
    assert_eq!(body["hold"]["reason"], "no_collection");
}

#[test]
fn display_on_off_auto는_control에_남고_state_json에_반영된다() {
    let cli = Cli::new();
    assert_eq!(cli.json(&["display", "on"]).body()["display"]["mode"], "on");
    assert_eq!(cli.control_json()["displayMode"], "on");
    assert_eq!(cli.state()["gate"]["mode"], "on");
    assert_eq!(cli.json(&["display", "off"]).body()["display"]["reason"], "display_off");
    assert_eq!(cli.json(&["next"]).body()["hold"]["reason"], "display_off");
    assert_eq!(cli.json(&["display", "auto"]).body()["display"]["mode"], "auto");
    assert_eq!(cli.json(&["display"]).body()["display"]["mode"], "auto");
    let bad = cli.json(&["display", "maybe"]);
    let body = bad.body();
    assert_eq!((bad.status, &body["ok"], &body["error"]["code"]), (2, &json!(false), &json!("usage")));
}

#[test]
fn display_on_뒤_next와_list_json이_항목을_내고_문자열은_정제된다() {
    let cli = cli_with_item();
    cli.json(&["display", "on"]);
    let list = cli.json(&["list"]);
    let body = list.body();
    assert_eq!(list.status, 0);
    assert_eq!(body["ok"], true);
    assert_eq!(body["items"].as_array().unwrap().len(), 1);
    assert_eq!(body["items"][0]["title"], "합성 멘션");
    assert_eq!(body["counts"]["open"]["P1"], 1);
    assert!(!list.out.contains("\\u001b"));
    assert_eq!(cli.json(&["next"]).status, 0);
}

#[test]
fn list_기본_출력은_그대로이고_items는_게이트와_무관하게_열린_항목을_보여_준다() {
    let cli = cli_with_item();
    assert!(cli.run(&["list"]).out.contains("저장된 항목 없음"));
    let shown = cli.run(&["list", "--items"]);
    assert_eq!(shown.status, 0);
    assert!(shown.out.contains("#1 P1 답장 필요 Slack 합성 멘션"), "{}", shown.out);
}

#[test]
fn later_done_keep은_항목_id만_받고_결과를_낸다() {
    let cli = cli_with_item();
    let later = cli.json(&["later", "1"]);
    let body = later.body();
    assert_eq!(later.status, 0);
    assert_eq!(body["ok"], true);
    assert_eq!((&body["results"][0]["status"], &body["results"][0]["state"]), (&json!("changed"), &json!("parked")));
    assert_eq!(cli.state()["items"]["parked"], 1);
    assert_eq!(cli.json(&["done", "1"]).body()["results"][0]["state"], "closed");
    assert_eq!(cli.state()["items"]["parked"], 0);
    let again = cli.json(&["done", "1"]);
    let body = again.body();
    assert_eq!(again.status, 0);
    assert_eq!(body["ok"], true);
    assert_eq!(body["results"][0]["status"], "already_closed");
    assert_eq!(body["results"][0]["message"], "이미 처리됨");
    let missing = cli.json(&["later", "99"]);
    let body = missing.body();
    assert_eq!(missing.status, 1);
    assert_eq!(body["ok"], false);
    assert_eq!(body["results"][0]["status"], "not_found");
    assert_eq!(body["error"]["code"], "not_found");
}

#[test]
fn later의_when은_hour와_tomorrow만_받는다() {
    let cli = cli_with_item();
    for bad in ["soon", "next_bundle", "", "1;rm"] {
        let refused = cli.json(&["later", "1", "--when", bad]);
        assert_eq!((refused.status, &refused.body()["error"]["code"]), (2, &json!("usage")), "{bad}");
    }
    assert!(items(&cli.db())[0].parked_until.is_none());
    let before = now();
    let later = cli.json(&["later", "1", "--when", "hour"]);
    let body = later.body();
    assert_eq!((later.status, &body["results"][0]["state"]), (0, &json!("parked")));
    assert_eq!(body["results"][0]["message"], "1시간 뒤에 다시 보여 줍니다");
    let until = Instant::parse(items(&cli.db())[0].parked_until.as_deref().unwrap()).unwrap();
    let waited = until.ms_since(before);
    assert!((3_600_000..3_660_000).contains(&waited), "{waited}");
    let cli = cli_with_item();
    let tomorrow = cli.json(&["later", "1", "--when", "tomorrow"]);
    assert_eq!(tomorrow.body()["results"][0]["message"], "다음 근무일 아침에 다시 보여 줍니다");
}

#[test]
fn next_json은_소스별_수집_상태를_싣는다() {
    let cli = cli_with_item();
    let body = cli.json(&["next"]).body();
    assert_eq!(body["display"]["open"], false, "표시 게이트가 닫혀 있어도 수집 상태는 낸다");
    let sources = body["sources"].as_array().unwrap();
    let labels: Vec<&str> = sources.iter().map(|row| row["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["Calendar", "GitHub", "Slack", "Gmail", "Notion"]);
    let allowed = ["ok", "delayed", "never", "not_connected", "quiet"];
    for row in sources {
        assert!(allowed.contains(&row["status"].as_str().unwrap()), "{row}");
        let keys: Vec<&String> = row.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["source", "label", "status", "lastOkAt", "delayMinutes"], "{row}");
    }
    // 목록 응답은 그대로다. 수집 상태는 next에만 있다.
    assert!(cli.json(&["list"]).body().get("sources").is_none());
}

#[test]
fn next의_seen_ready는_같은_응답을_내고_카드_노출을_적지_않는다() {
    let cli = cli_with_item();
    let plain = cli.json(&["next"]).body();
    let ready = cli.json(&["next", "--seen-ready", "--surface", "panel"]);
    assert_eq!(ready.status, 0);
    let body = ready.body();
    for key in ["ok", "display", "card", "ready", "counts", "budget"] {
        assert_eq!(body[key], plain[key], "{key}");
    }
    let after = cli.json(&["next"]).body();
    assert_eq!((&after["card"], &after["budget"]), (&plain["card"], &plain["budget"]));
    // 호스트 이름 검사는 그대로다.
    assert_eq!(cli.json(&["next", "--seen-ready", "--surface", "bogus"]).status, 2);
}

#[test]
fn keep과_done_drop도_cli로_바뀐다() {
    let cli = cli_with_item();
    assert_eq!(cli.json(&["keep", "1"]).body()["results"][0]["state"], "parked");
    assert_eq!(cli.json(&["done", "2", "--drop"]).body()["results"][0]["state"], "closed");
    let rows: Vec<(String, Option<String>)> = items(&cli.db())
        .into_iter()
        .map(|row| (row.reason.to_string(), row.close_reason.map(|reason| reason.to_string())))
        .collect();
    assert_eq!(rows, [("reply_owed".to_owned(), None), ("work_owed".to_owned(), Some("user_dropped".to_owned()))]);
}

#[test]
fn next_seen과_ack는_같은_형식으로_답하고_surface_값을_검사한다() {
    let cli = cli_with_item();
    cli.json(&["display", "on"]);
    let flag_sets: [&[&str]; 3] = [&["--seen"], &["--ack"], &["--seen", "--surface", "panel"]];
    for flags in flag_sets {
        let mut args = vec!["next"];
        args.extend(flags);
        let done = cli.json(&args);
        let body = done.body();
        assert_eq!((done.status, &body["schema"], &body["ok"]), (0, &json!(1), &json!(true)), "{flags:?}");
        let filled = if body["card"].is_null() {
            !body["hold"]["reason"].as_str().unwrap().is_empty()
        } else {
            !body["card"]["items"].as_array().unwrap().is_empty()
        };
        assert!(filled);
        for key in ["at", "display", "counts", "budget", "focus"] {
            assert!(body.get(key).is_some(), "{key}");
        }
    }
    let bad = cli.json(&["next", "--surface", "band"]);
    assert_eq!((bad.status, &bad.body()["error"]["code"]), (2, &json!("usage")));
}

#[test]
fn 항목_id가_아닌_인자는_거부하고_아무것도_바꾸지_않는다() {
    let cli = cli_with_item();
    for name in ["later", "done", "keep"] {
        for value in ["abc", "1;rm", "$(id)", "0", "-1", "1.5", ""] {
            let child = cli.run(&[name, "--json", "--", value]);
            let body = child.body();
            assert_eq!(
                (child.status, &body["ok"], &body["error"]["code"]),
                (2, &json!(false), &json!("bad_id")),
                "{name} {value}"
            );
        }
        assert_eq!(cli.json(&[name]).body()["error"]["code"], "usage");
    }
    assert_eq!(item(&cli.db(), 1).state.to_string(), "open");
}

#[test]
fn 모르는_옵션도_json_오류로_답한다() {
    let done = Cli::new().json(&["next", "--bogus"]);
    let body = done.body();
    assert_eq!((done.status, &body["ok"], &body["error"]["code"]), (2, &json!(false), &json!("usage")));
}

#[test]
fn 플레이북_대상이_아닌_항목의_accept는_아무것도_바꾸지_않는다() {
    let cli = cli_with_item();
    cli.open_sessions();
    let done = cli.json(&["accept", "1"]);
    let body = done.body();
    assert_eq!((done.status, &body["ok"], &body["error"]["code"]), (1, &json!(false), &json!("not_eligible")));
    let db = cli.db();
    assert_eq!(item(&db, 1).state.to_string(), "open");
    assert_eq!(count(&db, "runs"), 0);
}

#[test]
fn add는_제목을_저장만_하고_빈_입력은_거부한다() {
    let cli = Cli::new();
    let added = cli.json(&["add", "합성\u{1b}[31m 할 일"]);
    assert_eq!(added.status, 0);
    assert_eq!(added.body()["results"][0]["status"], "changed");
    let rows = items(&cli.db());
    assert_eq!(rows[0].title.as_deref(), Some("합성 할 일"));
    assert_eq!((rows[0].reason.to_string(), rows[0].tier.to_string()), ("manual".to_owned(), "P2".to_owned()));
    let empty = cli.json(&["add", "   "]);
    assert_eq!((empty.status, &empty.body()["error"]["code"]), (2, &json!("usage")));
}

#[test]
fn triage_json은_10건까지_내고_keep_drop은_id만_받는다() {
    let cli = Cli::new();
    let at_now = now().to_iso();
    let objects: Vec<NormalizedObject> = (1..=11)
        .map(|index| NormalizedObject {
            state: Some("open".into()),
            assigned_to_me: Some(true),
            authored_by_me: Some(false),
            text_head: Some(format!("합성 이슈 {index}")),
            ..NormalizedObject::new(
                format!("gh:acme/widget#{index}"),
                SourceName::Github,
                ObjectKind::Issue,
                index.to_string(),
            )
        })
        .collect();
    push_objects(&cli.db(), SourceName::Github, &at_now, objects);
    let batch = cli.json(&["triage"]);
    let body = batch.body();
    assert_eq!(batch.status, 0);
    assert_eq!(body["items"].as_array().unwrap().len(), 10);
    assert_eq!(body["remaining"], 1);
    let first = &body["items"][0];
    let ids: Vec<&Value> = first["actions"].as_array().unwrap().iter().map(|action| &action["id"]).collect();
    assert_eq!(ids, [&json!("triage_keep"), &json!("triage_drop")]);
    let id = first["id"].to_string();
    assert_eq!(first["actions"][0]["argv"], json!(["triage", "keep", id, "--json"]));
    assert_eq!(cli.json(&["triage", "keep", &id]).body()["results"][0]["state"], "open");
    let second = body["items"][1]["id"].to_string();
    assert_eq!(cli.json(&["triage", "drop", &second]).body()["results"][0]["state"], "closed");
    let bad = cli.json(&["triage", "maybe", &id]);
    assert_eq!((bad.status, &bad.body()["error"]["code"]), (2, &json!("usage")));
}

#[test]
fn focus_시작_상태_끝() {
    let cli = Cli::new();
    let started = cli.json(&["focus", "50"]);
    let body = started.body();
    assert_eq!(started.status, 0);
    assert_eq!(body["focus"]["active"], true);
    let until = Instant::parse(body["focus"]["until"].as_str().unwrap()).unwrap();
    assert!((until.ms_since(now()) - 50 * 60_000).abs() < 60_000);
    assert_eq!(cli.json(&["focus", "--status"]).body()["focus"]["active"], true);
    assert_eq!(cli.json(&["focus", "--end"]).body()["focus"]["active"], false);
    let bad = cli.json(&["focus", "soon"]);
    assert_eq!((bad.status, &bad.body()["error"]["code"]), (2, &json!("usage")));
}

#[test]
fn state_json은_건수와_게이트만_담고_제목은_담지_않는다() {
    let cli = cli_with_item();
    cli.json(&["display", "on"]);
    let state = cli.state();
    assert_eq!((&state["items"]["open"]["P1"], &state["items"]["badge"]), (&json!(1), &json!(1)));
    assert!(!state.to_string().contains("합성 멘션"));
    cli.json(&["display", "off"]);
    let state = cli.state();
    assert_eq!(state["items"]["badge"], 0, "게이트가 닫히면 표시 건수는 0이다");
    assert_eq!(state["items"]["open"]["P1"], 1);
}
