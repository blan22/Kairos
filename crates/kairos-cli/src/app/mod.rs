//! 조립 지점. 시스템 구현은 여기서만 고른다. 명령은 이 객체가 쥔 포트만 쓴다.

mod console;
mod layout;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kairos_agenda::{
    ControlFile, KairosControl, NOTIFIER_APP_REL, NativeNotifier, NextContext, ORCA_BIN, OrcaPanelOpener, SourceHealth,
    SourceHealthBook, StateBuilder,
};
use kairos_core::model::Source;
use kairos_core::ports::{Clock, Notifier, PanelOpener, SystemClock};
use kairos_core::process::{ProcessRunner, SystemProcessRunner};
use kairos_core::{Instant, KairosError, KairosHome, KairosResult, home, js};
use kairos_ledger::OffDateBook;
use kairos_ops::{ResumeFile, find_binary, find_claude};
use kairos_sessions::OrcaClient;
use kairos_source_github::GithubConfig;
use kairos_store::Database;
use kairos_tick::{Doctor, DoctorConfig, SleepProbe, SourceRegistry, TickConfig, TickRunner};

pub use console::Console;
pub use layout::InstallLayout;

/// 환경에서 실행 파일을 고른다. `값.trim() || findBinary(이름)`이다. 빈 값은 없는 값이다.
fn configured_binary(env: &BTreeMap<String, String>, key: &str, name: &str) -> Option<String> {
    given(env, key).map(str::to_owned).or_else(|| find_binary(name, env.get("PATH").map_or("", String::as_str)))
}

/// 앞으로 끌어올 때 쓰는 open. 시험은 `KAIROS_OPEN`으로 가짜를 준다.
const OPEN_BIN: &str = "/usr/bin/open";

/// 환경 값이 비어 있지 않으면 그 값이다.
fn given<'a>(env: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    env.get(key).map(|value| js::trim(value)).filter(|value| !value.is_empty())
}

/// 알림 헬퍼의 실행 파일. `KAIROS_NOTIFIER`가 있으면 그 값, 없으면 설치 자료 폴더 안의 번들이다. 파일이 없으면 None이다.
fn notifier_bin(env: &BTreeMap<String, String>, layout: Option<&InstallLayout>) -> Option<String> {
    let bin = match given(env, "KAIROS_NOTIFIER") {
        Some(bin) => PathBuf::from(bin),
        None => layout?.asset_root().join(NOTIFIER_APP_REL),
    };
    Path::is_file(&bin).then(|| bin.to_string_lossy().into_owned())
}

/// `doctor`가 권한 상태를 묻는 헬퍼. `notifier_bin`과 달리 지금 폴더의 조상은 보지 않는다.
/// 진단 명령이 남의 저장소에 놓인 실행 파일을 띄우지 않게 한다.
fn doctor_notifier_bin(env: &BTreeMap<String, String>, layout: Option<&InstallLayout>) -> Option<String> {
    let bin = match given(env, "KAIROS_NOTIFIER") {
        Some(bin) => PathBuf::from(bin),
        None => layout?.binary_asset_root()?.join(NOTIFIER_APP_REL),
    };
    Path::is_file(&bin).then(|| bin.to_string_lossy().into_owned())
}

/// 프로세스 환경. 자식 환경이 문자열이라 UTF-8이 아닌 값은 넘어가지 않는다.
fn process_env() -> BTreeMap<String, String> {
    std::env::vars_os().filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect()
}

/// 명령이 쓰는 값과 포트의 모음.
pub struct AppContext {
    home: KairosHome,
    user_home: PathBuf,
    clock: Arc<dyn Clock>,
    runner: Arc<dyn ProcessRunner>,
    env: BTreeMap<String, String>,
    sources: SourceRegistry,
    notifier: Option<Arc<dyn Notifier>>,
    /// 알림 헬퍼의 실행 파일. `doctor`가 권한 상태를 읽는다.
    notifier_bin: Option<String>,
    panel: Option<Arc<dyn PanelOpener>>,
    sleep: Option<Arc<dyn SleepProbe>>,
    layout: Option<InstallLayout>,
    console: Console,
}

