//! `kairos update`. 지금 판, 아는 최신 판, 설치 방식에 맞는 업데이트 명령을 알려 준다.
//!
//! - 옵션 없이: 아무것도 바꾸지 않는다. 최신 판은 수집 틱이 하루 한 번 적어 둔 `update.json`에서 읽는다.
//! - `--check`: 지금 gh로 최신 판을 묻고 적는다. 내려받지 않는다.
//! - `--apply`: 사용자가 터미널에서 실행했을 때만 새 판을 설치한다. 세션, Orca 터미널, Claude Code 안에서는 거절한다.
//!   릴리스 설치본은 gh로 그 판의 묶음을 받아 SHA256과 빌드 증명을 확인한 뒤 묶음의 `kairos update --finish`를 띄운다.
//!   소스 설치본은 저장소의 설치 스크립트(`--update`)를 띄운다. 둘 다 터미널을 물려줘 질문에 사람이 답한다.
//! - `--finish`: 새 판 묶음의 실행 파일이 받는다. 실행 파일 옆 설치 자료로 설치하고 알림 권한과 Orca 패널만 묻는다.
//!
//! 설치 방식은 설치 스크립트가 데이터 폴더에 남긴 `install-source`에서 읽는다.

use std::fs::{self, DirBuilder};
use std::path::{Path, PathBuf};
use std::process::Command as Process;

use kairos_core::KairosResult;
use kairos_core::home::DIR_MODE;
use kairos_core::platform::{self, CreateMode, Os};
use kairos_core::text::sanitize;
use kairos_ops::{UPDATE_APPLY_COMMAND, UpdateFile, UpdateRecord, Version, system_tool};
use serde_json::json;

use super::sources;
use super::tools::{is_plain, shell_quote};
use crate::app::AppContext;
use crate::args::{ArgParser, OptionSpec, flag};
use crate::command::Command;
use crate::help::{self, Group};
use crate::reply::Reply;

const USAGE: &str = "사용법: kairos update [--check] [--apply] [--json]";
const OPTIONS: [OptionSpec; 4] = [flag("json"), flag("check"), flag("apply"), flag("finish")];
/// 설치 스크립트가 남기는 파일. 한 줄이다. `source:<저장소 절대 경로>` 또는 `release`.
const INSTALL_SOURCE_FILE: &str = "install-source";
const SOURCE_PREFIX: &str = "source:";
const RELEASE_MARK: &str = "release";
/// 저장소 경로 길이 상한. 넘으면 모르는 값으로 본다.
const MAX_SOURCE_BYTES: u64 = 4096;
/// 새 판을 받아 푸는 작업 폴더. 데이터 폴더 안이라 다른 사용자가 끼어들 수 없다. 끝나면 지운다.
const WORK_DIR: &str = "update-work";
/// 사람이 터미널에서 쳐도 안내를 붙이지 않는 명령. 스스로 판을 말하거나 다른 프로그램이 읽는 출력이다.
const NO_HINT: [&str; 4] = ["update", "completions", "watch", "mcp"];

/// OS마다 다른 설치 스크립트와 릴리스 설치 명령.
struct Installer {
    /// 저장소 안의 설치 스크립트. 경로 조각이다.
    script: [&'static str; 2],
    /// 릴리스 설치 명령. 다시 돌리면 최신 판으로 올린다.
    release: &'static str,
    /// 붙여 넣을 셸. Windows는 PowerShell이다.
    powershell: bool,
}

impl Installer {
    const fn for_os(os: Os) -> Self {
        match os {
            Os::MacOs => Self {
                script: ["scripts", "install.sh"],
                release: "curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.sh | zsh",
                powershell: false,
            },
            Os::Linux => Self {
                script: ["scripts", "install-linux.sh"],
                release: "curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release-linux.sh | sh",
                powershell: false,
            },
            Os::Windows => Self {
                script: ["scripts", "install.ps1"],
                release: "irm https://raw.githubusercontent.com/blan22/Kairos/main/scripts/install-release.ps1 | iex",
                powershell: true,
            },
        }
    }

