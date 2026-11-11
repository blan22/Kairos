//! 사용자와 조직마다 다른 값. 코드에 박혀 있던 본인 식별자, 조직, 시간대, 근무 시간, 플레이북 매핑을 여기에 모은다.
//!
//! 값은 `kairos-config`가 설정 파일에서 읽고 검증해 만든다. 이 모듈은 모양과 전역 자리만 가진다.
//! 프로세스가 시작할 때 `install`로 한 번 정하고 그 뒤로 바꾸지 않는다. `Instant` 같은 값 객체가 시간대를 알아야 해서
//! 생성자 주입 대신 전역으로 둔다. 정하지 않고 읽으면 `Profile::empty`(본인 식별자 없음)다.
//!
//! 보안 불변식(외부 읽기 전용, 세션 샌드박스, 토큰 형식, 알림 고정 문구)은 여기에 없다. 설정으로 풀 수 없다.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use chrono::Weekday;
use chrono_tz::Tz;

/// 본인 식별자. 소스가 응답에서 확인한 신원과 비교하고, T0 판정에 쓴다.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Identity {
    /// 소문자 GitHub 로그인.
    pub github_login: Option<String>,
    /// Google 계정 주소. 개인 캘린더 ID이자 메일 주소다.
    pub google_email: Option<String>,
    /// Slack 사용자 ID(`U…`).
    pub slack_user: Option<String>,
    /// 사람이 쓰는 이름. 연차 일정 제목과 Notion 알림의 행위자 이름을 맞출 때 쓴다.
    pub display_name: Option<String>,
}

/// 조직이 정하는 값. 조직 프로필로 나눠 배포한다.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Org {
    /// GitHub 조직(소문자). 검색 범위이자 T1 판정의 바탕이다. 첫 값이 기본 조직이다.
    pub github_orgs: Vec<String>,
    /// 이 도메인과 하위 도메인의 메일 주소를 조직 사람(T1)으로 본다. 소문자.
    pub trusted_domains: Vec<String>,
    /// 배포 일정만 담는 그룹 캘린더 ID.
    pub deploy_calendar: Option<String>,
    /// 연차 일정이 올라오는 그룹 캘린더 ID.
    pub leave_calendar: Option<String>,
    /// 공휴일 캘린더 ID.
    pub holiday_calendar: Option<String>,
    /// 배포 일정 제목에 붙는 표시.
    pub deploy_tag: String,
    /// Notion 워크스페이스 표시 이름. 인기·추천 페이지 메일 제목을 알아볼 때 쓴다.
    pub notion_workspace: Option<String>,
}

impl Org {
    pub fn is_org(&self, owner: &str) -> bool {
        self.github_orgs.iter().any(|org| org.eq_ignore_ascii_case(owner))
    }

    /// 그룹 캘린더 중 신뢰하는 것. 그룹 캘린더는 아무나 만들 수 있어 ID로만 믿는다.
    pub fn is_trusted_calendar(&self, address: &str) -> bool {
        [&self.deploy_calendar, &self.leave_calendar].into_iter().flatten().any(|calendar| calendar == address)
    }

    /// `@`가 하나이고 앞뒤가 비지 않은 주소만 본다. 도메인은 영문자, 숫자, `.`, `-`뿐이어야 한다.
    /// `x@evil.example@<신뢰 도메인>`처럼 `@`가 여럿인 값이나 주소가 아닌 값은 외부다.
    /// 소문자로 바꾸기 전에 도메인 글자를 본다. 유니코드 소문자화는 켈빈 기호(U+212A)를 k로 바꿔 다른 이름을 신뢰 도메인으로 만든다.
    /// 빈 이름(`.contoso.example`, `a..contoso.example`)도 외부다.
    pub fn is_trusted_address(&self, address: &str) -> bool {
        let Some((local, domain)) = address.split_once('@') else {
            return false;
        };
        let domain = domain.to_ascii_lowercase();
        let domain_ok = domain.split('.').all(|label| !label.is_empty())
            && domain.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
        if local.is_empty() || !domain_ok {
            return false;
        }
        self.trusted_domains.iter().any(|trusted| domain == *trusted || domain.ends_with(&format!(".{trusted}")))
    }
}