impl AppContext {
    /// 시험이 쓰는 조립. 알림과 패널이 없고 표준 입출력에도 붙지 않는다. 소스는 운영의 다섯이다.
    pub fn new(
        home: KairosHome,
        clock: Arc<dyn Clock>,
        runner: Arc<dyn ProcessRunner>,
        env: BTreeMap<String, String>,
    ) -> Self {
        let github = GithubConfig::from_vars(|key| env.get(key).cloned());
        let sources = SourceRegistry::standard(clock.clone(), runner.clone(), github);
        let user_home = env.get("HOME").map_or_else(home::user_home, PathBuf::from);
        Self {
            home,
            user_home,
            clock,
            runner,
            env,
            sources,
            notifier: None,
            notifier_bin: None,
            panel: None,
            sleep: None,
            layout: None,
            console: Console::captured(""),
        }
    }

    /// 운영의 조립. 실제 알림 헬퍼와 Orca 패널은 여기서만 고른다.
    /// 알림은 헬퍼로만 보낸다. osascript 알림은 누르면 Script Editor가 열리고 권한 창을 따로 띄워서 쓰지 않는다.
    /// 헬퍼가 없으면 알리지 않는다. 헬퍼가 실패하면 다음 틱이 다시 보낸다.
    pub fn system() -> Self {
        let env = process_env();
        let runner: Arc<dyn ProcessRunner> = Arc::new(SystemProcessRunner);
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let layout = InstallLayout::current();
        let notifier = notifier_bin(&env, layout.as_ref());
        let mut context = Self::new(KairosHome::from_env(), clock, runner.clone(), env).with_console(Console::system());
        // doctor가 상태를 묻는 헬퍼는 실행 파일 쪽 설치 자료에서만 고른다. 지금 폴더는 남의 저장소일 수 있다.
        context.notifier_bin = doctor_notifier_bin(&context.env, layout.as_ref());
        if let Some(bin) = notifier {
            context = context.with_notifier(Arc::new(NativeNotifier::new(runner.clone(), bin)));
        }
        // 알림 뒤의 패널 열기는 Orca를 켰을 때만이다. 다른 호스트는 reveal.json과 next --json으로 스스로 연다.
        if kairos_core::profile::current().hosts.is_enabled("orca") {
            context = context.with_panel(Arc::new(OrcaPanelOpener::new(runner)));
        }
        Self { layout, ..context }
    }

    #[must_use]
    pub fn with_notifier(mut self, notifier: Arc<dyn Notifier>) -> Self {
        self.notifier = Some(notifier);
        self
    }

    #[must_use]
    pub fn with_panel(mut self, panel: Arc<dyn PanelOpener>) -> Self {
        self.panel = Some(panel);
        self
    }

    #[must_use]
    pub fn with_sleep_probe(mut self, sleep: Arc<dyn SleepProbe>) -> Self {
        self.sleep = Some(sleep);
        self
    }

    #[must_use]
    pub fn with_console(mut self, console: Console) -> Self {
        self.console = console;
        self
    }

    /// 소스를 바꾼다. 시험이 합성 소스를 넣는다.
    #[must_use]
    pub fn with_sources(mut self, sources: SourceRegistry) -> Self {
        self.sources = sources;
        self
    }

    #[must_use]
    pub fn with_layout(mut self, layout: InstallLayout) -> Self {
        self.layout = Some(layout);
        self
    }

    pub const fn home(&self) -> &KairosHome {
        &self.home
    }

    pub const fn console(&self) -> &Console {
        &self.console
    }

    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    pub fn clock(&self) -> &dyn Clock {
        self.clock.as_ref()
    }

    pub fn clock_arc(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    pub fn runner(&self) -> Arc<dyn ProcessRunner> {
        self.runner.clone()
    }

    pub fn env(&self, key: &str) -> Option<&str> {
        self.env.get(key).map(String::as_str)
    }

    /// JS의 참 거짓. 빈 문자열은 없는 값이다.
    pub fn env_set(&self, key: &str) -> bool {
        self.env(key).is_some_and(|value| !value.is_empty())
    }

    pub const fn registry(&self) -> &SourceRegistry {
        &self.sources
    }

    pub fn sources(&self) -> Vec<&dyn Source> {
        self.sources.sources().iter().map(AsRef::as_ref).collect()
    }

    pub const fn user_home(&self) -> &PathBuf {
        &self.user_home
    }

    pub fn layout(&self) -> KairosResult<&InstallLayout> {
        self.layout.as_ref().ok_or_else(|| KairosError::new("install_root_unknown"))
    }

    pub fn open_db(&self) -> KairosResult<Database> {
        Database::open(&self.home)
    }

    pub const fn control_file(&self) -> ControlFile<'_> {
        ControlFile::new(&self.home)
    }

