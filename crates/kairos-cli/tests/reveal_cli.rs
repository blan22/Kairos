//! 알림을 누르면 헬퍼가 부르는 reveal과 next의 reveal 필드. 가짜 orca와 open만 부른다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::*;
use kairos_agenda::{ORCA_BUNDLE_ID, PANEL_OPEN_ARGS, REVEAL_FILE, REVEAL_WINDOW_MS};
use serde_json::{Value, json};

/// 인자를 줄마다 남기고 정해 둔 코드로 끝나는 가짜 실행 파일. 환경 값으로 넘기고 기록 파일을 준다.
fn fake(cli: &mut Cli, key: &str, name: &str, code: i32) -> PathBuf {
    let (bin, log) = (cli.root.join(name), cli.root.join(format!("{name}.log")));
    write_script(&bin, &format!("#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\nexit {code}\n", log.display()));
    cli.set(key, bin.to_str().unwrap());
    log
}

fn lines(log: &Path) -> Vec<String> {
    fs::read_to_string(log).unwrap_or_default().lines().map(str::to_owned).collect()
}

fn reveal_file(cli: &Cli) -> PathBuf {
    cli.root.join(REVEAL_FILE)
}

#[test]
fn reveal은_파일을_0600으로_쓰고_패널을_연_뒤_orca를_앞으로_끌어온다() {
    let mut cli = Cli::new();
    let orca = fake(&mut cli, "KAIROS_ORCA", "fake-orca", 0);
    let open = fake(&mut cli, "KAIROS_OPEN", "fake-open", 0);
    let before = now().millis();
    let done = cli.json(&["reveal", "3", "7", "3"]);
    let after = now().millis();
    assert_eq!(done.status, 0, "{}", done.err);
    let body = done.body();
    assert_eq!((&body["schema"], &body["ok"]), (&json!(1), &json!(true)));
    assert_eq!(body["reveal"]["items"], json!([3, 7]), "같은 ID는 한 번만 둔다");
    let at = body["reveal"]["at"].as_i64().unwrap();
    assert!((before..=after).contains(&at), "{at}");
    assert_eq!((&body["panelOpened"], &body["activated"]), (&json!(true), &json!(true)));
    assert_eq!(lines(&orca), PANEL_OPEN_ARGS);
    assert_eq!(lines(&open), ["-b", ORCA_BUNDLE_ID]);
    let written: Value = serde_json::from_str(&fs::read_to_string(reveal_file(&cli)).unwrap()).unwrap();
    assert_eq!(written, json!({ "items": [3, 7], "at": at }));
    assert_eq!(fs::metadata(reveal_file(&cli)).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn reveal은_orca를_열지_못해도_파일을_남기고_성공으로_답한다() {
    let mut cli = Cli::new();
    fake(&mut cli, "KAIROS_ORCA", "fake-orca", 1);
    let done = cli.json(&["reveal", "5"]);
    assert_eq!(done.status, 0, "{}", done.err);
    let body = done.body();
    assert_eq!((&body["panelOpened"], &body["activated"]), (&json!(false), &json!(false)));
    assert!(reveal_file(&cli).exists());
    let text = cli.run(&["reveal", "5"]);
    assert_eq!((text.status, text.out.as_str()), (0, "패널을 열지 못했습니다. Orca에서 kairos 패널을 여세요\n"));
    fake(&mut cli, "KAIROS_ORCA", "fake-orca-ok", 0);
    assert_eq!(cli.run(&["reveal", "5", "6"]).out, "알림의 항목 2건을 패널에 보여 줍니다\n");
}

#[test]
fn reveal은_orca가_꺼져_있으면_띄운_뒤_패널을_다시_연다() {
    let mut cli = Cli::new();
    // 가짜 open이 남긴 표시가 있어야 성공한다. Orca가 뜨기 전의 plugin open처럼 처음에는 실패한다.
    let started = cli.root.join("orca-started");
    let (orca, orca_log) = (cli.root.join("fake-orca"), cli.root.join("fake-orca.log"));
    write_script(
        &orca,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n[ -e '{}' ] || exit 1\nexit 0\n",
            orca_log.display(),
            started.display()
        ),
    );
    cli.set("KAIROS_ORCA", orca.to_str().unwrap());
    let open = cli.root.join("fake-open");
    write_script(&open, &format!("#!/bin/sh\n: > '{}'\nexit 0\n", started.display()));
    cli.set("KAIROS_OPEN", open.to_str().unwrap());
    let done = cli.json(&["reveal", "12"]);
    assert_eq!(done.status, 0, "{}", done.err);
    let body = done.body();
    assert_eq!((&body["panelOpened"], &body["activated"]), (&json!(true), &json!(true)));
    assert_eq!(lines(&orca_log), ["plugin", "plugin"], "띄우기 전 한 번, 띄운 뒤 한 번");
}

#[test]
fn reveal은_틀린_id나_21개_이상이면_아무것도_부르지_않고_2로_끝난다() {
    let mut cli = Cli::new();
    let orca = fake(&mut cli, "KAIROS_ORCA", "fake-orca", 0);
    let open = fake(&mut cli, "KAIROS_OPEN", "fake-open", 0);
    let many: Vec<String> = (1..=21).map(|id| id.to_string()).collect();
    let many: Vec<&str> = std::iter::once("reveal").chain(many.iter().map(String::as_str)).collect();
    for (args, code) in [
        (vec!["reveal"], "usage"),
        (vec!["reveal", "0"], "bad_id"),
        (vec!["reveal", "3", "x"], "bad_id"),
        (vec!["reveal", "1234567890123456"], "bad_id"),
        (many, "usage"),
    ] {
        let done = cli.json(&args);
        assert_eq!((done.status, &done.body()["error"]["code"]), (2, &json!(code)), "{args:?}");
    }
    assert!(!reveal_file(&cli).exists());
    assert!(lines(&orca).is_empty() && lines(&open).is_empty());
}

#[test]
fn next는_10분_안의_reveal만_싣고_없거나_깨졌으면_키가_없다() {
    let mut cli = Cli::new();
    assert!(cli.json(&["next"]).body().get("reveal").is_none(), "파일이 없다");
    fake(&mut cli, "KAIROS_ORCA", "fake-orca", 0);
    let written = cli.json(&["reveal", "4", "2"]).body()["reveal"].clone();
    assert_eq!(cli.json(&["next"]).body()["reveal"], written);
    let stale = now().millis() - REVEAL_WINDOW_MS - 60_000;
    let future = now().millis() + 60_000;
    for content in [
        format!("{{\"items\":[4],\"at\":{stale}}}"),
        format!("{{\"items\":[4],\"at\":{future}}}"),
        format!("{{\"items\":[],\"at\":{}}}", now().millis()),
        format!("{{\"items\":[0],\"at\":{}}}", now().millis()),
        "깨진 파일".to_owned(),
    ] {
        fs::write(reveal_file(&cli), &content).unwrap();
        let body = cli.json(&["next"]).body();
        assert_eq!((&body["schema"], body.get("reveal")), (&json!(1), None), "{content}");
    }
}
