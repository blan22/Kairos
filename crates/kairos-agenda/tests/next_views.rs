//! 뷰, 버튼, 목록, 플레이북.
#![allow(clippy::unwrap_used)]

mod common;
mod next_support;

use common::*;
use kairos_agenda::{ActionContext, DeployView, ItemState, KairosControl, Playbook, Presenter, issue_url_matches};
use kairos_core::model::{EventType, NormalizedEvent, NormalizedObject, SourceName, TrustTier};
use kairos_policy::ItemReason;
use next_support::*;
use regex::Regex;

/// 시험 프로필(`Profile::fixture`)의 플레이북 대상.
const PLAYBOOK_ORG: &str = "contoso";
const PLAYBOOK_REPOS: [&str; 2] = ["atlas", "atlas-specs"];

#[test]
fn 기준선으로_남긴_항목과_대기_항목은_카드가_되지_않는다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    let none = none();
    actions(&db).later(&[1], &ActionContext { now: at(&kst(MON, "10:00")), off_dates: &none }).unwrap();
    assert_eq!(peek(&db, &kst(MON, "10:01")).card, None);
    assert_eq!(list(&db, &kst(MON, "10:01")).items[0].state, ItemState::Parked);
}

#[test]
fn add로_만든_할_일은_목록에만_있고_카드가_되지_않는다() {
    let db = seeded();
    actions(&db).add("합성 할 일", at(&kst(MON, "10:00"))).unwrap();
    assert_eq!(peek(&db, &kst(MON, "13:31")).card, None);
    let shown = list(&db, &kst(MON, "13:31"));
    let row = &shown.items[0];
    assert_eq!(row.title, "합성 할 일");
    assert_eq!(row.actions.iter().map(|action| action.id.as_str()).collect::<Vec<_>>(), ["later", "done"]);
}

#[test]
fn 카드_문구는_정제되고_버튼_인자는_id와_고정_낱말뿐이다() {
    let db = seeded();
    let hostile = "\u{1b}[31m$(touch /tmp/x) `id`\u{202e}\n; rm -rf ~";
    let when = kst(MON, "10:00");
    let event = NormalizedEvent {
        text_head: Some(hostile.into()),
        permalink: Some("javascript:alert(1)".into()),
        ..mention("1", &when)
    };
    slack(&when).events(vec![event]).run(&db);
    refresh(&db, &when);
    let card = peek(&db, &when).card.unwrap();
    let row = &card.items[0];
    let control = Regex::new(r"[\x{0}-\x{1f}\x{7f}-\x{9f}\x{202a}-\x{202e}]").unwrap();
    assert!(!control.is_match(&row.title));
    assert_eq!(row.url, None, "https가 아닌 링크는 내지 않는다");
    let allowed = Regex::new(r"^(?:later|done|keep|accept|next|--json|--until-deploy|--ack|[0-9]+)$").unwrap();
    for action in card.actions.iter().chain(&row.actions) {
        for part in &action.argv {
            assert!(allowed.is_match(part), "{part}");
        }
    }
}

#[test]
fn 시작_버튼은_플레이북_조건을_통과한_담당_이슈에만_있다() {
    let db = seeded();
    let repo = PLAYBOOK_REPOS[0];
    let url = format!("https://github.com/{PLAYBOOK_ORG}/{repo}/issues/1");
    assert!(issue_url_matches(&url));
    assert!(!issue_url_matches(&format!("{url}?x=1")));
    assert!(!issue_url_matches(&format!("{url}\n")));
    let when = kst(MON, "10:00");
    let shaped = |number, permalink: String, trust_tier| NormalizedObject {
        permalink: Some(permalink),
        trust_tier: Some(trust_tier),
        ..issue(number)
    };
    let objects = vec![
        shaped(1, url.clone(), TrustTier::T1),
        shaped(2, format!("https://github.com/{PLAYBOOK_ORG}/{repo}/issues/2"), TrustTier::T3),
        shaped(3, format!("https://github.com/{PLAYBOOK_ORG}/other-repo/issues/3"), TrustTier::T1),
        shaped(4, "https://github.com/acme/widget/issues/4".into(), TrustTier::T1),
    ];
    github(&when).objects(objects.clone()).run(&db);
    for object in &objects {
        let event = NormalizedEvent {
            object_key: Some(object.object_key.clone()),
            trust_tier: Some(TrustTier::T1),
            ..comment(&format!("c{}", object.external_id), &when)
        };
        github(&when).events(vec![event]).run(&db);
    }
    refresh(&db, &when);
    let rows = list(&db, &when).items;
    let playbooks: Vec<_> = rows.iter().map(|row| row.playbook).collect();
    assert_eq!(playbooks, [Some(Playbook::QaIssue), None, None, None]);
    let start = &rows[0].actions[0];
    let id = rows[0].id.to_string();
    assert_eq!((start.id.as_str(), start.label.as_str()), ("start", "시작"));
    assert_eq!(start.argv, ["accept", id.as_str(), "--json"]);
    assert_eq!(rows[0].scope.as_deref(), Some(Playbook::QaIssue.scope()));
    assert!(!rows[1].actions.iter().any(|action| action.id == "start"));
}

