//! 배포 일정과 내 일감의 연결.

use kairos_core::model::ObjectKind;
use kairos_core::{Instant, LocalDate, OffDates, js, string_enum};
use unicode_normalization::UnicodeNormalization;

use crate::rule::RuleObject;

string_enum! {
    /// linked 연결 건 있음, cleared 마일스톤은 맞지만 열린 건 없음, no_link 조인 실패(추측하지 않는다).
    pub enum DeployLinkStatus {
        Linked => "linked",
        Cleared => "cleared",
        NoLink => "no_link",
    }
}

string_enum! {
    pub enum DeployStage {
        D0 => "d0",
        D1 => "d1",
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeployCandidate {
    pub object_key: String,
    pub kind: ObjectKind,
    pub state: Option<String>,
    pub text_head: Option<String>,
    pub milestone_title: Option<String>,
    pub assigned_to_me: bool,
    pub authored_by_me: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeployLink {
    pub status: DeployLinkStatus,
    pub milestones: Vec<String>,
    pub object_keys: Vec<String>,
}

fn comparable(value: Option<&str>) -> String {
    let composed: String = value.unwrap_or("").nfc().collect();
    js::trim(&composed).to_lowercase()
}

/// 배포 연결 판정기.
#[derive(Clone, Copy, Debug, Default)]
pub struct DeployLinker;

impl DeployLinker {
    /// 배포 연결 규칙은 하나다. [배포] 제목이 마일스톤 제목을 포함하면, 내 담당 열린 이슈와
    /// 제목에 마일스톤 제목이 들어간 내 열린 PR을 묶는다. 제목 토큰 유사도는 쓰지 않는다.
    pub fn link(deploy_title: Option<&str>, candidates: &[DeployCandidate]) -> DeployLink {
        let title = comparable(deploy_title);
        // 처음 나온 순서를 지킨다. `milestones`가 그 순서로 나간다.
        let mut known: Vec<String> = Vec::new();
        for candidate in candidates {
            let milestone = comparable(candidate.milestone_title.as_deref());
            if js::utf16_len(&milestone) >= 2 && !known.contains(&milestone) {
                known.push(milestone);
            }
        }
        let milestones: Vec<String> =
            known.into_iter().filter(|milestone| title.contains(milestone.as_str())).collect();
        if milestones.is_empty() {
            return DeployLink { status: DeployLinkStatus::NoLink, milestones: Vec::new(), object_keys: Vec::new() };
        }

        let is_linked = |candidate: &DeployCandidate| match candidate.kind {
            ObjectKind::Issue => {
                candidate.assigned_to_me && milestones.contains(&comparable(candidate.milestone_title.as_deref()))
            }
            ObjectKind::PullRequest => {
                let head = comparable(candidate.text_head.as_deref());
                candidate.authored_by_me && milestones.iter().any(|milestone| head.contains(milestone.as_str()))
            }
            _ => false,
        };
        let object_keys: Vec<String> = candidates
            .iter()
            .filter(|candidate| candidate.state.as_deref() == Some("open"))
            .filter(|candidate| is_linked(candidate))
            .map(|candidate| candidate.object_key.clone())
            .collect();
        let status = if object_keys.is_empty() { DeployLinkStatus::Cleared } else { DeployLinkStatus::Linked };
        DeployLink { status, milestones, object_keys }
    }

    /// 배포 날짜. 종일 일정은 저장된 날짜 그대로, 시간 일정은 KST 날짜다.
    pub fn date(object: &RuleObject) -> Option<LocalDate> {
        if object.all_day {
            return object.start_date.as_deref().and_then(LocalDate::parse);
        }
        object.start_at.as_deref().and_then(Instant::parse).map(Instant::local_date)
    }

    /// 오늘이 배포일이면 d0, 직전 근무일부터 배포 전날까지면 d1이다. 주말과 휴일은 근무일에서 뺀다.
    pub fn stage(date: LocalDate, today: LocalDate, off_dates: &OffDates) -> Option<DeployStage> {
        if today == date {
            return Some(DeployStage::D0);
        }
        (today >= off_dates.previous_workday(date) && today < date).then_some(DeployStage::D1)
    }
}
