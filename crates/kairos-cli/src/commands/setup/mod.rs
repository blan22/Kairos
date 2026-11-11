//! 처음 설정. 조직 프로필, 켤 소스, 본인 식별자, 설치, 알림 권한과 Orca 패널을 한 번에 묻는다.
//!
//! 값은 `config init`, `sources`, `connect`, `install`과 같은 길로 쓴다. 이 명령만의 저장 방식은 없다.
//! 다시 실행해도 된다. Enter는 지금 값을 그대로 둔다. 설치는 묻고 `y`일 때만 한다(기본은 하지 않음).
//!
//! `--yes`는 묻지 않는다. 소스와 계정은 지금 값(또는 `--google`, `--slack`, `--name`)을 쓰고,
//! 설치, 알림 권한, Orca 패널 승인은 예로 답한다. 조직 프로필은 `--org`가 없으면 설치 자료에 하나뿐일 때만 들인다.
//! macOS 알림 권한 창의 허용은 사람이 누른다. `--no-install`은 설치를 묻지 않고 건너뛴다(설정만 바꿀 때).

mod surfaces;

use std::fs;
use std::path::{Path, PathBuf};

use kairos_config::{Connections, IdentityField, ORG_FILE};
use kairos_core::KairosResult;
use kairos_core::catalog::{self, CATALOG, Need, SourceSpec};
use kairos_core::profile::Profile;
use kairos_ops::{InstallMode, InstallOptions, Installer};
use sha2::{Digest, Sha256};

use super::{config, connect, sources};
use crate::app::{AppContext, Console};
use crate::args::{ArgParser, OptionSpec, flag, text};
use crate::command::Command;

const USAGE: &str = "사용법: kairos setup [--org <파일> [--sha256 <값>]] [--yes] [--no-install] [--google <주소>] [--slack <멤버 ID>] [--name <이름>]";
const OPTIONS: [OptionSpec; 7] =
    [text("org"), text("sha256"), flag("yes"), flag("no-install"), text("google"), text("slack"), text("name")];
/// 값 하나를 다시 묻는 횟수.
const ATTEMPTS: usize = 3;

pub struct Setup;

impl Command for Setup {
    fn name(&self) -> &'static str {
        "setup"
    }

    fn usage(&self) -> Option<&'static str> {
        Some(USAGE)
    }

    fn execute(&self, context: &AppContext, args: &[String]) -> KairosResult<i32> {
        let parsed = ArgParser::new(&OPTIONS).parse(args)?;
        let console = context.console();
        if sources::session_caller(context) {
            console.print("세션 안에서는 설정을 바꿀 수 없습니다. 일반 터미널에서 실행하세요");
            return Ok(1);
        }
        context.home().ensure()?;
        let root = context.home().root();
        let given = Given {
            google: parsed.text("google").map(str::to_owned),
            slack: parsed.text("slack").map(str::to_owned),
            name: parsed.text("name").map(str::to_owned),
        };
        let wizard = Wizard { context, console, root, yes: parsed.flag("yes"), given };
        console.print(&format!("kairos 설정을 시작합니다. 데이터 폴더: {}", root.to_string_lossy()));
        if wizard.yes {
            console.print("--yes: 묻지 않고 지금 값과 예로 진행합니다. 알림 권한 창이 뜨면 허용만 누르세요.");
        } else {
            console.print("Enter는 [ ] 안의 값을 고릅니다. 언제든 Ctrl+C로 멈추고 다시 실행할 수 있습니다.");
        }
        wizard.organization(parsed.text("org"), parsed.text("sha256"))?;
        let Some(profile) = wizard.profile() else {
            return Ok(1);
        };
        wizard.choose_sources(&profile)?;
        let Some(profile) = wizard.profile() else {
            return Ok(1);
        };
        wizard.identities(&profile)?;
        let Some(profile) = wizard.profile() else {
            return Ok(1);
        };
        wizard.summary(&profile);
        let code = if parsed.flag("no-install") { 0 } else { wizard.install()? };
        if code == 0 {
            wizard.surfaces()?;
        }
        Ok(code)
    }
}

struct Wizard<'a> {
    context: &'a AppContext,
    console: &'a Console,
    root: &'a Path,
    /// `--yes`. 묻지 않는다.
    yes: bool,
    given: Given,
}

/// 옵션으로 받은 본인 식별자. 있으면 묻지 않고 쓴다. 모양은 저장할 때 검사한다.
struct Given {
    google: Option<String>,
    slack: Option<String>,
    name: Option<String>,
}