    /// 저장소 밖에서 쓰는 스크립트 이름(scripts/install.sh 꼴).
    fn relative(&self) -> String {
        self.script.join("/")
    }

    fn script_in(&self, repo: &str) -> PathBuf {
        self.script.iter().fold(Path::new(repo).to_path_buf(), |path, part| path.join(part))
    }

    /// 저장소의 설치 스크립트로 올리는 명령.
    fn update_command(&self, script: &Path) -> String {
        let script = script.to_string_lossy();
        if self.powershell {
            // 실행 정책이 막아도 돌도록 -File로 넘긴다.
            format!("powershell -NoProfile -ExecutionPolicy Bypass -File {} --update", powershell_quote(&script))
        } else {
            format!("{} --update", shell_quote(&script))
        }
    }

    /// `update_command`와 같은 일을 셸 없이 띄우는 프로그램과 인자.
    fn update_process(&self, script: &Path) -> (PathBuf, Vec<String>) {
        let script = script.to_string_lossy().into_owned();
        if self.powershell {
            let args = ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", &script, "--update"];
            (PathBuf::from(system_tool(r"WindowsPowerShell\v1.0\powershell.exe")), args.map(str::to_owned).to_vec())
        } else {
            (PathBuf::from(script), vec!["--update".to_owned()])
        }
    }
}

/// 설치 방식.
#[derive(Clone, Debug, Eq, PartialEq)]
enum Origin {
    /// 저장소에서 빌드했다. 값은 저장소 경로다.
    Source(String),
    Release,
    /// 기록이 없거나 읽을 수 없다.
    Unknown,
}

impl Origin {
    fn read(home: &Path, os: Os) -> Self {
        let path = home.join(INSTALL_SOURCE_FILE);
        let small = fs::metadata(&path).is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_SOURCE_BYTES);
        let Some(text) = small.then(|| fs::read_to_string(&path).ok()).flatten() else {
            return Self::Unknown;
        };
        Self::parse(&text, os)
    }

    fn parse(text: &str, os: Os) -> Self {
        let line = text.lines().next().unwrap_or("").trim();
        if line == RELEASE_MARK {
            return Self::Release;
        }
        match line.strip_prefix(SOURCE_PREFIX) {
            // 절대 경로만 받는다. 그대로 찍을 수 없는 값(제어 문자, bidi 문자)은 모르는 값으로 본다. 붙여 넣을 명령이 된다.
            Some(repo) if platform::is_absolute(os, repo) && is_plain(repo) => Self::Source(repo.to_owned()),
            _ => Self::Unknown,
        }
    }

    const fn method(&self) -> &'static str {
        match self {
            Self::Source(_) => "source",
            Self::Release => "release",
            Self::Unknown => "unknown",
        }
    }
}

/// 붙여 넣을 명령에 넣어도 되는 경로인가. 글자, 숫자, 공백, 경로 구분자와 `:._-+`만 받는다.
fn is_safe_path(path: &str) -> bool {
    path.chars().all(|letter| letter.is_alphanumeric() || " /\\:._-+".contains(letter))
}

/// PowerShell에 그대로 붙여 넣을 수 있게 작은따옴표로 감싼다. 작은따옴표 안에서는 `$`와 백틱을 해석하지 않는다.
fn powershell_quote(text: &str) -> String {
    // PowerShell은 유니코드 따옴표(‘ ’ ‚ ‛)도 작은따옴표로 읽는다. 모두 두 번 써서 닫히지 않게 한다.
    let mut quoted = String::from("'");
    for letter in text.chars() {
        if matches!(letter, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            quoted.push(letter);
        }
        quoted.push(letter);
    }
    quoted.push('\'');
    quoted
}