    pub fn control(&self) -> KairosControl {
        self.control_file().read()
    }

    pub fn resumed_at(&self) -> Option<String> {
        ResumeFile::new(&self.home).read().map(Instant::to_iso)
    }

    /// 셸을 거치지 않고 데이터 폴더를 cwd로 부른다.
    pub fn orca(&self) -> OrcaClient {
        let bin = configured_binary(&self.env, "KAIROS_ORCA", "orca");
        OrcaClient::new(self.runner.clone(), bin, self.home.root()).with_env(self.env.clone())
    }

    /// 패널을 여는 orca. `KAIROS_ORCA`가 있으면 그 값, 없으면 틱과 같은 고정 경로다.
    /// 알림을 눌러 불릴 때는 PATH가 비어 있어서 PATH를 찾지 않는다.
    pub fn panel_orca_bin(&self) -> String {
        given(&self.env, "KAIROS_ORCA").unwrap_or(ORCA_BIN).to_owned()
    }

    /// Orca를 앞으로 끌어오는 open.
    /// `git` 세션 호스트. 시험은 `KAIROS_GIT`으로 가짜 git을 준다.
    pub fn git_workspaces(&self) -> kairos_sessions::GitWorkspaces {
        let bin = given(&self.env, "KAIROS_GIT").unwrap_or(kairos_sessions::GIT_BIN);
        let home = self.user_home.to_string_lossy().into_owned();
        kairos_sessions::GitWorkspaces::new(self.runner.clone(), bin, &home, self.home.root())
    }

