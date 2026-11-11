//! 대응하는 TS 시험은 없고 호환을 고정하려고 새로 썼다
//! 기대값은 지운 TS 구현을 Node 26에서 실행해 얻었다. 운영 DB의 해시와 맞아야 하니 고치지 않는다.
//! 이 값이 어긋나면 운영의 모든 일정이 변경으로 잡혀 가짜 알림이 쏟아진다.
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_core::model::{NormalizedObject, ObjectKind, SourceName, SourceResult, TrustTier};
use kairos_ledger::snapshot_hash;

fn full() -> NormalizedObject {
    NormalizedObject {
        permalink: Some("https://example.invalid/p?a=1&b=2".into()),
        actor_id: Some("U0SYNTH01".into()),
        channel_id: Some("C0SYNTH".into()),
        ts: Some("1772413200.000100".into()),
        text_head: Some("합성 😀 \"제목\"\t탭\n줄".into()),
        trust_tier: Some(TrustTier::T1),
        all_day: Some(false),
        start_date: Some("2026-03-03".into()),
        end_date: Some("2026-03-04".into()),
        start_at: Some("2026-03-03T10:00:00+09:00".into()),
        end_at: Some("2026-03-03T11:00:00+09:00".into()),
        series_id: Some("series".into()),
        attendee_count: Some(7),
        my_response: Some("needsAction".into()),
        busy: Some(false),
        state: Some("confirmed".into()),
        milestone_title: Some("é\u{301} \u{2028} 구분".into()),
        assigned_to_me: Some(true),
        authored_by_me: Some(false),
        review_requested_me: Some(true),
        is_draft: Some(false),
        review_decision: Some("CHANGES_REQUESTED".into()),
        checks_state: Some("FAILURE".into()),
        head_ref_oid: Some("abcdef0123456789".into()),
        ..NormalizedObject::new("gcal:\"따옴표\"\\역슬래시", SourceName::Calendar, ObjectKind::Meeting, "evt/1")
    }
}

#[test]
fn 없는_필드는_null이다() {
    let object = NormalizedObject::new("gh:acme/widget#1", SourceName::Github, ObjectKind::Issue, "1");
    assert_eq!(snapshot_hash(&object), "ef3db2c036488d9e696c395fdc7ca369810f88baebd4f83f70ce2d775bcf6424");
}

#[test]
fn 시간_일정() {
    assert_eq!(snapshot_hash(&meeting("m1")), "333bca7b40e7d0ef5b93452e832610dee84592b214e473720bc3db1ba0208cdd");
}

#[test]
fn 종일_일정() {
    assert_eq!(snapshot_hash(&deploy("d1")), "34fef57352bc05fa90726e7ab7ae0f97b070e7cb54d31e503b10f48ceef9a852");
}

#[test]
fn 모든_필드와_따옴표와_유니코드() {
    assert_eq!(snapshot_hash(&full()), "17f543ca0218221137bebc97b18e3c7351e1912417cd02c1503bb0f6837fcbae");
}

#[test]
fn 제어_문자의_이스케이프는_json_stringify와_같다() {
    let object = NormalizedObject {
        attendee_count: Some(0),
        busy: Some(false),
        ..NormalizedObject::new(
            "k",
            SourceName::Slack,
            ObjectKind::DmConversation,
            "\u{1}\u{8}\u{b}\u{c}\u{1f}\u{7f}\u{85}\u{2029}\u{feff}/",
        )
    };
    assert_eq!(snapshot_hash(&object), "cab71b0153e51647c1041b0f3ee58fcd96e17842a49b5f6d86ce820b59e7eb20");
}

/// 저장 경로는 정제한 값을 해시한다. 원본의 `cleaned` 이후다.
#[test]
fn 저장되는_해시는_정제_이후의_값이다() {
    let ansi = NormalizedObject {
        text_head: Some(
            "\u{1b}[31m경고\u{1b}[0m\u{202e} 보기 https://mg.example.invalid/x?y=1 끝 a.b@c.example".into(),
        ),
        milestone_title: Some("WWW.Example.com 1차\n릴리스".into()),
        permalink: Some(" https://github.invalid/acme/widget/issues/2\t".into()),
        actor_id: Some("   ".into()),
        channel_id: Some(String::new()),
        state: Some("open".into()),
        assigned_to_me: Some(true),
        authored_by_me: Some(false),
        checks_state: Some("\u{0}".into()),
        head_ref_oid: Some("a".repeat(40)),
        ..NormalizedObject::new(" gh:acme/widget#2\u{7}", SourceName::Github, ObjectKind::Issue, "\u{1b}[31m2\u{1b}[0m")
    };
    let long = NormalizedObject {
        text_head: Some("가".repeat(300)),
        milestone_title: Some(format!("{}가나", "😀".repeat(119))),
        permalink: Some(format!("https://x.invalid/{}", "p".repeat(600))),
        actor_id: Some(format!("{} b", "a".repeat(511))),
        review_decision: Some("CHANGES_REQUESTED".into()),
        ..NormalizedObject::new("gh:acme/widget#3", SourceName::Github, ObjectKind::PullRequest, "3")
    };
    let secret = NormalizedObject {
        text_head: Some(format!("토큰ghp_{}끝 xoxb-1234567890-abcdef {}", "a".repeat(36), "f".repeat(40))),
        state: Some("open".into()),
        ..NormalizedObject::new("gh:acme/widget#4", SourceName::Github, ObjectKind::Issue, "4")
    };

    let db = test_db();
    ingest(&db, SourceResult { objects: vec![ansi, long, secret], ..result() }, T0);

    let first = object_row(&db, "gh:acme/widget#2");
    assert_eq!(first.external_id, "2");
    assert_eq!(first.text_head.as_deref(), Some("경고 보기 [링크] 끝 [주소]"));
    assert_eq!(first.milestone_title.as_deref(), Some("[링크] 1차 릴리스"));
    assert_eq!(first.actor_id, None);
    assert_eq!(first.channel_id, None);
    assert_eq!(first.checks_state, None);
    assert_eq!(first.snapshot_hash, "ea901bec4febc865e2816e97851381a5e9b995aace8ec04f141e1f2d3679b383");

    let second = object_row(&db, "gh:acme/widget#3");
    assert_eq!(second.snapshot_hash, "f8c4e3b06fc2489adda37f1124fc978054ef9094c1e4c170db113e1f470acec5");

    let third = object_row(&db, "gh:acme/widget#4");
    assert_eq!(third.text_head.as_deref(), Some("토큰[redacted]끝 [redacted] [redacted]"));
    assert_eq!(third.snapshot_hash, "1ac83c1b2b0ec395fa019f2278961cee53e90d34169863125e86ca2eebcac77e");
}
