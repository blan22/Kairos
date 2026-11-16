//! 새 판 확인과 설치(`update --check|--apply`, `next`와 `hello`의 `update`)의 프로세스 시험.
//! gh는 가짜다. 받기는 시험이 만든 묶음을 놓고, 빌드 증명은 표식 파일이 있으면 거절한다.
//! 묶음 안의 kairos도 가짜라 받은 인자만 적는다. 실제 설치와 등록은 일어나지 않는다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::*;
use kairos_core::platform::Os;
use serde_json::json;

const NEWER: &str = "999.0.0";

fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 수집 틱이 남기는 것과 같은 모양의 확인 기록.
fn write_record(cli: &Cli, latest: &str) {
    let body = json!({ "checkedAt": T0, "ok": true, "latest": latest, "notified": null });
    fs::write(cli.root.join("update.json"), body.to_string()).unwrap();
}

/// 이 컴퓨터의 릴리스 묶음 이름. 묶음이 없는 OS와 CPU(Intel Mac)면 None이다.
fn asset() -> Option<&'static str> {
    kairos_ops::asset_name(Os::current(), std::env::consts::ARCH)
}

struct FakeRelease {
    gh_log: PathBuf,
    finish_log: PathBuf,
    /// 있으면 빌드 증명을 거절한다.
    deny: PathBuf,
}

