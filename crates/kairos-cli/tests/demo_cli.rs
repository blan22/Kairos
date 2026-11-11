//! `kairos demo`. 계정 없이 보는 합성 데이터 폴더. 외부 실행 파일은 모두 없는 경로를 가리킨다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::*;

#[test]
fn demo는_비어_있는_새_폴더에_합성_항목을_만든다() {
    let mut cli = Cli::new();
    // 부르면 기록을 남기는 가짜 orca와 gh. 데모는 둘 다 부르지 않아야 한다.
    let orca = cli.fake_orca();
    let gh_log = cli.fake_gh();
    let home = cli.root.join("demo");
    let env = [("KAIROS_HOME", home.to_str().unwrap())];
    let made = cli.run_with(&env, &["demo", "--json"]);
    assert_eq!(made.status, 0, "{}{}", made.out, made.err);
    assert_eq!(made.body()["opened"], 4, "{}", made.out);

    // 같은 폴더의 next가 P1 카드 하나를 낸다. 표시 게이트는 열려 있다.
    let next = cli.run_with(&env, &["next", "--json"]);
    assert_eq!(next.status, 0, "{}{}", next.out, next.err);
    let card = &next.body()["card"];
    assert_eq!(card["tier"], "P1", "{}", next.out);
    let items = cli.run_with(&env, &["list", "--items"]);
    assert_eq!(items.out.lines().filter(|line| line.starts_with('#')).count(), 4, "{}", items.out);

    // 설정과 DB는 소유자만 읽는다. 합성 값만 들어 있다.
    for name in ["org.toml", "config.toml", "connections.toml", "kairos.db", "control.json"] {
        let mode = fs::metadata(home.join(name)).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{name}");
    }
    assert_eq!(fs::metadata(&home).unwrap().permissions().mode() & 0o777, 0o700);
    assert!(fs::read_to_string(home.join("org.toml")).unwrap().contains("contoso.example"));
    assert!(!orca.called(), "{}", orca.raw());
    assert!(!gh_log.exists());
}

#[test]
fn demo는_기존_데이터_폴더와_기본_폴더에_쓰지_않는다() {
    let cli = Cli::new();
    // 시험 하네스의 폴더는 설정 파일이 이미 있다.
    let occupied = cli.json(&["demo"]);
    assert_eq!(occupied.status, 1);
    assert_eq!(occupied.body()["error"]["code"], "demo_home_not_empty");
    // KAIROS_HOME을 주지 않으면 기본 폴더로 가므로 거절한다.
    let unset = cli.run_with(&[("KAIROS_HOME", "")], &["demo", "--json"]);
    assert_eq!(unset.status, 2);
    assert_eq!(unset.body()["error"]["code"], "demo_home_required");
    // 공백만 준 값도 기본 폴더로 가므로 거절한다.
    for blank in [" ", "\u{a0}", "\u{feff}"] {
        let spaced = cli.run_with(&[("KAIROS_HOME", blank)], &["demo", "--json"]);
        assert_eq!(spaced.status, 2, "{blank:?}");
        assert_eq!(spaced.body()["error"]["code"], "demo_home_required");
    }
    assert!(!cli.root.join("Library").exists(), "기본 데이터 폴더를 만들면 안 된다");
}