#[test]
fn 세션이_잠겨_있으면_제안은_보이고_시작만_없다() {
    let db = seeded();
    let when = kst(MON, "10:00");
    let url = format!("https://github.com/{PLAYBOOK_ORG}/{}/issues/1", PLAYBOOK_REPOS[0]);
    let object = NormalizedObject { permalink: Some(url), trust_tier: Some(TrustTier::T1), ..issue(1) };
    let key = object.object_key.clone();
    github(&when).objects(vec![object]).run(&db);
    let event = NormalizedEvent { object_key: Some(key), trust_tier: Some(TrustTier::T1), ..comment("c1", &when) };
    github(&when).events(vec![event]).run(&db);
    refresh(&db, &when);
    let locked = context(&when, &control().with_sessions(None), Over::default());
    let shown = Presenter::new(&db).list_response(&locked, false).unwrap();
    let row = &shown.items[0];
    assert_eq!(row.playbook, Some(Playbook::QaIssue));
    assert!(row.scope.is_some());
    assert!(!row.actions.iter().any(|action| action.id == "start"));
}

#[test]
fn 플레이북이_없는_항목이_먼저_열려_있어도_제안은_한_항목에_붙는다() {
    let db = seeded();
    let when = kst(MON, "10:00");
    let url = format!("https://github.com/{PLAYBOOK_ORG}/{}/pull/1", PLAYBOOK_REPOS[0]);
    let object = NormalizedObject {
        permalink: Some(url),
        trust_tier: Some(TrustTier::T1),
        authored_by_me: Some(false),
        assigned_to_me: Some(true),
        ..pull_request(1)
    };
    let key = object.object_key.clone();
    github(&when).objects(vec![object]).run(&db);
    let event = |id: &str, event_type, at: &str| NormalizedEvent {
        object_key: Some(key.clone()),
        trust_tier: Some(TrustTier::T1),
        event_type,
        ..comment(id, at)
    };
    let later = kst(MON, "10:01");
    let events = vec![event("a", EventType::GhAssigned, &when), event("r", EventType::GhReviewRequested, &later)];
    github(&later).events(events).run(&db);
    refresh(&db, &later);
    let rows = list(&db, &later).items;
    let offers: Vec<_> = rows.iter().map(|row| (row.reason, row.playbook)).collect();
    assert_eq!(rows.len(), 2);
    assert!(offers.contains(&(ItemReason::Assigned, None)));
    assert!(offers.contains(&(ItemReason::ReviewOwed, Some(Playbook::PrReview))));
}