/// 받기 단계의 실패 코드를 사람이 읽을 글로 바꾼다.
fn fetch_message(code: &str) -> String {
    match code {
        "update_unsupported_platform" => "이 OS와 CPU에 맞는 릴리스 묶음이 없습니다. 저장소에서 빌드하세요".to_owned(),
        "update_download_failed" => "묶음을 받지 못했습니다. gh auth status로 로그인을 확인하세요".to_owned(),
        "update_checksum_missing" => "SHA256 파일이 없거나 모양이 틀립니다. 설치하지 않습니다".to_owned(),
        "update_checksum_mismatch" => "SHA256이 다릅니다. 받은 파일을 쓰지 않습니다".to_owned(),
        "update_attestation_failed" => {
            "빌드 증명을 확인하지 못했습니다. 이 저장소의 릴리스 워크플로가 만든 묶음이 아니거나 증명이 없습니다. 설치하지 않습니다"
                .to_owned()
        }
        "update_extract_failed" => "묶음을 풀지 못했습니다".to_owned(),
        "update_bundle_invalid" => "묶음의 모양이 맞지 않습니다. 설치하지 않습니다".to_owned(),
        other => format!("받는 중에 멈췄습니다({other})"),
    }
}

/// 터미널을 물려 띄우고 끝날 때까지 기다린다. 셸을 거치지 않는다. 띄우지 못했으면 None이다.
fn run_attached(program: &Path, args: &[String]) -> Option<i32> {
    let status = Process::new(program).args(args).status().ok()?;
    Some(status.code().unwrap_or(1))
}

/// 사람이 터미널에서 쓴 명령 뒤에 붙이는 한 줄. `--json`, 내부 명령, 세션과 에이전트 안, 확인을 끈 설정에는 붙이지 않는다.
/// 호출한 쪽이 stderr가 터미널일 때만 부른다. 호스트는 파이프로 부르므로 받지 않는다.
pub fn hint(context: &AppContext, args: &[String]) -> Option<String> {
    let name = args.first()?;
    if args.iter().any(|arg| arg == "--json") || NO_HINT.contains(&name.as_str()) {
        return None;
    }
    if help::find(name).is_none_or(|entry| entry.group == Group::Internal) || sources::session_caller(context) {
        return None;
    }
    let notice = context.update_notice()?;
    Some(format!(
        "kairos 새 판 {}이 나왔습니다(지금 {}). {UPDATE_APPLY_COMMAND}로 올립니다",
        notice.latest, notice.current
    ))
}

pub struct Update;

impl Command for Update {
    fn name(&self) -> &'static str {
        "update"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(USAGE)
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&OPTIONS).parse(args)?;
        let reply = Reply::new(context, parsed.flag("json"));
        let os = Os::current();
        let installer = Installer::for_os(os);
        let origin = Origin::read(context.home().root(), os);
        if parsed.flag("finish") || parsed.flag("apply") {
            // 사람이 답하는 설치다. JSON으로 낼 것이 없다.
            if reply.json() || (parsed.flag("finish") && parsed.flag("apply")) {
                return Ok(reply.usage(USAGE));
            }
            if sources::session_caller(context) {
                let console = context.console();
                console.print("세션, Orca 터미널, Claude Code 안에서는 올릴 수 없습니다");
                console.print(&format!("일반 터미널 앱에서 {UPDATE_APPLY_COMMAND}를 실행하세요"));
                return Ok(1);
            }
            if parsed.flag("finish") {
                return super::setup::finish_update(context);
            }
            return Ok(apply(context, &installer, &origin));
        }
        let file = UpdateFile::new(context.home());
        let record = if parsed.flag("check") { check(context, &file)? } else { file.read() };
        show(context, &reply, &installer, &origin, &record)
    }
}

/// 지금 최신 판을 묻고 적는다. gh가 없으면 묻지 않고 적힌 기록을 돌려준다.
fn check(context: &AppContext, file: &UpdateFile) -> KairosResult<UpdateRecord> {
    let Some(probe) = context.release_probe() else {
        return Ok(file.read());
    };
    file.check_now(context.now(), probe.as_ref())
}