/// 자정 이후 분으로 쓴 하루의 시각 구간. `start..end`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MinuteRange {
    pub start: u32,
    pub end: u32,
}

impl MinuteRange {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn contains(self, minutes: u32) -> bool {
        self.start <= minutes && minutes < self.end
    }
}

/// 근무 요일. 비트 0이 월요일이다. 비지 않는다(kairos-config가 검증한다).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkDays(u8);

impl WorkDays {
    /// 월~금.
    pub const WEEKDAYS: Self = Self(0b001_1111);
    pub const ALL: Self = Self(0b111_1111);
    /// 설정에 쓰는 이름. 월요일부터다.
    pub const NAMES: [&'static str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

    pub fn from_weekdays(days: impl IntoIterator<Item = Weekday>) -> Self {
        Self(days.into_iter().fold(0, |bits, day| bits | (1 << day.num_days_from_monday())))
    }

    /// 설정 이름(`mon`~`sun`) 목록. 모르는 이름이 있으면 None이다.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        names.into_iter().try_fold(Self(0), |days, name| {
            let bit = Self::NAMES.iter().position(|known| *known == name)?;
            Some(Self(days.0 | (1 << bit)))
        })
    }

    pub const fn contains(self, day: Weekday) -> bool {
        self.0 & (1 << day.num_days_from_monday()) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// 설정 이름 목록. 월요일부터다.
    pub fn names(self) -> Vec<&'static str> {
        (0..7).filter(|bit| self.0 & (1 << bit) != 0).map(|bit| Self::NAMES[bit]).collect()
    }

    /// ISO 요일 숫자(월 1 ~ 일 7)를 이어 쓴 값. 감시자가 `strftime %u`와 비교한다.
    pub fn iso_digits(self) -> String {
        (0..7u8).filter(|bit| self.0 & (1 << bit) != 0).map(|bit| char::from(b'1' + bit)).collect()
    }
}

/// 시간대와 근무 리듬.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Schedule {
    pub zone: Tz,
    /// 근무 요일. 이 요일 밖과 공휴일, 연차는 쉬는 날이라 카드와 알림을 내지 않는다.
    pub days: WorkDays,
    /// 카드와 알림을 내는 시간. 이 밖이 방해 금지 시간이다.
    pub work: MinuteRange,
    /// 카드를 보류하는 시간. 비어 있으면(`start == end`) 점심 보류가 없다.
    pub lunch: MinuteRange,
    /// P2 묶음 시각. 오름차순이고 하나 이상이다.
    pub bundles: Vec<u32>,
    /// 수집 틱이 도는 시간. launchd 일정과 감시자가 같은 값을 쓴다.
    pub tick: MinuteRange,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            zone: chrono_tz::Asia::Seoul,
            days: WorkDays::WEEKDAYS,
            work: MinuteRange::new(10 * 60, 19 * 60 + 30),
            lunch: MinuteRange::new(12 * 60, 13 * 60 + 30),
            bundles: vec![13 * 60 + 30, 18 * 60],
            tick: MinuteRange::new(9 * 60, 20 * 60),
        }
    }
}

/// 세션에 맡길 수 있는 저장소 묶음 하나.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaybookTarget {
    /// 소문자 조직. 대상 저장소는 모두 이 조직에 있다.
    pub owner: String,
    /// 이슈와 PR을 받는 저장소 이름들.
    pub repos: Vec<String>,
    /// 세션이 일하는 저장소(`owner/name`). 호스트가 자기 저장소 ID로 푼다.
    pub workspace_repo: String,
    /// Orca 저장소 ID. 없으면 Orca 호스트가 `workspace_repo`로 찾는다.
    pub orca_repo: Option<String>,
    /// 작업 공간을 만들 기준 브랜치(`origin/…`).
    pub base_branch: String,
}