#[test]
fn 근거는_규칙과_사실_코드와_제안_조건뿐이고_외부_글을_싣지_않는다() {
    let db = seeded();
    let when = kst(MON, "10:00");
    let url = format!("https://github.com/{PLAYBOOK_ORG}/{}/issues/7", PLAYBOOK_REPOS[0]);
    let hostile = "$(touch /tmp/x) <img src=x onerror=alert(1)>";
    let object = NormalizedObject {
        permalink: Some(url),
        trust_tier: Some(TrustTier::T1),
        text_head: Some(hostile.into()),
        milestone_title: Some(hostile.into()),
        ..issue(7)
    };
    let key = object.object_key.clone();
    github(&when).objects(vec![object]).run(&db);
    let event = NormalizedEvent {
        object_key: Some(key),
        trust_tier: Some(TrustTier::T1),
        text_head: Some(hostile.into()),
        ..comment("c7", &when)
    };
    github(&when).events(vec![event]).run(&db);
    refresh(&db, &when);
    let mut rows = list(&db, &when).items;
    assert!(rows.iter().all(|row| row.why.is_none()), "Presenter는 근거를 채우지 않는다. golden의 바이트가 그대로다");
    kairos_agenda::Explainer::new(db.connection()).fill(&mut rows).unwrap();
    let why = rows[0].why.as_ref().unwrap();
    assert!(why.rule.is_some());
    assert_eq!(why.events, 1);
    assert!(why.facts.contains(&"issue"));
    assert!(why.facts.contains(&"milestone"), "마일스톤은 있다는 사실만 낸다");
    let offer = why.offer.as_ref().unwrap();
    assert_eq!(offer.repo, format!("{PLAYBOOK_ORG}/{}", PLAYBOOK_REPOS[0]));
    assert_eq!(offer.checks, ["assigned_me", "open", "mapped", "insiders", "no_session"]);
    let text = serde_json::to_string(why).unwrap();
    assert!(!text.contains("touch") && !text.contains("img"), "{text}");
}

#[test]
fn 플레이북마다_수락_범위가_있고_코멘트를_하지_않는다고_적는다() {
    for playbook in [Playbook::QaIssue, Playbook::PrReview, Playbook::ReplyDraft] {
        assert!(playbook.scope().contains("코멘트"), "{playbook}");
    }
}

#[test]
fn 배포_연결_항목의_남김은_배포_때_다시이고_건수는_호출_시점에_계산한다() {
    let db = seeded();
    prime(&db, SourceName::Calendar, "team");
    let milestone = |object| NormalizedObject { milestone_title: Some("합성 기능".into()), ..object };
    let linked = milestone(issue(7));
    github(T0).objects(vec![linked.clone(), milestone(issue(8))]).run(&db);
    let event =
        NormalizedObject { start_date: Some("2026-03-03".into()), end_date: Some("2026-03-04".into()), ..deploy("d") };
    calendar("team", T0).objects(vec![event]).run(&db);
    let owned = NormalizedEvent { object_key: Some(linked.object_key), ..comment("1", T1) };
    github(T1).events(vec![owned]).run(&db);
    refresh(&db, T1);
    let rows = list(&db, T1).items;
    let risk = rows.iter().find(|row| row.reason == ItemReason::DeployRisk).unwrap();
    assert_eq!(risk.deploy, Some(DeployView { date: date("2026-03-03"), linked_count: 2 }));
    let owned = rows.iter().find(|row| row.reason == ItemReason::ReplyOwed).unwrap();
    let keep = owned.actions.iter().find(|action| action.id == "keep").unwrap();
    assert_eq!((keep.label.as_str(), keep.argv[2].as_str()), ("배포 때 다시", "--until-deploy"));
    let risk_keep = risk.actions.iter().find(|action| action.id == "keep").unwrap();
    assert_eq!(risk_keep.label, "할일로 남김");
}

#[test]
fn 목록은_게이트가_닫히면_숨기고_전체_보기로만_보인다_노출을_기록하지_않는다() {
    let db = seeded();
    p1(&db, T0, &["C1"]);
    let auto = ctx_over(T1, Over::control(KairosControl::default()));
    let presenter = Presenter::new(&db);
    let hidden = presenter.list_response(&auto, false).unwrap();
    assert_eq!((hidden.items.len(), hidden.hidden, hidden.display.open), (0, 1, false));
    let all = presenter.list_response(&auto, true).unwrap();
    assert_eq!((all.items.len(), all.hidden), (1, 0));
    assert_eq!(count(&db, "suggestions"), 0);
}

#[test]
fn 남김을_고른_항목은_카드에서_내려가고_할_일로_목록에_남는다() {
    let db = seeded();
    p1(&db, &kst(MON, "10:00"), &["C1"]);
    see(&db, &kst(MON, "10:00"));
    let none = none();
    actions(&db).keep(&[1], &ActionContext { now: at(&kst(MON, "10:01")), off_dates: &none }, false).unwrap();
    assert_eq!(peek(&db, &kst(MON, "11:10")).card, None);
    let mut reasons: Vec<_> = list(&db, &kst(MON, "11:10")).items.iter().map(|row| row.reason.as_str()).collect();
    reasons.sort_unstable();
    assert_eq!(reasons, ["reply_owed", "work_owed"]);
}
