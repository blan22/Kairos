//! setup의 설치 뒤 단계(알림 권한, Orca 패널)와 `kairos notify setup|test`. 실제 Orca, 알림 헬퍼, lsregister, open은 부르지 않는다.
// 가짜 실행 파일(sh)과 unix 권한을 쓰는 시험이다. Windows는 단위 시험과 Windows 백엔드 시험으로 본다.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;

const FINGERPRINT: &str = "ap1-0123456789abcdef0123456789abcdef";

struct Fakes {
    cli: Cli,
    log: PathBuf,
    /// macOS 시험만 읽는다.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    panel: PathBuf,
}

impl Fakes {
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_owned).collect()
    }
}

/// 데이터 폴더에 설치본 모양(`current -> 0.2.0`)과 가짜 헬퍼, lsregister, open, orca를 둔다. 호출은 한 파일에 남는다.
/// `status`는 헬퍼의 권한 상태, `orca_approved`는 승인 없이 올라가는지다.
/// 데이터 폴더에 `post-exit` 파일이 있으면 헬퍼의 `post`가 그 안의 코드로 끝난다.
fn fakes(status: &str, orca_approved: bool) -> Fakes {
    let mut cli = Cli::new();
    let root = cli.root.clone();
    let log = root.join("calls.log");
    let runtime = root.join("0.2.0");
    let macos = runtime.join("ops/notifier/KairosNotifier.app/Contents/MacOS");
    fs::create_dir_all(&macos).unwrap();
    let panel = runtime.join("plugins/orca-panel");
    fs::create_dir_all(&panel).unwrap();
    std::os::unix::fs::symlink("0.2.0", root.join("current")).unwrap();
    let log_text = log.to_str().unwrap();
    let post_exit = root.join("post-exit");
    let post_exit = post_exit.to_str().unwrap();
    let notifier = macos.join("KairosNotifier");
    write_script(
        &notifier,
        &format!(
            "#!/bin/sh\nprintf 'notifier %s\\n' \"$*\" >> '{log_text}'\ncase \"$1\" in\nstatus) printf '{status}\\n' ;;\npost) [ -f '{post_exit}' ] && exit \"$(cat '{post_exit}')\" ;;\nesac\nexit 0\n"
        ),
    );
    let open = root.join("open");
    write_script(&open, &format!("#!/bin/sh\nprintf 'open %s\\n' \"$*\" >> '{log_text}'\n"));
    let lsregister = root.join("lsregister");
    write_script(&lsregister, &format!("#!/bin/sh\nprintf 'lsregister %s\\n' \"$*\" >> '{log_text}'\n"));
    let orca = root.join("orca");
    let pending = if orca_approved {
        "printf 'local.kairos 0.1.0 loaded\\n'\nexit 0".to_owned()
    } else {
        format!(
            "printf 'local.kairos 0.1.0 needs approval before it can run.\\n- Runs its own code in a separate Node.js process\\nFingerprint: {FINGERPRINT}\\nNext step: To approve, run: orca plugin dev load x --approve {FINGERPRINT}\\n'\nexit 3"
        )
    };
    write_script(
        &orca,
        &format!(
            "#!/bin/sh\nprintf 'orca %s\\n' \"$*\" >> '{log_text}'\ncase \"$*\" in\n*--approve*) printf 'approved\\n'; exit 0 ;;\n'plugin restart '*) exit 0 ;;\nesac\n{pending}\n"
        ),
    );
    cli.set("KAIROS_NOTIFIER", notifier.to_str().unwrap());
    cli.set("KAIROS_LSREGISTER", lsregister.to_str().unwrap());
    cli.set("KAIROS_OPEN", open.to_str().unwrap());
    cli.set("KAIROS_ORCA", orca.to_str().unwrap());
    let panel = fs::canonicalize(panel).unwrap();
    Fakes { cli, log, panel }
}