/// 알려 줄 명령. 셸이 해석할 글자가 든 경로는 명령으로 찍지 않는다. 붙여 넣는 셸(cmd, fish 등)마다 인용 규칙이 다르다.
fn manual_command(installer: &Installer, origin: &Origin) -> Option<String> {
    match origin {
        Origin::Source(repo) if !is_safe_path(repo) => None,
        Origin::Source(repo) => {
            let script = installer.script_in(repo);
            script.is_file().then(|| installer.update_command(&script))
        }
        Origin::Release => Some(installer.release.to_owned()),
        Origin::Unknown => None,
    }
}

fn show(
    context: &AppContext,
    reply: &Reply<'_>,
    installer: &Installer,
    origin: &Origin,
    record: &UpdateRecord,
) -> KairosResult<i32> {
    let current = Version::current();
    let available = record.available(current);
    let command = manual_command(installer, origin);
    if reply.json() {
        let mut update = json!({ "version": env!("CARGO_PKG_VERSION"), "method": origin.method() });
        // 없는 값은 키째 뺀다. 사람이 터미널에 붙여 넣을 셸 문자열이다. 호스트가 실행하지 않는다.
        if let Some(command) = &command {
            update["command"] = json!(command);
        }
        if let Some(latest) = record.latest {
            update["latest"] = json!(latest.to_string());
            update["available"] = json!(available.is_some());
        }
        if let Some(at) = record.checked_at {
            update["checkedAt"] = json!(at.to_iso());
        }
        return reply.success(json!({ "update": update }));
    }
    let console = context.console();
    console.print(&format!("지금 판: {}", help::version_line()));
    let checked = |label: &str| match record.checked_at {
        Some(at) => {
            let minutes = at.local_minutes();
            format!("최신 판: {label} ({} {:02}:{:02} 확인)", at.local_date(), minutes / 60, minutes % 60)
        }
        None => format!("최신 판: {label}"),
    };
    match (record.latest, record.checked_at) {
        (Some(latest), _) if record.ok => console.print(&checked(&latest.to_string())),
        (Some(latest), _) => console.print(&checked(&format!("{latest}, 마지막 확인은 실패"))),
        (None, Some(_)) => console.print(&checked("확인하지 못함(gh auth status로 로그인을 확인하세요)")),
        (None, None) => console.print("최신 판: 아직 확인하지 않음(kairos update --check)"),
    }
    let label = match origin {
        Origin::Source(repo) => format!("소스 빌드({})", sanitize(repo)),
        Origin::Release => "릴리스".to_owned(),
        Origin::Unknown => "기록 없음".to_owned(),
    };
    console.print(&format!("설치 방식: {label}"));
    if let Some(latest) = available.filter(|_| *origin != Origin::Unknown) {
        console.print(&format!("새 판 {latest}이 있습니다. 다음 명령으로 올립니다. 설정과 데이터는 그대로 둡니다"));
        console.print(&format!("  {UPDATE_APPLY_COMMAND}"));
    }
    match (origin, command) {
        (_, Some(command)) => {
            let shell = if installer.powershell { "PowerShell" } else { "터미널" };
            console.print(&format!("직접 올리려면 {shell}에서 다음을 실행하세요. 설정과 데이터는 그대로 둡니다"));
            console.print(&format!("  {command}"));
        }
        (Origin::Source(repo), None) if !is_safe_path(repo) => {
            console.print(&format!(
                "저장소 경로에 셸이 해석하는 글자가 있어 명령을 찍지 않습니다. 저장소 폴더에서 {} --update를 실행하세요",
                installer.relative()
            ));
        }
        (Origin::Source(_), None) => {
            console.print(&format!(
                "저장소가 그 자리에 없습니다. 저장소를 다시 받아 {}를 실행하세요",
                installer.relative()
            ));
        }
        _ => {
            console.print("설치 방식을 적어 두기 전의 판으로 설치했습니다. 받은 저장소에서 다음을 실행하세요");
            console.print(&format!("  {} --update", installer.relative()));
        }
    }
    Ok(0)
}

