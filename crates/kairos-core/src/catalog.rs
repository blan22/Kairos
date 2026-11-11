//! 소스 목록. 소스마다 무엇을 읽는지, 무엇이 있어야 도는지를 한 곳에 둔다.
//!
//! 수집 등록(`kairos-tick`), 켜고 끄기(`kairos sources`), 처음 설정(`kairos setup`)이 이 표를 함께 읽는다.
//! 소스를 더하면 `SourceName`에 이름을 더하고 여기에 한 줄을 더한다. 빠지면 시험이 실패한다.

use crate::model::SourceName;
use crate::profile::Profile;

/// 소스가 돌려면 있어야 하는 값. 본인 식별자는 `kairos connect`가 쓰고, 조직 값은 조직 프로필에서 온다.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Need {
    GithubLogin,
    GithubOrg,
    GoogleEmail,
    SlackUser,
    DisplayName,
}

impl Need {
    /// `kairos sources --json`의 키.
    pub const fn key(self) -> &'static str {
        match self {
            Self::GithubLogin => "github_login",
            Self::GithubOrg => "github_org",
            Self::GoogleEmail => "google_email",
            Self::SlackUser => "slack_user",
            Self::DisplayName => "display_name",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::GithubLogin => "GitHub 로그인",
            Self::GithubOrg => "GitHub 조직",
            Self::GoogleEmail => "Google 주소",
            Self::SlackUser => "Slack 사용자 ID",
            Self::DisplayName => "이름",
        }
    }

    /// 채우는 명령.
    pub const fn hint(self) -> &'static str {
        match self {
            Self::GithubLogin => "kairos connect github",
            Self::GithubOrg => "조직 프로필(kairos config init --org)",
            Self::GoogleEmail => "kairos connect google <주소>",
            Self::SlackUser => "kairos connect slack <사용자 ID>",
            Self::DisplayName => "kairos connect name <이름>",
        }
    }

    /// 사용자가 `kairos connect`로 채울 수 있는가. 조직 값은 조직 관리자가 배포한다.
    pub const fn connectable(self) -> bool {
        !matches!(self, Self::GithubOrg)
    }

    pub fn met(self, profile: &Profile) -> bool {
        let identity = &profile.identity;
        match self {
            Self::GithubLogin => identity.github_login.is_some(),
            Self::GithubOrg => !profile.org.github_orgs.is_empty(),
            Self::GoogleEmail => identity.google_email.is_some(),
            Self::SlackUser => identity.slack_user.is_some(),
            Self::DisplayName => identity.display_name.is_some(),
        }
    }
}

/// 소스 하나의 설명.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceSpec {
    pub name: SourceName,
    /// 무엇을 읽는가. 사람이 읽는 한 줄이다.
    pub summary: &'static str,
    /// 모두 있어야 돈다. 없으면 수집 등록이 `설정 필요`로 남긴다.
    pub requires: &'static [Need],
    /// 없어도 돌지만 있으면 더 읽는다.
    pub optional: &'static [Need],
}

/// 소스의 지금 상태.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Readiness {
    /// 사용자가 껐다.
    Off,
    /// 켰지만 값이 모자란다.
    Missing(Vec<Need>),
    Ready,
}

impl Readiness {
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Missing(_) => "needs_setup",
            Self::Ready => "ready",
        }
    }

    pub const fn label(&self) -> &'static str {
        match self {
            Self::Off => "끔",
            Self::Missing(_) => "설정 필요",
            Self::Ready => "켬",
        }
    }
}

