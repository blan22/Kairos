//! setup의 설치 뒤 단계(알림 권한, Orca 패널). 실제 Orca, 알림 헬퍼, lsregister는 부르지 않는다.
#![allow(clippy::unwrap_used)]

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;

const FINGERPRINT: &str = "ap1-0123456789abcdef0123456789abcdef";

struct Fakes {
    cli: Cli,
    log: PathBuf,
    panel: PathBuf,
}

impl Fakes {
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(&self.log).unwrap_or_default().lines().map(str::to_owned).collect()
    }
}

/// 데이터 폴더에 설치본 모양(`current -> 0.2.0`)과 가짜 헬퍼, lsregister, orca를 둔다. 호출은 한 파일에 남는다.
/// `status`는 헬퍼의 권한 상태, `orca_approved`는 승인 없이 올라가는지다.
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
    let notifier = macos.join("KairosNotifier");
    write_script(
        &notifier,
        &format!(
            "#!/bin/sh\nprintf 'notifier %s\\n' \"$*\" >> '{log_text}'\ncase \"$1\" in\nstatus) printf '{status}\\n' ;;\nesac\nexit 0\n"
        ),
    );
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
            "#!/bin/sh\nprintf 'orca %s\\n' \"$*\" >> '{log_text}'\ncase \"$*\" in\n*--approve*) printf 'approved\\n'; exit 0 ;;\nesac\n{pending}\n"
        ),
    );
    cli.set("KAIROS_NOTIFIER", notifier.to_str().unwrap());
    cli.set("KAIROS_LSREGISTER", lsregister.to_str().unwrap());
    cli.set("KAIROS_ORCA", orca.to_str().unwrap());
    let panel = fs::canonicalize(panel).unwrap();
    Fakes { cli, log, panel }
}

/// setup의 앞 네 단계는 Enter로 지금 값을 지키고 설치는 하지 않는다. 그 뒤에 `tail`을 넘긴다.
fn answers(tail: &str) -> Vec<u8> {
    // 소스 다섯, GitHub 다시 읽기, Google 주소, Slack ID, 이름, 설치.
    format!("{}{tail}", "\n".repeat(10)).into_bytes()
}

#[test]
fn setup은_알림_권한을_받고_패널을_승인한다() {
    let fakes = fakes("denied", false);
    // 알림 예(Enter), 패널 예(Enter), 승인 y.
    let done = fakes.cli.pipe(&["setup"], &answers("\n\ny\n"));
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("5/5 알림과 Orca 패널"), "{}", done.out);
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
        ],
        "{}",
        done.out
    );
}

#[test]
fn setup은_묻지_않으면_패널을_승인하지_않는다() {
    let fakes = fakes("denied", false);
    // 입력이 끝나면 기본값이다. 알림과 적재는 예, 승인은 아니오다.
    let done = fakes.cli.pipe(&["setup"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("승인하지 않았습니다"), "{}", done.out);
    assert!(fakes.calls().iter().all(|call| !call.contains("--approve")), "{:?}", fakes.calls());
}

#[test]
fn setup은_이미_받은_권한과_승인을_다시_묻지_않는다() {
    let fakes = fakes("authorized", true);
    let done = fakes.cli.pipe(&["setup"], b"");
    assert_eq!(done.status, 0, "{}{}", done.out, done.err);
    assert!(done.out.contains("권한이 이미 있습니다"), "{}", done.out);
    assert!(done.out.contains("Orca 패널을 올렸습니다"), "{}", done.out);
    let calls = fakes.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0], "notifier status");
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

#[test]
fn setup_yes는_묻지_않고_계정을_기록하고_패널까지_승인한다() {
    let fakes = fakes("denied", false);
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
    assert_eq!(calls.last().unwrap(), &format!("orca plugin dev load {panel} --approve {FINGERPRINT}"));
    let connections = fs::read_to_string(fakes.cli.root.join("connections.toml")).unwrap();
    assert!(connections.contains("someone@example.com"), "{connections}");
    assert!(connections.contains("U0TEST1234"), "{connections}");
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