impl PlaybookTarget {
    pub fn covers(&self, owner: &str, repo: &str) -> bool {
        self.owner.eq_ignore_ascii_case(owner) && self.repos.iter().any(|name| name == repo)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Playbooks {
    pub targets: Vec<PlaybookTarget>,
}

impl Playbooks {
    /// `https://github.com/<owner>/<repo>/(issues|pull)/<n>`의 대상. URL 모양 검사는 호출자가 먼저 한다.
    pub fn target_for_url(&self, url: &str) -> Option<&PlaybookTarget> {
        let mut parts = url.strip_prefix("https://github.com/")?.split('/');
        let owner = parts.next()?;
        let repo = parts.next()?;
        self.targets.iter().find(|target| target.covers(owner, repo))
    }

    /// 대상 조직들. 이슈 URL 정규식이 이 목록만 받는다.
    pub fn owners(&self) -> Vec<&str> {
        let mut owners: Vec<&str> = self.targets.iter().map(|target| target.owner.as_str()).collect();
        owners.sort_unstable();
        owners.dedup();
        owners
    }
}

/// 소스 하나의 MCP 바인딩. 커넥터가 정한 동작 이름(`mail.search`)을 이 컴퓨터의 서버와 도구 이름에 잇는다.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct McpBinding {
    pub server: String,
    /// 동작 이름 → 도구 이름.
    pub tools: BTreeMap<String, String>,
}

/// 표시면과 작업 공간을 맡는 호스트.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Hosts {
    /// 켠 호스트. 순서가 reveal을 시도하는 순서다.
    pub enabled: Vec<String>,
}

impl Default for Hosts {
    fn default() -> Self {
        Self { enabled: vec!["orca".to_owned()] }
    }
}

impl Hosts {
    pub fn is_enabled(&self, host: &str) -> bool {
        self.enabled.iter().any(|enabled| enabled == host)
    }
}

/// 수락이 작업 공간과 터미널을 맡기는 곳.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SessionHost {
    /// Orca가 worktree와 터미널을 만든다.
    #[default]
    Orca,
    /// kairos가 git worktree를 만들고, 호스트(VS Code, Claude Code)나 사용자가 터미널에서 kairos-run을 띄운다.
    Git,
}

impl SessionHost {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Orca => "orca",
            Self::Git => "git",
        }
    }
}

/// `git` 세션 호스트의 값.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct GitHost {
    /// `owner/name` → 로컬 클론의 절대 경로.
    pub repos: BTreeMap<String, std::path::PathBuf>,
    /// worktree를 만들 폴더. 없으면 클론 옆의 `<클론 이름>-kairos`다.
    pub worktree_root: Option<std::path::PathBuf>,
}

/// 사용자와 조직마다 다른 값 전부.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Profile {
    pub identity: Identity,
    pub org: Org,
    pub schedule: Schedule,
    pub playbooks: Playbooks,
    /// 끈 소스 이름. 끈 소스는 연결 없음으로 남아 doctor에 이유가 나온다.
    pub disabled_sources: Vec<String>,
    pub hosts: Hosts,
    /// 소스 이름 → MCP 바인딩. 없는 소스는 커넥터의 기본 이름을 쓴다.
    pub bindings: BTreeMap<String, McpBinding>,
    /// `kairos mcp serve`가 에이전트에 여는 상태 변경 동작. 기본은 없음(읽기만).
    pub mcp_actions: Vec<String>,
    pub session_host: SessionHost,
    pub git: GitHost,
}

impl Profile {
    /// 설정이 없을 때. 본인 식별자와 조직이 없어 그 값이 필요한 소스는 연결 없음으로 남는다.
    pub fn empty() -> Self {
        Self { org: Org { deploy_tag: "[배포]".to_owned(), ..Org::default() }, ..Self::default() }
    }

    /// 시험이 쓰는 합성 프로필. 시험 고정값의 식별자와 맞춘다.
    #[cfg(any(test, feature = "test-profile"))]
    pub fn fixture() -> Self {
        Self {
            identity: Identity {
                github_login: Some("kairos-dev".to_owned()),
                google_email: Some("kairos-dev@contoso.example".to_owned()),
                slack_user: Some("U0SYNTH0001".to_owned()),
                display_name: Some("김준서".to_owned()),
            },
            org: Org {
                github_orgs: vec!["contoso".to_owned()],
                trusted_domains: vec!["contoso.example".to_owned(), "atlas.example".to_owned()],
                deploy_calendar: Some("c_synthetic0deploy0calendar0@group.calendar.google.com".to_owned()),
                leave_calendar: Some("contoso.example_synthetic0leave00calendar0@group.calendar.google.com".to_owned()),
                holiday_calendar: Some("ko.south_korea#holiday@group.v.calendar.google.com".to_owned()),
                deploy_tag: "[배포]".to_owned(),
                notion_workspace: Some("Contoso".to_owned()),
            },
            schedule: Schedule::default(),
            playbooks: Playbooks {
                targets: vec![PlaybookTarget {
                    owner: "contoso".to_owned(),
                    repos: vec!["atlas".to_owned(), "atlas-specs".to_owned()],
                    workspace_repo: "contoso/atlas".to_owned(),
                    orca_repo: Some("0000c0de-0000-4000-8000-000000000001".to_owned()),
                    base_branch: "origin/fe-stage".to_owned(),
                }],
            },
            disabled_sources: Vec::new(),
            hosts: Hosts::default(),
            bindings: BTreeMap::new(),
            mcp_actions: Vec::new(),
            session_host: SessionHost::Orca,
            git: GitHost::default(),
        }
    }
}