    /// 'PR 올리기'. git은 `KAIROS_GIT`, gh는 `KAIROS_GH`나 PATH의 gh다. 사용자의 자격 증명 도우미와 gh 로그인을 쓴다.
    pub fn pull_requests<'a>(
        &self,
        database: &'a Database,
        orca: &'a OrcaClient,
    ) -> kairos_sessions::PullRequestPublisher<'a> {
        let git = given(&self.env, "KAIROS_GIT").unwrap_or(kairos_sessions::GIT_BIN).to_owned();
        let gh = configured_binary(&self.env, "KAIROS_GH", "gh").unwrap_or_else(|| "gh".to_owned());
        let path = self.env.get("PATH").map_or("/usr/bin:/bin", String::as_str);
        let home = self.user_home.to_string_lossy().into_owned();
        kairos_sessions::PullRequestPublisher::new(
            database,
            orca,
            self.runner.clone(),
            (&git, &gh),
            path,
            &home,
            self.home.root(),
        )
    }

    /// 세션 토큰의 키체인. 시험은 `KAIROS_SECURITY`로 가짜를 준다.
    pub fn keychain(&self) -> kairos_sessions::Keychain {
        let bin = given(&self.env, "KAIROS_SECURITY").unwrap_or(kairos_sessions::SECURITY_BIN);
        kairos_sessions::Keychain::new(self.runner.clone(), bin)
    }

    pub fn open_bin(&self) -> String {
        given(&self.env, "KAIROS_OPEN").unwrap_or(OPEN_BIN).to_owned()
    }

    /// kairos-run이 띄울 claude.
    pub fn claude_bin(&self) -> Option<String> {
        let given = self.env.get("KAIROS_CLAUDE").map(|value| js::trim(value)).filter(|value| !value.is_empty());
        given.map(str::to_owned).or_else(|| {
            let agents = self.user_home.join("Library").join("LaunchAgents");
            find_claude(self.env.get("PATH").map_or("", String::as_str), &agents)
        })
    }

    /// 틱 하나. 알림과 패널은 조립할 때 고른 값을 넘긴다.
    pub fn tick_runner(&self) -> TickRunner {
        let config = TickConfig::from_env(self.home.clone(), self.user_home.clone(), self.env.clone());
        let tick = TickRunner::new(
            config,
            self.clock.clone(),
            self.runner.clone(),
            self.notifier.clone(),
            self.panel.clone(),
            self.sources.sources().to_vec(),
        );
        match &self.sleep {
            Some(sleep) => tick.with_sleep_probe(sleep.clone()),
            None => tick,
        }
    }

    pub fn doctor(&self) -> Doctor {
        let config = DoctorConfig { home: self.home.clone(), user_home: self.user_home.clone(), env: self.env.clone() };
        Doctor::new(config, self.clock.clone(), self.runner.clone(), self.sources.sources().to_vec())
            .with_notifier(self.notifier_bin.clone())
    }

    /// state.json을 새로 쓴다. 상태 표시줄 건수가 조작 뒤 바로 맞게 한다.
    pub fn refresh_state(&self) -> KairosResult<()> {
        let database = self.open_db()?;
        let resumed_at = self.resumed_at();
        let state = StateBuilder::new(database.connection()).build(
            &self.sources(),
            self.now(),
            &self.control(),
            resumed_at.as_deref(),
        )?;
        StateBuilder::write_state(&state, &self.home)
    }

    /// 건강 경고 한 줄.
    /// 질의별 건강. 건강 경고 한 줄과 소스별 수집 상태가 같은 값에서 나온다.
    pub fn source_health(
        &self,
        database: &Database,
        at: Instant,
        control: &KairosControl,
    ) -> KairosResult<Vec<SourceHealth>> {
        let off_dates = OffDateBook::new(database.connection()).load_watch_off_dates(&control.off_date_set())?;
        let resumed_at = self.resumed_at();
        SourceHealthBook::new(database.connection()).source_health(
            &self.sources(),
            at,
            &off_dates,
            resumed_at.as_deref(),
        )
    }

    pub fn health_line(
        &self,
        database: &Database,
        at: Instant,
        control: &KairosControl,
    ) -> KairosResult<Option<String>> {
        Ok(SourceHealthBook::health_line(&self.source_health(database, at, control)?))
    }

    /// 카드를 고르는 데 쓰는 값. 표시 게이트가 처음 열리는 순간을 여기서도 남긴다.
    pub fn next_context(&self, database: &Database, at: Instant) -> KairosResult<NextContext> {
        let sources = self.sources();
        let control = self.control_file().latch_display(database.connection(), &sources, at, self.control())?;
        let control = kairos_agenda::SessionApproval::new(&self.home).effective(control);
        let gate = kairos_agenda::DisplayGate::current(database.connection(), &sources, at, &control)?;
        Ok(NextContext {
            now: at,
            off_dates: OffDateBook::new(database.connection()).load_off_dates(&control.off_date_set())?,
            health: self.health_line(database, at, &control)?,
            gate,
            control,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn 알림_헬퍼는_환경_값_다음_설치_자료_번들이고_파일이_없으면_쓰지_않는다() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["ops", "plugins", "bin"] {
            fs::create_dir(dir.path().join(name)).unwrap();
        }
        let layout = InstallLayout::from_binary(dir.path().join("bin").join("kairos")).unwrap();
        let bundled = dir.path().join(NOTIFIER_APP_REL);
        let empty = BTreeMap::from([("KAIROS_NOTIFIER".to_owned(), " ".to_owned())]);
        assert_eq!(notifier_bin(&empty, Some(&layout)), None, "번들이 아직 없다");
        assert_eq!(notifier_bin(&empty, None), None);
        fs::create_dir_all(bundled.parent().unwrap()).unwrap();
        fs::write(&bundled, "").unwrap();
        assert_eq!(notifier_bin(&empty, Some(&layout)), Some(bundled.to_string_lossy().into_owned()));
        let given = dir.path().join("fake-notifier");
        let env = BTreeMap::from([("KAIROS_NOTIFIER".to_owned(), given.to_string_lossy().into_owned())]);
        assert_eq!(notifier_bin(&env, Some(&layout)), None, "준 값이 없는 파일이면 알리지 않는다");
        fs::write(&given, "").unwrap();
        assert_eq!(notifier_bin(&env, Some(&layout)), Some(given.to_string_lossy().into_owned()));
    }
}