/// setup의 앞 네 단계는 Enter로 지금 값을 지키고 설치는 하지 않는다. 그 뒤에 `tail`을 넘긴다.
#[cfg(target_os = "macos")]
fn answers(tail: &str) -> Vec<u8> {
    // 소스 다섯, GitHub 다시 읽기, Google 주소, Slack ID, 이름, 설치.
    format!("{}{tail}", "\n".repeat(10)).into_bytes()
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn setup은_알림_권한을_받고_패널을_승인한다() {
    let fakes = fakes("notDetermined", false);
    // 적재가 쓴 타입 파일이다. 승인 해시에는 빠지고 워커 검사에는 들어가므로 setup이 지운다.
    fs::create_dir_all(fakes.panel.join(".orca/types/orca")).unwrap();
    fs::write(fakes.panel.join(".orca/types/orca/index.d.ts"), "export {}\n").unwrap();
    // 알림은 묻지 않는다. macOS 권한 창이 묻는다. 패널 예(Enter), 승인 y.
    let done = fakes.cli.pipe(&["setup"], &answers("\ny\n"));
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("5/5 알림과 Orca 패널"), "{}", done.out);
    assert!(!done.out.contains("알림 권한을 받을까요"), "{}", done.out);
    assert!(done.out.contains("권한 창에서 '허용'을 누르세요"), "{}", done.out);
    assert!(done.out.contains("Mac 알림 권한을 받았습니다"), "{}", done.out);
    // Orca가 보여 준 권한 문구는 띄우고, 명령 안내 줄은 뺀다.
    assert!(done.out.contains("Runs its own code"), "{}", done.out);
    assert!(!done.out.contains("Next step"), "{}", done.out);
    assert!(done.out.contains("Orca 패널을 승인했습니다"), "{}", done.out);
    let panel = fakes.panel.to_str().unwrap();
    let calls = fakes.calls();
    let notifier_app = fakes.panel.parent().unwrap().parent().unwrap().join("ops/notifier/KairosNotifier.app");
    assert_eq!(
        calls,
        [
            "notifier status".to_owned(),
            format!("lsregister -f {}", notifier_app.to_str().unwrap()),
            "notifier setup".to_owned(),
            format!("orca plugin dev load {panel}"),
            format!("orca plugin dev load {panel} --approve {FINGERPRINT}"),
            // 타입 파일을 지운 뒤 워커를 새로 띄운다.
            "orca plugin restart local.kairos".to_owned(),
        ],
        "{}",
        done.out
    );
    assert!(!fakes.panel.join(".orca").exists());
    assert!(fakes.panel.is_dir());
}