/// 가짜 gh와 그 gh가 내려 줄 묶음. 묶음의 kairos는 받은 인자를 적고 끝난다.
fn fake_release(cli: &mut Cli, latest: &str) -> FakeRelease {
    let root = cli.root.clone();
    let (gh_log, finish_log, deny) = (root.join("gh.log"), root.join("finish.log"), root.join("deny-attestation"));
    let served = root.join("served");
    let stage = root.join("stage");
    let top = format!("kairos-{latest}");
    fs::create_dir_all(stage.join(&top).join("bin")).unwrap();
    fs::create_dir_all(&served).unwrap();
    let new_kairos = format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n", finish_log.display());
    write_script(&stage.join(&top).join("bin").join("kairos"), &new_kairos);
    fs::write(stage.join(&top).join("VERSION"), format!("{latest}\n")).unwrap();
    let asset = asset().unwrap();
    let tarball = served.join(asset);
    let made = Command::new("/usr/bin/tar")
        .args(["-czf", tarball.to_str().unwrap(), "-C", stage.to_str().unwrap(), &top])
        .status()
        .unwrap();
    assert!(made.success());
    let sum = Command::new("/usr/bin/shasum").args(["-a", "256", tarball.to_str().unwrap()]).output();
    let sum = match sum {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).into_owned(),
        _ => String::from_utf8_lossy(&Command::new("sha256sum").arg(&tarball).output().unwrap().stdout).into_owned(),
    };
    fs::write(served.join(format!("{asset}.sha256")), sum).unwrap();
    let script = format!(
        r#"#!/bin/sh
printf '%s\n' "$*" >> '{log}'
case "$1" in
api) printf 'v{latest}\n' ;;
release)
  dir=""; prev=""
  for arg in "$@"; do [ "$prev" = "--dir" ] && dir="$arg"; prev="$arg"; done
  cp '{served}'/* "$dir"/ ;;
attestation) [ -f '{deny}' ] && exit 1; exit 0 ;;
*) exit 1 ;;
esac
"#,
        log = gh_log.display(),
        served = served.display(),
        deny = deny.display(),
    );
    let bin = root.join("fake-gh");
    write_script(&bin, &script);
    cli.set("KAIROS_GH", bin.to_str().unwrap());
    fs::write(root.join("install-source"), "release\n").unwrap();
    FakeRelease { gh_log, finish_log, deny }
}

fn lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path).unwrap_or_default().lines().map(str::to_owned).collect()
}

#[test]
fn 적힌_새_판을_update와_next와_hello가_알린다() {
    let cli = Cli::new();
    // 기록이 없으면 키도 없다.
    assert_eq!(cli.json(&["next"]).body().get("update"), None);
    assert_eq!(cli.json(&["hello"]).body().get("update"), None);
    assert!(cli.run(&["update"]).out.contains("최신 판: 아직 확인하지 않음"));

    write_record(&cli, NEWER);
    let expected = json!({ "current": current(), "latest": NEWER });
    assert_eq!(cli.json(&["next"]).body()["update"], expected);
    assert_eq!(cli.json(&["hello"]).body()["update"], expected);
    fs::write(cli.root.join("install-source"), "release\n").unwrap();
    let shown = cli.run(&["update"]);
    assert!(shown.out.contains(&format!("최신 판: {NEWER} (")), "{}", shown.out);
    assert!(shown.out.contains(&format!("새 판 {NEWER}이 있습니다")), "{}", shown.out);
    assert!(shown.out.contains("\n  kairos update --apply\n"), "{}", shown.out);
    let body = cli.json(&["update"]).body();
    assert_eq!((body["update"]["latest"].as_str(), body["update"]["available"].as_bool()), (Some(NEWER), Some(true)));
    assert_eq!(body["update"]["checkedAt"], T0);
    // 파이프로 부른 명령에는 안내 줄이 붙지 않는다.
    assert_eq!(cli.run(&["list"]).err, "");

    // 지금 판과 같으면 새 판이 아니다.
    write_record(&cli, current());
    assert_eq!(cli.json(&["next"]).body().get("update"), None);
    assert_eq!(cli.json(&["update"]).body()["update"]["available"], false);
    assert!(!cli.run(&["update"]).out.contains("kairos update --apply"));
}

#[test]
fn 확인을_끄면_적힌_새_판도_알리지_않는다() {
    let cli = Cli::new();
    write_record(&cli, NEWER);
    let config = cli.root.join("config.toml");
    let text = fs::read_to_string(&config).unwrap();
    fs::write(&config, format!("{text}\n[updates]\ncheck = false\n")).unwrap();
    assert_eq!(cli.json(&["next"]).body().get("update"), None);
    assert_eq!(cli.json(&["hello"]).body().get("update"), None);
    let shown = cli.json(&["config", "show"]).body();
    assert_eq!(
        (&shown["profile"]["updateCheck"], &shown["origins"]["updates.check"]),
        (&json!(false), &json!("config.toml"))
    );
}

#[test]
fn check는_gh로_지금_묻고_기록을_남긴다() {
    if asset().is_none() {
        return;
    }
    let mut cli = Cli::new();
    let release = fake_release(&mut cli, NEWER);
    let checked = cli.run(&["update", "--check"]);
    assert_eq!(checked.status, 0, "{}{}", checked.out, checked.err);
    assert!(checked.out.contains(&format!("최신 판: {NEWER} (")), "{}", checked.out);
    assert_eq!(lines(&release.gh_log), ["api repos/blan22/Kairos/releases/latest --jq .tag_name"]);
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cli.root.join("update.json")).unwrap()).unwrap();
    assert_eq!((record["ok"].as_bool(), record["latest"].as_str()), (Some(true), Some(NEWER)));
    // gh가 없으면 묻지 않고 적힌 기록을 보여 준다.
    cli.set("KAIROS_GH", cli.root.join("no-such-gh").to_str().unwrap());
    assert!(cli.run(&["update", "--check"]).out.contains(&format!("최신 판: {NEWER}, 마지막 확인은 실패")));
}

#[test]
fn apply는_증명을_확인한_묶음의_finish를_띄우고_작업_폴더를_지운다() {
    if asset().is_none() {
        return;
    }
    let mut cli = Cli::new();
    let release = fake_release(&mut cli, NEWER);
    let applied = cli.run(&["update", "--apply"]);
    assert_eq!(applied.status, 0, "{}{}", applied.out, applied.err);
    assert!(applied.out.contains(&format!("kairos {} → {NEWER}", current())), "{}", applied.out);
    assert!(applied.out.contains("릴리스 워크플로가 만든 묶음임을 확인했습니다"), "{}", applied.out);
    assert_eq!(lines(&release.finish_log), ["update --finish"]);
    assert!(!cli.root.join("update-work").exists());
    let calls = lines(&release.gh_log);
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert!(
        calls[1].starts_with(&format!("release download v{NEWER} --repo blan22/Kairos --pattern {}", asset().unwrap()))
    );
    let attestation = &calls[2];
    for part in [
        "attestation verify ",
        "--repo blan22/Kairos",
        &format!("--cert-identity https://github.com/blan22/Kairos/.github/workflows/release.yml@refs/tags/v{NEWER}"),
        &format!("--source-ref refs/tags/v{NEWER}"),
        "--deny-self-hosted-runners",
    ] {
        assert!(attestation.contains(part), "{attestation}");
    }
}

#[test]
fn apply는_증명이_없거나_이미_최신이면_설치하지_않는다() {
    if asset().is_none() {
        return;
    }
    let mut cli = Cli::new();
    let release = fake_release(&mut cli, NEWER);
    fs::write(&release.deny, "").unwrap();
    let denied = cli.run(&["update", "--apply"]);
    assert_eq!(denied.status, 1, "{}{}", denied.out, denied.err);
    assert!(denied.out.contains("빌드 증명을 확인하지 못했습니다"), "{}", denied.out);
    assert!(lines(&release.finish_log).is_empty());
    assert!(!cli.root.join("update-work").exists());

    let mut same = Cli::new();
    let release = fake_release(&mut same, current());
    let latest = same.run(&["update", "--apply"]);
    assert_eq!(latest.status, 0, "{}{}", latest.out, latest.err);
    assert!(latest.out.contains("이미 최신 판입니다"), "{}", latest.out);
    assert_eq!(lines(&release.gh_log).len(), 1);
}

#[test]
fn apply는_세션과_에이전트_안에서_거절하고_json을_받지_않는다() {
    let mut cli = Cli::new();
    if asset().is_some() {
        fake_release(&mut cli, NEWER);
    }
    for marker in ["CLAUDECODE", "ORCA_TERMINAL_HANDLE", "KAIROS_SESSION"] {
        let refused = cli.run_with(&[(marker, "1")], &["update", "--apply"]);
        assert_eq!(refused.status, 1, "{marker}");
        assert!(refused.out.contains("일반 터미널 앱에서 kairos update --apply"), "{marker} {}", refused.out);
        let finish = cli.run_with(&[(marker, "1")], &["update", "--finish"]);
        assert_eq!(finish.status, 1, "{marker}");
    }
    assert!(!cli.root.join("gh.log").exists());
    assert_eq!(cli.run(&["update", "--apply", "--json"]).status, 2);
    assert_eq!(cli.run(&["update", "--apply", "--finish"]).status, 2);
}

#[test]
fn 소스_설치본의_apply는_저장소의_설치_스크립트를_띄운다() {
    let cli = Cli::new();
    let repo = cli.root.join("my repo");
    let script = if cfg!(target_os = "macos") { "install.sh" } else { "install-linux.sh" };
    fs::create_dir_all(repo.join("scripts")).unwrap();
    let log = cli.root.join("script.log");
    write_script(
        &repo.join("scripts").join(script),
        &format!("#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 3\n", log.display()),
    );
    fs::write(cli.root.join("install-source"), format!("source:{}\n", repo.display())).unwrap();
    let applied = cli.run(&["update", "--apply"]);
    // 스크립트의 종료 코드를 그대로 돌려준다.
    assert_eq!(applied.status, 3, "{}{}", applied.out, applied.err);
    assert_eq!(lines(&log), ["--update"]);

    fs::write(cli.root.join("install-source"), "source:/synthetic/missing\n").unwrap();
    let missing = cli.run(&["update", "--apply"]);
    assert_eq!(missing.status, 1);
    assert!(missing.out.contains("설치 스크립트를 찾지 못했습니다"), "{}", missing.out);
    fs::remove_file(cli.root.join("install-source")).unwrap();
    assert_eq!(cli.run(&["update", "--apply"]).status, 1);
}