/// `--apply`. 종료 코드를 돌려준다.
fn apply(context: &AppContext, installer: &Installer, origin: &Origin) -> i32 {
    let console = context.console();
    match origin {
        Origin::Release => apply_release(context),
        Origin::Source(repo) if is_safe_path(repo) && installer.script_in(repo).is_file() => {
            let script = installer.script_in(repo);
            console.print("저장소를 당겨 다시 빌드하고 설치합니다:");
            console.print(&format!("  {}", installer.update_command(&script)));
            let (program, args) = installer.update_process(&script);
            run_attached(&program, &args).unwrap_or_else(|| {
                console.print("설치 스크립트를 띄우지 못했습니다. 위 명령을 직접 실행하세요");
                1
            })
        }
        Origin::Source(_) => {
            console.print(&format!(
                "저장소의 설치 스크립트를 찾지 못했습니다. 저장소 폴더에서 {} --update를 실행하세요",
                installer.relative()
            ));
            1
        }
        Origin::Unknown => {
            console.print("설치 방식 기록이 없어 올릴 수 없습니다. 받은 저장소에서 다음을 한 번 실행하세요");
            console.print(&format!("  {} --update", installer.relative()));
            console.print(&format!("릴리스 묶음으로 설치했다면: {}", installer.release));
            1
        }
    }
}

/// 릴리스 설치본을 올린다. 확인, 받기, 증명, 풀기, 새 판의 `update --finish` 순서다.
fn apply_release(context: &AppContext) -> i32 {
    let console = context.console();
    let Some(probe) = context.release_probe() else {
        console.print("gh가 없어 새 판을 받을 수 없습니다. gh를 설치하고 gh auth login을 한 뒤 다시 실행하세요");
        return 1;
    };
    console.print("== 최신 판 확인");
    let current = Version::current();
    let record = match UpdateFile::new(context.home()).check_now(context.now(), probe.as_ref()) {
        Ok(record) if record.ok => record,
        _ => {
            console.print("최신 판을 확인하지 못했습니다. gh auth status로 로그인을 확인하세요");
            return 1;
        }
    };
    let Some(latest) = record.available(current) else {
        console.print(&format!("이미 최신 판입니다(kairos {current})"));
        return 0;
    };
    console.print(&format!("  kairos {current} → {latest}"));
    console.print("");
    console.print("== 받기와 확인(SHA256, 빌드 증명)");
    let work = context.home().file(WORK_DIR);
    let fetched = prepare_work(&work).and_then(|()| context.bundle_fetcher().fetch(latest, &work));
    let root = match fetched {
        Ok(root) => root,
        Err(error) => {
            console.print(&fetch_message(error.code()));
            let _ = fs::remove_dir_all(&work);
            return 1;
        }
    };
    console.print("  이 저장소의 릴리스 워크플로가 만든 묶음임을 확인했습니다");
    console.print("");
    let exe = root.join("bin").join(if Os::current() == Os::Windows { "kairos.exe" } else { "kairos" });
    // 새 판이 설치와 질문을 맡는다. 받은 묶음은 그 판이 설치본으로 복사한 뒤에 지운다.
    let code = run_attached(&exe, &["update".to_owned(), "--finish".to_owned()]).unwrap_or_else(|| {
        console.print("새 판의 실행 파일을 띄우지 못했습니다");
        1
    });
    let _ = fs::remove_dir_all(&work);
    code
}

