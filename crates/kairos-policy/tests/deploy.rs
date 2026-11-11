//! 배포 연결.
#![allow(clippy::unwrap_used)]

use kairos_core::model::ObjectKind;
use kairos_core::{Instant, LocalDate, OffDates};
use kairos_policy::{DeployCandidate, DeployLink, DeployLinkStatus, DeployLinker, DeployStage, RuleObject};

const TITLE: Option<&str> = Some("[배포] 합성 기능 고도화");

fn candidate(object_key: &str, milestone: Option<&str>) -> DeployCandidate {
    DeployCandidate {
        object_key: object_key.into(),
        kind: ObjectKind::Issue,
        state: Some("open".into()),
        text_head: Some("합성 이슈".into()),
        milestone_title: milestone.map(Into::into),
        assigned_to_me: true,
        authored_by_me: false,
    }
}

fn pull_request(object_key: &str, head: &str, authored_by_me: bool) -> DeployCandidate {
    DeployCandidate {
        kind: ObjectKind::PullRequest,
        authored_by_me,
        assigned_to_me: false,
        text_head: Some(head.into()),
        ..candidate(object_key, None)
    }
}

fn date(text: &str) -> LocalDate {
    LocalDate::parse(text).unwrap()
}

fn rule_object(all_day: bool, start_date: Option<&str>, start_at: Option<&str>) -> RuleObject {
    RuleObject {
        kind: ObjectKind::Deploy,
        state: None,
        all_day,
        start_date: start_date.map(Into::into),
        end_date: None,
        start_at: start_at.map(Into::into),
        end_at: None,
        my_response: None,
        busy: None,
        assigned_to_me: false,
        authored_by_me: false,
    }
}

fn stage(deploy: &str, today: &str) -> Option<DeployStage> {
    DeployLinker::stage(date(deploy), date(today), &OffDates::new())
}

#[test]
fn 배포_연결_마일스톤_이슈와_제목이_맞는_내_pr을_묶는다() {
    let milestone = Some("합성 기능 고도화");
    let link = DeployLinker::link(
        TITLE,
        &[
            candidate("a", milestone),
            candidate("b", Some("다른 마일스톤")),
            DeployCandidate { state: Some("closed".into()), ..candidate("c", milestone) },
            DeployCandidate { assigned_to_me: false, ..candidate("d", milestone) },
            pull_request("e", "feat: 합성 기능 고도화 API", true),
            pull_request("f", "feat: 무관한 작업", true),
            pull_request("g", "합성 기능 고도화 남의 PR", false),
        ],
    );
    assert_eq!(link.status, DeployLinkStatus::Linked);
    assert_eq!(link.object_keys, ["a", "e"]);
}

#[test]
fn 배포_연결_조인_실패는_연결_없음이고_토큰_유사도로_추측하지_않는다() {
    let link = DeployLinker::link(TITLE, &[candidate("gh:acme/widget#1", Some("합성 기능 개선"))]);
    assert_eq!(link, DeployLink { status: DeployLinkStatus::NoLink, milestones: vec![], object_keys: vec![] });
}

#[test]
fn 배포_연결_마일스톤은_맞지만_열린_건이_없으면_cleared() {
    let closed = DeployCandidate { state: Some("closed".into()), ..candidate("a", Some("합성 기능 고도화")) };
    assert_eq!(DeployLinker::link(TITLE, &[closed]).status, DeployLinkStatus::Cleared);
}

#[test]
fn 배포_날짜_종일_일정은_저장된_날짜_시간_일정은_kst_날짜() {
    assert_eq!(DeployLinker::date(&rule_object(true, Some("2026-11-05"), None)), Some(date("2026-11-05")));
    assert_eq!(DeployLinker::date(&rule_object(false, None, Some("2026-11-04T15:30:00Z"))), Some(date("2026-11-05")));
}

#[test]
fn 배포_단계_당일_전_근무일_그_밖() {
    assert_eq!(stage("2026-11-05", "2026-11-05"), Some(DeployStage::D0));
    assert_eq!(stage("2026-11-05", "2026-11-04"), Some(DeployStage::D1));
    assert_eq!(stage("2026-11-05", "2026-11-03"), None);
    assert_eq!(stage("2026-11-05", "2026-11-06"), None);
}

#[test]
fn 배포_단계_월요일_배포의_d1은_금요일이고_주말에도_유지된다() {
    assert_eq!(stage("2026-11-09", "2026-11-06"), Some(DeployStage::D1));
    assert_eq!(stage("2026-11-09", "2026-11-08"), Some(DeployStage::D1));
    assert_eq!(stage("2026-11-09", "2026-11-05"), None);
}

#[test]
fn 종일_배포일_함정_자정_직후_kst는_utc로_전날이어도_당일이다() {
    let deploy = DeployLinker::date(&rule_object(true, Some("2026-11-05"), None)).unwrap();
    let just_after_midnight = Instant::parse("2026-11-04T15:30:00Z").unwrap().local_date();
    assert_eq!(DeployLinker::stage(deploy, just_after_midnight, &OffDates::new()), Some(DeployStage::D0));
}

// 아래는 원본에 없는 시험이다. 문자열 비교 전 처리를 고정한다.

#[test]
fn 배포_연결은_nfc와_대소문자와_공백을_맞춰_비교한다() {
    // 맥에서 만든 제목은 자모가 풀린 NFD일 수 있다.
    let decomposed = "\u{1112}\u{1161}\u{11B8}\u{1109}\u{1165}\u{11BC}";
    let link =
        DeployLinker::link(Some("[배포] 합성 API"), &[candidate("a", Some(&format!("  {decomposed} api\u{FEFF} ")))]);
    assert_eq!(link.status, DeployLinkStatus::Linked);
    assert_eq!(link.milestones, ["합성 api"]);
}

#[test]
fn 배포_연결은_한_글자_마일스톤을_쓰지_않고_길이는_utf16으로_센다() {
    assert_eq!(DeployLinker::link(TITLE, &[candidate("a", Some("합"))]).status, DeployLinkStatus::NoLink);
    let emoji = DeployLinker::link(Some("[배포] 😀"), &[candidate("a", Some("😀"))]);
    assert_eq!(emoji.status, DeployLinkStatus::Linked);
}

#[test]
fn 배포_연결의_마일스톤은_처음_나온_순서로_한_번씩_나온다() {
    let link = DeployLinker::link(
        Some("[배포] 나나 가가"),
        &[candidate("a", Some("나나")), candidate("b", Some("가가")), candidate("c", Some("나나"))],
    );
    assert_eq!(link.milestones, ["나나", "가가"]);
    assert_eq!(link.object_keys, ["a", "b", "c"]);
}