#[test]
fn setup은_묻지_않으면_패널을_승인하지_않는다() {
    let fakes = fakes("notDetermined", false);
    // 입력이 끝나면 기본값이다. 적재는 예, 승인은 아니오다.
    let done = fakes.cli.pipe(&["setup"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("승인하지 않았습니다"), "{}", done.out);
    assert!(done.out.contains("kairos setup을 다시 실행"), "{}", done.out);
    // 승인하지 않았으면 워커를 건드리지 않는다.
    assert!(
        fakes.calls().iter().all(|call| !call.contains("--approve") && !call.contains("restart")),
        "{:?}",
        fakes.calls()
    );
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn setup은_이미_받은_권한과_승인을_다시_묻지_않는다() {
    let fakes = fakes("authorized", true);
    let done = fakes.cli.pipe(&["setup"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("권한이 이미 있습니다"), "{}", done.out);
    assert!(done.out.contains("Orca 패널을 올렸습니다"), "{}", done.out);
    let calls = fakes.calls();
    // 이미 승인돼 있어도 적재가 타입 파일을 다시 쓴다. 지우고 워커를 새로 띄운다.
    assert_eq!(calls.len(), 3, "{calls:?}");
    assert_eq!(calls[0], "notifier status");
    assert!(calls[1].starts_with("orca plugin dev load "), "{calls:?}");
    assert_eq!(calls[2], "orca plugin restart local.kairos");
}

#[test]
fn setup은_세션_표지가_있으면_승인까지_가지_않는다() {
    let fakes = fakes("denied", false);
    let done = fakes.cli.run_with(&[("KAIROS_SESSION", "1")], &["setup"]);
    assert_eq!(done.status, 1, "{}{}", done.out, done.err);
    assert!(fakes.calls().is_empty(), "{:?}", fakes.calls());
}

#[test]
fn setup은_설치본이_없으면_다섯째_단계를_건너뛴다() {
    let cli = Cli::new();
    let done = cli.pipe(&["setup"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("설치본이 없어 건너뜁니다"), "{}", done.out);
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn setup_yes는_묻지_않고_계정을_기록하고_패널까지_승인한다() {
    let fakes = fakes("notDetermined", false);
    let done = fakes
        .cli
        .pipe(&["setup", "--yes", "--no-install", "--google", "someone@example.com", "--slack", "U0TEST1234"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    // 입력을 읽지 않는다. 묻는 줄([Y/n])이 없다.
    assert!(!done.out.contains("[Y/n]") && !done.out.contains("[y/N]"), "{}", done.out);
    assert!(done.out.contains("Orca 패널을 승인했습니다"), "{}", done.out);
    let calls = fakes.calls();
    let panel = fakes.panel.to_str().unwrap();
    assert!(calls.contains(&"notifier setup".to_owned()), "{calls:?}");
    assert_eq!(calls[calls.len() - 2], format!("orca plugin dev load {panel} --approve {FINGERPRINT}"));
    assert_eq!(calls.last().unwrap(), "orca plugin restart local.kairos");
    let connections = fs::read_to_string(fakes.cli.root.join("connections.toml")).unwrap();
    assert!(connections.contains("someone@example.com"), "{connections}");
    assert!(connections.contains("U0TEST1234"), "{connections}");
    // 설치하지 않으면 표시 모드를 건드리지 않는다. 표시는 설치 단계가 켠다.
    assert!(!done.out.contains("표시를 켰습니다"), "{}", done.out);
    let control = fs::read_to_string(fakes.cli.root.join("control.json")).unwrap_or_default();
    assert!(!control.contains("displayMode"), "{control}");
}

#[test]
fn setup_yes는_실행_파일이_든_설치_자료의_조직_프로필을_들인다() {
    let cli = Cli::new();
    fs::remove_file(cli.root.join("org.toml")).unwrap();
    let bin = cli.asset_bin(&[("contoso.toml", EXAMPLE_ORG)]);
    let done = cli.pipe_installed(&bin, &["setup", "--yes", "--no-install"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("--yes라 묻지 않고 들입니다"), "{}", done.out);
    assert_eq!(fs::read_to_string(cli.root.join("org.toml")).unwrap(), EXAMPLE_ORG);
}

#[test]
fn setup_yes는_조직_프로필이_없거나_여럿이면_들이지_않는다() {
    for profiles in [&[][..], &[("a.toml", EXAMPLE_ORG), ("b.toml", EXAMPLE_ORG)]] {
        let cli = Cli::new();
        fs::remove_file(cli.root.join("org.toml")).unwrap();
        let bin = cli.asset_bin(profiles);
        let done = cli.pipe_installed(&bin, &["setup", "--yes", "--no-install"], b"");
        assert_eq!(done.status, 0, "{}{}", done.out, done.err);
        assert!(!cli.root.join("org.toml").exists(), "{}", done.out);
        assert_eq!(done.out.contains("조직 프로필이 여럿입니다"), profiles.len() > 1, "{}", done.out);
    }
}

#[test]
fn setup_yes는_모양이_틀린_계정을_기록하지_않는다() {
    let fakes = fakes("authorized", true);
    let done = fakes.cli.pipe(&["setup", "--yes", "--no-install", "--slack", "not an id"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("저장하지 않음"), "{}", done.out);
    let connections = fs::read_to_string(fakes.cli.root.join("connections.toml")).unwrap_or_default();
    assert!(!connections.contains("not an id"), "{connections}");
}

#[cfg(target_os = "macos")]
const SETTINGS: &str =
    "open x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=dev.kairos.notifier";

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn setup은_거절된_알림에_권한_창_대신_시스템_설정을_연다() {
    let fakes = fakes("denied", true);
    // 시스템 설정 예(Enter), 패널 예(Enter).
    let done = fakes.cli.pipe(&["setup"], &answers("\n\n"));
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("권한 창을 다시 띄우지 않습니다"), "{}", done.out);
    let calls = fakes.calls();
    assert!(calls.contains(&SETTINGS.to_owned()), "{calls:?}");
    assert!(!calls.contains(&"notifier setup".to_owned()), "{calls:?}");
    assert!(calls.iter().all(|call| !call.starts_with("lsregister")), "{calls:?}");
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn setup은_시스템_설정을_열지_않겠다고_하거나_yes면_열지_않는다() {
    let declined = fakes("denied", true);
    assert_eq!(declined.cli.pipe(&["setup"], &answers("n\n\n")).status, 0);
    assert!(declined.calls().iter().all(|call| !call.starts_with("open ")), "{:?}", declined.calls());
    // 입력이 끝났으면 사람이 없다. 화면을 띄우지 않는다.
    let unattended = fakes("denied", true);
    assert_eq!(unattended.cli.pipe(&["setup"], b"").status, 0);
    assert!(unattended.calls().iter().all(|call| !call.starts_with("open ")), "{:?}", unattended.calls());
    let yes = fakes("denied", true);
    let done = yes.cli.pipe(&["setup", "--yes", "--no-install"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("시스템 설정 > 알림 > kairos"), "{}", done.out);
    assert!(yes.calls().iter().all(|call| !call.starts_with("open ")), "{:?}", yes.calls());
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn notify_setup은_묻지_않고_권한_창을_띄운다() {
    let fakes = fakes("notDetermined", true);
    let done = fakes.cli.run(&["notify", "setup"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("Mac 알림 권한을 받았습니다"), "{}", done.out);
    let app = fakes.panel.parent().unwrap().parent().unwrap().join("ops/notifier/KairosNotifier.app");
    assert_eq!(
        fakes.calls(),
        ["notifier status".to_owned(), format!("lsregister -f {}", app.to_str().unwrap()), "notifier setup".to_owned()]
    );
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn notify_setup은_이미_받았으면_그대로_두고_거절됐으면_시스템_설정을_연다() {
    let granted = fakes("authorized", true);
    let done = granted.cli.run(&["notify", "setup"]);
    assert_eq!((done.status, done.out.as_str()), (0, "Mac 알림: 권한이 이미 있습니다\n"));
    assert_eq!(granted.calls(), ["notifier status"]);

    let denied = fakes("denied", true);
    let done = denied.cli.run(&["notify", "setup"]);
    assert_eq!(done.status, 1, "{}{}", done.out, done.err);
    assert!(done.out.contains("시스템 설정의 알림 화면을 열었습니다"), "{}", done.out);
    assert_eq!(denied.calls(), ["notifier status", SETTINGS]);
    // --json의 표준 출력은 JSON 한 덩어리다.
    let json = denied.cli.run(&["notify", "setup", "--json"]);
    assert_eq!(json.body()["notify"], serde_json::json!({ "permission": "denied" }));
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn notify_test는_고정_문구의_알림을_보내고_권한이_없으면_알린다() {
    let fakes = fakes("authorized", true);
    let done = fakes.cli.run(&["notify", "test"]);
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("시험 알림을 보냈습니다"), "{}", done.out);
    let calls = fakes.calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    let payload: serde_json::Value = serde_json::from_str(calls[0].strip_prefix("notifier post ").unwrap()).unwrap();
    assert_eq!(
        (&payload["title"], &payload["tier"], &payload["items"]),
        (&"kairos".into(), &"P1".into(), &serde_json::json!([]))
    );

    fs::write(fakes.cli.root.join("post-exit"), "3").unwrap();
    let refused = fakes.cli.run(&["notify", "test"]);
    assert_eq!(refused.status, 1);
    assert!(refused.out.contains("kairos notify setup"), "{}", refused.out);
    fs::write(fakes.cli.root.join("post-exit"), "5").unwrap();
    let failed = fakes.cli.run(&["notify", "test", "--json"]);
    assert_eq!((failed.status, &failed.body()["error"]["code"]), (1, &serde_json::json!("notify_failed")));
}

/// macOS 알림 헬퍼(KairosNotifier.app)의 흐름이다.
#[cfg(target_os = "macos")]
#[test]
fn notify_setup과_test는_설치본이_없거나_세션_안이면_하지_않는다() {
    let bare = Cli::new();
    for mode in ["setup", "test"] {
        let done = bare.run(&["notify", mode]);
        assert_eq!(done.status, 1, "{mode}");
        assert!(done.out.contains("설치본이 없습니다"), "{}", done.out);
    }
    let fakes = fakes("notDetermined", true);
    for mode in ["setup", "test"] {
        let done = fakes.cli.run_with(&[("KAIROS_SESSION", "1")], &["notify", mode]);
        assert_eq!(done.status, 1, "{mode}");
    }
    assert!(fakes.calls().is_empty(), "{:?}", fakes.calls());
}