static INSTALLED: OnceLock<Profile> = OnceLock::new();
static FALLBACK: OnceLock<Profile> = OnceLock::new();

/// 이미 다른 프로필을 정했다.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AlreadyInstalled;

/// 프로세스의 프로필을 정한다. 이미 정했으면 거절한다(같은 값이면 받아들인다).
pub fn install(profile: Profile) -> Result<(), AlreadyInstalled> {
    match INSTALLED.set(profile) {
        Ok(()) => Ok(()),
        Err(rejected) if INSTALLED.get() == Some(&rejected) => Ok(()),
        Err(_) => Err(AlreadyInstalled),
    }
}

/// 정한 프로필. 정하지 않았으면 빈 프로필이다. 시험 빌드(`test-profile`)에서는 합성 프로필이다.
pub fn current() -> &'static Profile {
    INSTALLED.get().unwrap_or_else(|| FALLBACK.get_or_init(fallback))
}

pub fn is_installed() -> bool {
    INSTALLED.get().is_some()
}

#[cfg(any(test, feature = "test-profile"))]
fn fallback() -> Profile {
    Profile::fixture()
}

#[cfg(not(any(test, feature = "test-profile")))]
fn fallback() -> Profile {
    Profile::empty()
}

/// 지금 프로필의 시간대.
pub fn zone() -> Tz {
    current().schedule.zone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 근무_요일은_월요일부터_이름과_iso_숫자로_읽는다() {
        let days = WorkDays::from_weekdays([Weekday::Sun, Weekday::Mon, Weekday::Sat]);
        assert_eq!(days.names(), ["mon", "sat", "sun"]);
        assert_eq!(days.iso_digits(), "167");
        assert!(days.contains(Weekday::Sun) && !days.contains(Weekday::Tue));
        assert_eq!(WorkDays::WEEKDAYS.iso_digits(), "12345");
        assert_eq!(WorkDays::ALL.names().len(), 7);
        assert!(WorkDays::from_weekdays([]).is_empty());
        assert_eq!(WorkDays::from_names(["sat", "mon"]), Some(WorkDays::from_weekdays([Weekday::Mon, Weekday::Sat])));
        assert_eq!(WorkDays::from_names(["Mon"]), None);
    }

    #[test]
    fn 신뢰_도메인은_하위_도메인까지_받는다() {
        let org = Profile::fixture().org;
        assert!(org.is_trusted_address("a@contoso.example"));
        assert!(org.is_trusted_address("a@x.Contoso.example"));
        assert!(!org.is_trusted_address("a@notcontoso.example"));
        for forged in [
            "x@evil.example@contoso.example",
            "contoso.example",
            "@contoso.example",
            "a@contoso.example>",
            "a@contoso .example",
            "a@.contoso.example",
            "a@x..contoso.example",
            "a@contoso.example.",
            "a@\u{212a}contoso.example",
        ] {
            assert!(!org.is_trusted_address(forged), "{forged}");
        }
    }

    #[test]
    fn 플레이북_대상은_조직과_저장소로_찾는다() {
        let playbooks = Profile::fixture().playbooks;
        assert!(playbooks.target_for_url("https://github.com/contoso/atlas/issues/1").is_some());
        assert!(playbooks.target_for_url("https://github.com/Contoso/atlas-specs/pull/2").is_some());
        assert!(playbooks.target_for_url("https://github.com/contoso/other/issues/1").is_none());
    }
}