/// `SourceName::ALL`과 같은 순서다.
pub const CATALOG: [SourceSpec; 5] = [
    SourceSpec {
        name: SourceName::Calendar,
        summary: "개인 일정, 배포, 공휴일, 연차 캘린더",
        requires: &[Need::GoogleEmail],
        optional: &[Need::DisplayName],
    },
    SourceSpec {
        name: SourceName::Github,
        summary: "담당 이슈, 내 PR, 리뷰 요청, 멘션",
        requires: &[Need::GithubLogin, Need::GithubOrg],
        optional: &[],
    },
    SourceSpec {
        name: SourceName::Slack,
        summary: "나를 부른 메시지와 스레드",
        requires: &[Need::SlackUser],
        optional: &[],
    },
    SourceSpec {
        name: SourceName::Gmail,
        summary: "신뢰 도메인에서 온 메일",
        requires: &[Need::GoogleEmail],
        optional: &[],
    },
    SourceSpec {
        name: SourceName::Notion,
        summary: "Notion 알림 메일, Gmail로 읽음",
        requires: &[Need::GoogleEmail],
        optional: &[],
    },
];

impl SourceSpec {
    pub fn is_disabled(&self, profile: &Profile) -> bool {
        profile.disabled_sources.iter().any(|disabled| disabled == self.name.as_str())
    }

    /// 모자란 필수 값.
    pub fn missing(&self, profile: &Profile) -> Vec<Need> {
        self.requires.iter().copied().filter(|need| !need.met(profile)).collect()
    }

    pub fn readiness(&self, profile: &Profile) -> Readiness {
        if self.is_disabled(profile) {
            return Readiness::Off;
        }
        let missing = self.missing(profile);
        if missing.is_empty() { Readiness::Ready } else { Readiness::Missing(missing) }
    }
}

/// 이름으로 찾는다. 표에 없는 이름은 없다(시험이 지킨다).
pub fn spec(name: SourceName) -> &'static SourceSpec {
    CATALOG.iter().find(|spec| spec.name == name).unwrap_or(&CATALOG[0])
}

/// 문자열 이름으로 찾는다. 사용자가 친 값이다.
pub fn find(name: &str) -> Option<&'static SourceSpec> {
    CATALOG.iter().find(|spec| spec.name.as_str() == name)
}

/// 켠 소스들이 필요로 하는 값. 필수가 앞이고 같은 값은 한 번만 나온다.
pub fn needs_of<'a>(specs: impl IntoIterator<Item = &'a SourceSpec> + Clone) -> Vec<Need> {
    let mut needs: Vec<Need> = Vec::new();
    let required = specs.clone().into_iter().flat_map(|spec| spec.requires.iter().copied());
    let optional = specs.into_iter().flat_map(|spec| spec.optional.iter().copied());
    for need in required.chain(optional) {
        if !needs.contains(&need) {
            needs.push(need);
        }
    }
    needs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 표는_모든_소스를_이름_순서대로_담는다() {
        let names: Vec<SourceName> = CATALOG.iter().map(|spec| spec.name).collect();
        assert_eq!(names, SourceName::ALL);
    }

    #[test]
    fn 끈_소스는_값이_있어도_끔이다() {
        let mut profile = Profile::fixture();
        assert_eq!(spec(SourceName::Slack).readiness(&profile), Readiness::Ready);
        profile.disabled_sources = vec!["slack".to_owned()];
        assert_eq!(spec(SourceName::Slack).readiness(&profile), Readiness::Off);
    }

    #[test]
    fn 모자란_값을_알려_준다() {
        let profile = Profile::empty();
        assert_eq!(
            spec(SourceName::Github).readiness(&profile),
            Readiness::Missing(vec![Need::GithubLogin, Need::GithubOrg])
        );
        // 이름은 연차 일정만 더 읽게 한다. 없어도 캘린더는 돈다.
        let mut profile = Profile::fixture();
        profile.identity.display_name = None;
        assert_eq!(spec(SourceName::Calendar).readiness(&profile), Readiness::Ready);
    }

    #[test]
    fn 필요한_값은_필수가_먼저고_겹치지_않는다() {
        let needs = needs_of([spec(SourceName::Calendar), spec(SourceName::Gmail), spec(SourceName::Slack)]);
        assert_eq!(needs, [Need::GoogleEmail, Need::SlackUser, Need::DisplayName]);
    }
}