impl Wizard<'_> {
    fn step(&self, title: &str) {
        self.console.print("");
        self.console.print(&format!("== {title}"));
    }

    /// 한 줄을 묻는다. 입력이 끝났거나 `--yes`면 None이다.
    fn ask(&self, question: &str) -> KairosResult<Option<String>> {
        if self.yes {
            return Ok(None);
        }
        self.console.prompt(question);
        Ok(self.console.read_line()?.map(|line| line.trim().to_owned()))
    }

    /// 예, 아니오. Enter와 입력 끝은 기본값이다. `--yes`도 기본값이다(지금 값을 지킨다).
    fn confirm(&self, question: &str, default: bool) -> KairosResult<bool> {
        if self.yes {
            self.console.print(&format!("{question} {}", if default { "예" } else { "아니오" }));
            return Ok(default);
        }
        let choices = if default { "[Y/n]" } else { "[y/N]" };
        for _ in 0..ATTEMPTS {
            let Some(answer) = self.ask(&format!("{question} {choices}"))? else {
                return Ok(default);
            };
            match answer.to_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" | "예" | "네" | "ㅇ" => return Ok(true),
                "n" | "no" | "아니오" | "아니요" => return Ok(false),
                _ => self.console.print("y 또는 n으로 답하세요"),
            }
        }
        Ok(default)
    }

    /// 진행할지 묻는다. `--yes`면 예다. 아니면 `confirm`과 같다.
    fn agree(&self, question: &str, default: bool) -> KairosResult<bool> {
        if self.yes {
            self.console.print(&format!("{question} 예(--yes)"));
            return Ok(true);
        }
        self.confirm(question, default)
    }

    /// 세 파일을 다시 읽는다. 깨졌으면 알리고 None이다.
    fn profile(&self) -> Option<Profile> {
        match sources::reload(self.root) {
            Ok(profile) => Some(profile),
            Err(error) => {
                self.console.print(&format!("설정 오류: {error}. kairos config validate로 확인한 뒤 다시 실행하세요"));
                None
            }
        }
    }

    /// 첫 단계. 조직 프로필과 사용자 설정 틀. 조직 프로필은 신뢰 도메인을 정한다.
    /// `--sha256`이 없으면 지문을 보여 주고 `y`일 때만 들인다. 들일 때는 본 지문으로 고정해 그사이 바뀐 파일을 받지 않는다.
    fn organization(&self, given: Option<&str>, sha256: Option<&str>) -> KairosResult<()> {
        self.step("1/5 조직 프로필");
        if self.root.join(ORG_FILE).exists() {
            self.console.print("org.toml이 있습니다. 바꾸려면 kairos config init --org <파일> --force");
        } else {
            let chosen = match given {
                Some(path) => Some(PathBuf::from(path)),
                None if self.yes => self.only_org(),
                None => self.pick_org()?,
            };
            let pinned = match (&chosen, sha256) {
                (Some(_), Some(expected)) => Some(expected.to_owned()),
                (Some(path), None) => self.confirm_org(path)?,
                (None, _) => None,
            };
            match (chosen, pinned) {
                (Some(path), Some(digest)) => {
                    match config::import_org(self.root, &path.to_string_lossy(), Some(&digest), false)? {
                        Ok(digest) => self.console.print(&format!("조직 프로필을 들였습니다. SHA256: {digest}")),
                        Err((_, message)) => self.console.print(&format!("들이지 않음: {message}")),
                    }
                }
                _ => self.console.print("조직 프로필 없이 갑니다. GitHub 소스와 배포, 연차 캘린더는 돌지 않습니다"),
            }
        }
        if config::ensure_user_template(self.root)? {
            self.console.print("사용자 설정 틀(config.toml)을 만들었습니다");
        }
        Ok(())
    }

    /// 지문을 보여 주고 묻는다. 기본은 들이지 않음이다. 들이면 본 지문이다.
    fn confirm_org(&self, path: &Path) -> KairosResult<Option<String>> {
        let Ok(content) = fs::read(path) else {
            self.console.print("들이지 않음: 조직 프로필을 읽을 수 없습니다");
            return Ok(None);
        };
        let digest: String = Sha256::digest(&content).iter().map(|byte| format!("{byte:02x}")).collect();
        self.console.print(&format!("조직 프로필: {}", path.to_string_lossy()));
        self.console.print(&format!("SHA256: {digest}"));
        self.console.print("이 파일이 신뢰 도메인과 조직을 정합니다. 관리자가 알려 준 지문과 같은지 확인하세요");
        if self.yes {
            self.console.print("--yes라 묻지 않고 들입니다. 지문을 고정하려면 --sha256을 함께 주세요");
        }
        Ok(self.agree("이 조직 프로필을 들일까요?", false)?.then_some(digest))
    }

    /// `--yes`에서 고를 조직 프로필. 설치 자료에 하나뿐일 때만 그것이다. 여럿이면 고르지 않는다.
    /// 실행 파일 쪽 설치 자료에서만 찾는다. 지금 폴더에서 찾은 후보는 묻지 않고 들이지 않는다.
    fn only_org(&self) -> Option<PathBuf> {
        let root = self.context.layout().ok()?.binary_asset_root()?;
        match Self::profiles_in(&root).as_slice() {
            [only] => Some(only.clone()),
            [] => None,
            _ => {
                self.console.print("조직 프로필이 여럿입니다. --org <파일>로 고르세요");
                None
            }
        }
    }

    /// 설치 자료의 `profiles/`에서 고른다. 번호나 파일 경로를 받는다.
    fn pick_org(&self) -> KairosResult<Option<PathBuf>> {
        let candidates = self.org_candidates();
        for (index, path) in candidates.iter().enumerate() {
            let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            self.console.print(&format!("  {}. {name}", index + 1));
        }
        let default = if candidates.is_empty() { "건너뜀" } else { "1" };
        for _ in 0..ATTEMPTS {
            let Some(answer) =
                self.ask(&format!("조직 프로필 번호나 파일 경로를 입력하세요. -는 건너뜀 [{default}]"))?
            else {
                // 입력이 끝났으면 아무것도 고르지 않는다.
                return Ok(None);
            };
            if answer == "-" || (answer.is_empty() && candidates.is_empty()) {
                return Ok(None);
            }
            if answer.is_empty() {
                return Ok(candidates.first().cloned());
            }
            if let Ok(number) = answer.parse::<usize>() {
                if let Some(path) = number.checked_sub(1).and_then(|index| candidates.get(index)) {
                    return Ok(Some(path.clone()));
                }
                self.console.print("목록에 없는 번호입니다");
                continue;
            }
            return Ok(Some(PathBuf::from(answer)));
        }
        Ok(None)
    }

    fn org_candidates(&self) -> Vec<PathBuf> {
        let Ok(layout) = self.context.layout() else {
            return Vec::new();
        };
        Self::profiles_in(&layout.asset_root())
    }

    /// 설치 자료 폴더의 `profiles/*.toml`, 이름순.
    fn profiles_in(asset_root: &Path) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(asset_root.join("profiles")) else {
            return Vec::new();
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "toml"))
            .collect();
        paths.sort();
        paths
    }

    /// 둘째 단계. 켤 소스.
    fn choose_sources(&self, profile: &Profile) -> KairosResult<()> {
        self.step("2/5 연동할 서비스");
        let pinned = kairos_config::user_disabled_sources(self.root).unwrap_or_default();
        let mut connections = self.connections()?;
        let mut changed = false;
        for spec in &CATALOG {
            let name = spec.name.as_str();
            if pinned.iter().any(|pinned| pinned == name) {
                self.console.print(&format!("{}: config.toml에서 껐습니다. 건너뜁니다", spec.name.label()));
                continue;
            }
            let question = format!("{} ({}) 켤까요?", spec.name.label(), spec.summary);
            let enabled = self.confirm(&question, !spec.is_disabled(profile))?;
            changed |= connections.set_source_enabled(name, enabled);
        }
        if changed {
            self.save(&connections);
        }
        Ok(())
    }

    /// 셋째 단계. 켠 소스가 필요로 하는 본인 식별자.
    fn identities(&self, profile: &Profile) -> KairosResult<()> {
        self.step("3/5 계정 연결");
        let enabled: Vec<&SourceSpec> = CATALOG.iter().filter(|spec| !spec.is_disabled(profile)).collect();
        let needs = catalog::needs_of(enabled.iter().copied());
        if needs.is_empty() {
            self.console.print("켠 소스가 없어 연결할 계정이 없습니다");
        }
        for need in needs {
            let users: Vec<&str> = enabled
                .iter()
                .filter(|spec| spec.requires.contains(&need) || spec.optional.contains(&need))
                .map(|spec| spec.name.label())
                .collect();
            let optional = !enabled.iter().any(|spec| spec.requires.contains(&need));
            let purpose = format!("{}{}", users.join(", "), if optional { ", 선택" } else { "" });
            match need {
                Need::GithubOrg => {
                    if !need.met(profile) {
                        self.console.print(&format!(
                            "{}({purpose}): 조직 프로필에 없습니다. {}",
                            need.label(),
                            need.hint()
                        ));
                    }
                }
                Need::GithubLogin => self.github(profile)?,
                Need::GoogleEmail | Need::SlackUser | Need::DisplayName => self.identity(profile, need, &purpose)?,
            }
        }
        Ok(())
    }

    fn github(&self, profile: &Profile) -> KairosResult<()> {
        let current = profile.identity.github_login.as_deref();
        let question = match current {
            Some(login) => format!("GitHub 로그인: {login}. gh 로그인 사용자로 다시 읽을까요?"),
            None => "GitHub 로그인: gh 로그인 사용자로 기록할까요?".to_owned(),
        };
        if !self.confirm(&question, current.is_none())? {
            return Ok(());
        }
        match connect::gh_login(self.context, &profile.org.github_orgs) {
            Ok(login) => {
                let mut connections = self.connections()?;
                connections.set_identity(IdentityField::Github, &login);
                if self.save(&connections) {
                    self.console.print(&format!("GitHub 로그인 {}을 기록했습니다", login.to_ascii_lowercase()));
                }
            }
            Err((_, message)) => self.console.print(message),
        }
        Ok(())
    }

    fn identity(&self, profile: &Profile, need: Need, purpose: &str) -> KairosResult<()> {
        let (field, current, example, given) = match need {
            Need::GoogleEmail => {
                (IdentityField::Google, &profile.identity.google_email, "name@example.com", &self.given.google)
            }
            Need::SlackUser => (
                IdentityField::Slack,
                &profile.identity.slack_user,
                "Slack 프로필 > 멤버 ID 복사, U로 시작",
                &self.given.slack,
            ),
            _ => (IdentityField::Name, &profile.identity.display_name, "연차 일정에 쓰는 이름", &self.given.name),
        };
        if let Some(value) = given.as_deref() {
            let mut connections = self.connections()?;
            connections.set_identity(field, value);
            if self.save(&connections) {
                self.console.print(&format!("{}을 기록했습니다", need.label()));
            } else {
                self.console.print(&format!("{}은 나중에: {}", need.label(), need.hint()));
            }
            return Ok(());
        }
        if self.yes && current.is_none() {
            self.console.print(&format!("{}({purpose}): 비어 있습니다. 나중에: {}", need.label(), need.hint()));
            return Ok(());
        }
        let default = current.as_deref().unwrap_or("건너뜀");
        for _ in 0..ATTEMPTS {
            let Some(answer) = self.ask(&format!("{}({purpose}, {example}) [{default}]", need.label()))? else {
                return Ok(());
            };
            if answer.is_empty() {
                return Ok(());
            }
            let mut connections = self.connections()?;
            connections.set_identity(field, &answer);
            if self.save(&connections) {
                return Ok(());
            }
        }
        self.console.print(&format!("{}은 나중에: {}", need.label(), need.hint()));
        Ok(())
    }

    fn connections(&self) -> KairosResult<Connections> {
        Connections::read(self.root).map_err(|error| kairos_core::KairosError::new(error.code))
    }

    /// 검증을 통과해야 쓴다. 실패하면 이유를 알리고 false다.
    fn save(&self, connections: &Connections) -> bool {
        match connections.save(self.root) {
            Ok(()) => true,
            Err(error) => {
                let reason =
                    if error.code == "config_invalid_value" { "형식이 맞지 않습니다" } else { error.code };
                self.console.print(&format!("저장하지 않음: {reason}"));
                false
            }
        }
    }

    /// 넷째 단계. 지금 상태와 설치.
    fn summary(&self, profile: &Profile) {
        self.step("4/5 확인과 설치");
        let pinned = kairos_config::user_disabled_sources(self.root).unwrap_or_default();
        for line in sources::lines(profile, &pinned) {
            self.console.print(&line);
        }
        self.console.print("세션을 쓰려면 읽기 전용 토큰을 넣습니다: kairos connect session-token < 토큰파일");
    }

    fn install(&self) -> KairosResult<i32> {
        let question = "설치하고 launchd에 등록해 수집을 시작할까요? (kairos install --register)";
        if !self.agree(question, false)? {
            self.console.print("설치하지 않았습니다. 나중에: kairos install --register");
            self.console.print("소스는 언제든 kairos sources on|off <소스>로 바꿉니다");
            return Ok(0);
        }
        // 설치 자료는 launchd 작업과 Orca 패널이 된다. --yes는 실행 파일 쪽 설치 자료만 쓴다. 지금 폴더는 남의 저장소일 수 있다.
        let layout = self.context.layout()?;
        let assets = if self.yes {
            let Some(root) = layout.binary_asset_root() else {
                self.console.print("실행 파일 옆에 설치 자료(ops/, plugins/)가 없어 --yes로 설치하지 않습니다");
                self.console.print("자료가 있는 폴더에서 kairos install --register를 직접 실행하세요");
                return Ok(1);
            };
            root
        } else {
            layout.asset_root()
        };
        let options =
            InstallOptions { mode: InstallMode::Register, ..InstallOptions::new(self.context.home().clone(), assets) };
        let result = Installer::new(self.context.runner()).install(&options)?;
        for line in &result.lines {
            self.console.print(line);
        }
        Ok(i32::from(result.failed))
    }
}