/// 앞의 실행이 남긴 작업 폴더를 지우고 비어 있는 폴더를 만든다. 링크면 따라가지 않고 링크만 지운다.
fn prepare_work(work: &Path) -> KairosResult<()> {
    match fs::symlink_metadata(work) {
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(work)?,
        Ok(_) => fs::remove_file(work)?,
        Err(_) => {}
    }
    DirBuilder::new().create_mode(DIR_MODE).create(work)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 설치_방식은_한_줄에서_읽고_모양이_틀리면_모른다고_본다() {
        for os in [Os::MacOs, Os::Linux] {
            assert_eq!(Origin::parse("release\n", os), Origin::Release);
            assert_eq!(Origin::parse("source:/synthetic/repo\n", os), Origin::Source("/synthetic/repo".to_owned()));
            assert_eq!(Origin::parse("source:/with space/repo", os), Origin::Source("/with space/repo".to_owned()));
            for bad in ["", "source:relative/repo", "source:/a\u{1b}[31m", "git:/x", "release extra", "source:C:\\repo"]
            {
                assert_eq!(Origin::parse(bad, os), Origin::Unknown, "{bad:?}");
            }
        }
        let windows = Origin::parse("source:C:\\Users\\synth\\kairos\r\n", Os::Windows);
        assert_eq!(windows, Origin::Source("C:\\Users\\synth\\kairos".to_owned()));
        for bad in ["source:/synthetic/repo", "source:repo", "source:C:repo"] {
            assert_eq!(Origin::parse(bad, Os::Windows), Origin::Unknown, "{bad:?}");
        }
    }

    #[test]
    fn 셸이_해석하는_글자가_든_경로는_명령에_넣지_않는다() {
        for good in ["/synthetic/my repo", "C:\\Users\\synth\\kairos", "/합성/저장소-1.0+x"] {
            assert!(is_safe_path(good), "{good}");
        }
        for bad in
            ["/a'b", "/a&b", "/a|b", "/a^b", "/%PATH%", "/a$b", "/a`b", "/a;b", "/a(b)", "/a<b>", "/a!b", "/a\u{2019}b"]
        {
            assert!(!is_safe_path(bad), "{bad}");
        }
    }

    #[test]
    fn 업데이트_명령은_os의_설치_스크립트와_셸을_따른다() {
        let mac = Installer::for_os(Os::MacOs);
        assert_eq!(
            mac.update_command(Path::new("/synthetic/repo/scripts/install.sh")),
            "/synthetic/repo/scripts/install.sh --update"
        );
        let (program, args) = mac.update_process(Path::new("/synthetic/my repo/scripts/install.sh"));
        assert_eq!(
            (program, args),
            (PathBuf::from("/synthetic/my repo/scripts/install.sh"), vec!["--update".to_owned()])
        );
        assert!(mac.release.ends_with("scripts/install-release.sh | zsh"));
        let linux = Installer::for_os(Os::Linux);
        assert_eq!(linux.relative(), "scripts/install-linux.sh");
        assert!(linux.release.ends_with("scripts/install-release-linux.sh | sh"));
        let windows = Installer::for_os(Os::Windows);
        assert_eq!(
            windows.update_command(Path::new("C:\\it's\\$x\\install.ps1")),
            "powershell -NoProfile -ExecutionPolicy Bypass -File 'C:\\it''s\\$x\\install.ps1' --update"
        );
        let (program, args) = windows.update_process(Path::new("C:\\it's\\install.ps1"));
        assert!(program.to_string_lossy().ends_with(r"WindowsPowerShell\v1.0\powershell.exe"), "{program:?}");
        assert_eq!(args, ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "C:\\it's\\install.ps1", "--update"]);
        assert_eq!(powershell_quote("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
        assert!(windows.release.starts_with("irm https://") && windows.release.ends_with("install-release.ps1 | iex"));
    }

    #[test]
    fn 받기_실패는_코드마다_사람이_읽을_글로_바꾼다() {
        for code in [
            "update_unsupported_platform",
            "update_download_failed",
            "update_checksum_missing",
            "update_checksum_mismatch",
            "update_attestation_failed",
            "update_extract_failed",
            "update_bundle_invalid",
        ] {
            assert!(!fetch_message(code).contains("update_"), "{code}");
        }
        assert!(fetch_message("EIO").contains("EIO"));
    }
}
