//! 항목, 표시, 정제 시험(R1~R3, R7~R9, R14, R16, R17).
//! 번호는 .codex/runs/2026-11-04-stage2.md의 표와 같다. 고정값은 모두 합성이다.
#![allow(clippy::unwrap_used)]

#[path = "../../kairos-agenda/tests/common/mod.rs"]
mod common;

use std::fs;

use common::*;
use kairos_agenda::{
    ActionContext, ActionStatus, ItemCloseReason, ItemState, ItemTier, KairosControl, ListResponse, NextOptions,
    NextResponse, Presenter, StateBuilder, TriageDecision,
};
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, NormalizedObject, SourceName};
use kairos_core::text::{sanitize_external, sanitize_head};
use kairos_ops::MaintenanceRepo;
use kairos_policy::ItemReason;
use kairos_store::Database;

/// 원본의 control. 표시가 켜져 있고 세션도 열렸다.
fn control() -> KairosControl {
    control_on().with_sessions(Some(T0))
}

fn ctx(now: &str) -> kairos_agenda::NextContext {
    context(now, &control(), Over::default())
}

fn next(db: &Database, now: &str) -> NextResponse {
    next_with(db, &ctx(now), NextOptions::default())
}

fn list(db: &Database, now: &str) -> ListResponse {
    Presenter::new(db).list_response(&ctx(now), false).unwrap()
}

fn refresh_ok(db: &Database, now: &str, ok_sources: &[&str]) {
    refresh_with(db, now, &none(), ok_sources);
}

fn live(db: &Database) -> Vec<(i64, ItemReason)> {
    items(db).into_iter().filter(|row| row.state != ItemState::Closed).map(|row| (row.id, row.reason)).collect()
}

fn day(date: &str) -> String {
    kst(date, "10:00")
}

fn closed_event(id: &str, when: &str) -> NormalizedEvent {
    NormalizedEvent {
        object_key: Some(format!("gh:acme/widget#{id}")),
        ..NormalizedEvent::new(
            format!("gh:closed:{id}"),
            SourceName::Github,
            EventType::GhClosed,
            when,
            ActorKind::Human,
        )
    }
}

fn would_show(db: &Database) -> usize {
    StateBuilder::new(db.connection()).would_show_events().unwrap().len()
}

fn disposition(db: &Database, key: &str) -> Option<String> {
    event_text(db, key, "disposition")
}

fn event_exists(db: &Database, key: &str) -> bool {
    db.connection()
        .query_row("SELECT count(*) FROM events WHERE dedupe_key = ?", [key], |row| row.get::<_, i64>(0))
        .unwrap()
        > 0
}

fn owned_issue(db: &Database, when: &str) {
    prime(db, SourceName::Github, "tick");
    github(when).objects(vec![issue(1)]).run(db);
    github(when).events(vec![comment("c1", when)]).run(db);
    refresh(db, when);
}

fn keep(db: &Database, ids: &[i64], now: &str, until_deploy: bool) -> ActionStatus {
    let off_dates = none();
    let context = ActionContext { now: at(now), off_dates: &off_dates };
    actions(db).keep(ids, &context, until_deploy).unwrap()[0].status
}

fn prune_items(db: &Database, now: &str) -> i64 {
    MaintenanceRepo::new(db.connection()).prune_items(at(now)).unwrap()
}

fn prune_events(db: &Database, now: &str) -> i64 {
    MaintenanceRepo::new(db.connection()).prune_events(at(now)).unwrap()
}

#[test]
fn r1_끝낸_항목의_객체에_새_사람_댓글이_오면_접지_않고_새_항목을_연다() {
    let db = test_db();
    owned_issue(&db, T0);
    assert_eq!(items(&db).iter().map(|row| row.reason).collect::<Vec<_>>(), [ItemReason::ReplyOwed]);
    actions(&db).done(&[1], at(T1), false).unwrap();
    github(T2).events(vec![comment("c2", T2)]).run(&db);
    refresh(&db, T2);
    assert_eq!(disposition(&db, "gh:comment:c2").as_deref(), Some("opened"));
    assert_eq!(live(&db), [(2, ItemReason::ReplyOwed)]);
}

#[test]
fn r1_버린_항목도_같다_살아_있는_항목이면_그대로_접는다() {
    let db = test_db();
    owned_issue(&db, T0);
    github(T1).events(vec![comment("c2", T1)]).run(&db);
    assert_eq!(disposition(&db, "gh:comment:c2").as_deref(), Some("folded"), "살아 있는 항목의 댓글은 접는다");
    actions(&db).done(&[1], at(T1), true).unwrap();
    github(T2).events(vec![comment("c3", T2)]).run(&db);
    assert_eq!(disposition(&db, "gh:comment:c3").as_deref(), Some("opened"));
}

#[test]
fn r1_배포_연결_객체의_댓글도_끝낸_뒤에는_새_p1_항목을_연다() {
    let db = test_db();
    prime(&db, SourceName::Github, "tick");
    prime(&db, SourceName::Calendar, "mine");
    let linked = NormalizedObject { milestone_title: Some("합성 기능".into()), ..issue(7) };
    let key = linked.object_key.clone();
    github(T0).objects(vec![linked]).run(&db);
    calendar("mine", T0).objects(vec![deploy("d1")]).run(&db);
    let on_linked = |id: &str, when: &str| NormalizedEvent { object_key: Some(key.clone()), ..comment(id, when) };
    github(T0).events(vec![on_linked("c1", T0)]).run(&db);
    refresh(&db, T0);
    let first = items(&db).into_iter().find(|row| row.reason == ItemReason::ReplyOwed).unwrap();
    assert_eq!(first.tier, ItemTier::P1);
    actions(&db).done(&[first.id], at(T1), false).unwrap();
    github(T2).events(vec![on_linked("c2", T2)]).run(&db);
    refresh(&db, T2);
    assert_eq!(event_text(&db, "gh:comment:c2", "rule_id").as_deref(), Some("p1.deploy_owned_comment"));
    assert_eq!(disposition(&db, "gh:comment:c2").as_deref(), Some("opened"));
    let open = items(&db).into_iter().filter(|row| row.reason == ItemReason::ReplyOwed);
    assert_eq!(open.filter(|row| row.state == ItemState::Open).count(), 1);
}

#[test]
fn r1_할일로_남겨_대기인_항목은_접힌_댓글로_깨고_접힌_이벤트가_종료를_막지_않는다() {
    let db = test_db();
    owned_issue(&db, T0);
    keep(&db, &[1], T1, false);
    assert_eq!(item(&db, 1).state, ItemState::Parked);
    github(T2).events(vec![comment("c2", T2)]).run(&db);
    assert_eq!(disposition(&db, "gh:comment:c2").as_deref(), Some("folded"));
    refresh(&db, T2);
    assert_eq!(item(&db, 1).state, ItemState::Open, "접힌 댓글도 새 사건이다");
    let mine = NormalizedEvent { actor_kind: ActorKind::SelfActor, actor_id: Some("me".into()), ..comment("me", T3) };
    github(T3).events(vec![mine]).run(&db);
    refresh(&db, T3);
    assert_eq!(item(&db, 1).state, ItemState::Closed);
    assert_eq!(item(&db, 1).close_reason, Some(ItemCloseReason::DoneEvidence));
}

#[test]
fn r2_같은_날_끝낸_항목의_이벤트는_항목_정리_뒤_고아로_남지_않고_표시_후보로_되살아나지_않는다() {
    let db = test_db();
    prime_slack(&db);
    let when = kst(MON, "10:30");
    slack(&when).events(vec![mention("1", &when)]).run(&db);
    refresh(&db, &when);
    actions(&db).done(&[1], at(&when), false).unwrap();
    assert_eq!(would_show(&db), 0);
    let later = day("2026-04-02");
    assert_eq!(prune_items(&db, &later), 1);
    assert_eq!(would_show(&db), 0, "항목 행이 지워져도 끝낸 건이 다시 나오지 않는다");
    prune_events(&db, &later);
    assert!(!event_exists(&db, &format!("slack:{CHANNEL}:1")));
}

#[test]
fn r2_대조_항목이_살아_있거나_항목이_없는_열린_이벤트는_지우지_않는다() {
    let db = test_db();
    prime_slack(&db);
    let when = kst(MON, "10:30");
    slack(&when).events(vec![mention("1", &when)]).run(&db);
    refresh(&db, &when);
    slack(&when).events(vec![mention("2", &when)]).run(&db);
    assert_eq!(prune_events(&db, &day("2026-04-10")), 0);
    assert_eq!(would_show(&db), 2);
}

/// 기준선 이슈를 triage로 유지하고 댓글로 올린다.
fn lifted_baseline(db: &Database) -> i64 {
    github(T0).objects(vec![issue(1)]).run(db);
    let id = actions(db).triage_batch(at(T1)).unwrap().rows[0].id;
    actions(db).triage_decide(&[id], TriageDecision::Keep, at(T1)).unwrap();
    github(T2).events(vec![comment("c1", T2)]).run(db);
    id
}

#[test]
fn r3_기준선에서_올라온_항목도_증거로_닫힌다() {
    let db = test_db();
    let id = lifted_baseline(&db);
    refresh(&db, T2);
    assert!(!item(&db, id).baseline, "올라왔다");
    github(T3).events(vec![closed_event("1", T3)]).run(&db);
    refresh_ok(&db, T3, &["github"]);
    assert_eq!(item(&db, id).state, ItemState::Closed);
    assert_eq!(item(&db, id).close_reason, Some(ItemCloseReason::DoneEvidence));
}

#[test]
fn r3_대조_그_소스가_ok가_아니면_닫지_않는다() {
    let db = test_db();
    let id = lifted_baseline(&db);
    github(T3).events(vec![closed_event("1", T3)]).partial().run(&db);
    refresh(&db, T3);
    assert_eq!(item(&db, id).state, ItemState::Open);
}

fn kept_mention(db: &Database) -> String {
    prime_slack(db);
    let when = kst(MON, "10:30");
    let key = format!("slack:{CHANNEL}:1.1");
    slack(&when).events(vec![mention("1.1", &when)]).run(db);
    refresh(db, &when);
    keep(db, &[1], &when, false);
    let reply = kst(MON, "11:00");
    slack(&reply).events(vec![my_message("1.2", &reply, &key)]).run(db);
    refresh(db, &reply);
    key
}

#[test]
fn r7_남긴_할_일은_원래_이벤트가_닫히고_30일이_지나도_제목과_링크를_잃지_않는다() {
    let db = test_db();
    kept_mention(&db);
    assert_eq!(item(&db, 1).close_reason, Some(ItemCloseReason::DoneEvidence));
    let later = day("2026-04-10");
    prune_items(&db, &later);
    prune_events(&db, &later);
    let shown = list(&db, &later);
    let view = shown.items.iter().find(|row| row.reason == ItemReason::WorkOwed).unwrap();
    assert_eq!(view.title, "합성 멘션 1.1");
    assert_eq!(view.url.as_deref(), Some(format!("https://synthetic.example/archives/{CHANNEL}/p1.1").as_str()));
}

#[test]
fn r7_그_이벤트의_제목도_90일_뒤에는_다른_열린_사건처럼_지운다() {
    let db = test_db();
    let key = kept_mention(&db);
    MaintenanceRepo::new(db.connection()).scrub_stale_open(at(&day("2026-06-15"))).unwrap();
    assert_eq!(event_text(&db, &key, "text_head"), None);
    assert!(event_exists(&db, &key), "행은 남는다");
}

#[test]
fn r8_배포_대기에서_깨어난_기준선_항목은_카드와_건수에_나온다() {
    let db = test_db();
    let linked = NormalizedObject { milestone_title: Some("합성 기능".into()), ..issue(7) };
    github(T0).objects(vec![linked]).run(&db);
    prime(&db, SourceName::Calendar, "mine");
    let moved =
        NormalizedObject { start_date: Some("2026-03-05".into()), end_date: Some("2026-03-06".into()), ..deploy("d1") };
    calendar("mine", T0).objects(vec![moved.clone()]).run(&db);
    let id = actions(&db).triage_batch(at(T0)).unwrap().rows[0].id;
    actions(&db).triage_decide(&[id], TriageDecision::Keep, at(T0)).unwrap();
    assert_eq!(keep(&db, &[id], T0, true), ActionStatus::Changed);
    let later =
        NormalizedObject { start_date: Some("2026-03-20".into()), end_date: Some("2026-03-21".into()), ..moved };
    calendar("mine", T1).objects(vec![later]).run(&db);
    // 원본은 wakeParked를 직접 부른다. 공개 경로에서 대기 시각을 반영하는 것은 응답 만들기다.
    list(&db, &day("2026-03-04"));
    let row = item(&db, id);
    assert_eq!((row.state, row.tier, row.baseline), (ItemState::Open, ItemTier::P1, false));
    let response = next(&db, &kst("2026-03-04", "10:05"));
    assert_eq!(response.counts.open.p1, 1);
    assert_eq!(response.card.unwrap().item_ids, [id]);
}

#[test]
fn r9_partial_실행의_취소는_항목을_닫지_않고_다음_ok_틱에_닫는다() {
    let db = test_db();
    prime(&db, SourceName::Calendar, "mine");
    let object = meeting_at("m1", &format!("{TUE}T15:00:00+09:00"), &format!("{TUE}T16:00:00+09:00"));
    calendar("mine", T0).objects(vec![object.clone()]).run(&db);
    refresh_ok(&db, T0, &["calendar"]);
    let cancelled = NormalizedObject { state: Some("cancelled".into()), ..object };
    calendar("mine", T1).objects(vec![cancelled]).partial().run(&db);
    refresh(&db, T1);
    assert_eq!(item(&db, 1).state, ItemState::Open, "ok가 아닌 실행은 어떤 종료도 만들지 못한다");
    refresh_ok(&db, T2, &["calendar"]);
    assert_eq!(item(&db, 1).state, ItemState::Closed);
    assert_eq!(item(&db, 1).close_reason, Some(ItemCloseReason::Withdrawn));
    assert_eq!(count(&db, "items"), 1);
}

#[test]
fn r14_올라온_뒤_끝낸_기준선_객체는_triage_후보로_돌아오지_않는다() {
    let db = test_db();
    let id = lifted_baseline(&db);
    refresh(&db, T2);
    actions(&db).done(&[id], at(T3), false).unwrap();
    assert!(actions(&db).triage_batch(at(T3)).unwrap().rows.is_empty());
    let month = at(T3).plus_ms(31 * 86_400_000).to_iso();
    prune_items(&db, &month);
    assert!(actions(&db).triage_batch(at(&month)).unwrap().rows.is_empty(), "30일이 지나도 돌아오지 않는다");
}

#[test]
fn r16_외부_앞부분의_링크와_주소는_저장하지_않는다() {
    let url = "https://click.synthetic.example/t/abc123?uid=999&sig=deadbeef";
    let mail = "attendee.synth@outside.example";
    assert_eq!(sanitize_external(&format!("앞 {url} 가운데 {mail} 뒤")), "앞 [링크] 가운데 [주소] 뒤");
    let wrapped = format!("<{url}|눌러> www.synthetic.example/x <mailto:{mail}|{mail}>");
    assert!(!sanitize_external(&wrapped).contains("synthetic.example"));
    let plain = "일반 문장. 버전 1.2.3, 경로 a/b, 10:30 회의";
    assert_eq!(sanitize_external(plain), plain);
    let (_dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    prime_slack(&db);
    let event = NormalizedEvent { text_head: Some(format!("확인 {url} {mail}")), ..mention("1", T0) };
    slack(T0).events(vec![event]).run(&db);
    assert_eq!(event_text(&db, &format!("slack:{CHANNEL}:1"), "text_head").as_deref(), Some("확인 [링크] [주소]"));
    db.connection().execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    let bytes = fs::read(home.db_path()).unwrap();
    let needle = mail.as_bytes();
    assert!(!bytes.windows(needle.len()).any(|window| window == needle), "DB 파일에도 없다");
}

#[test]
fn r16_사용자가_직접_쓴_제목의_링크는_그대로_둔다() {
    let title = "회고 https://synthetic.example/x 보기";
    assert_eq!(sanitize_head(title), title);
}

#[test]
fn r17_쓰는_서비스의_토큰_형식을_가린다() {
    let tokens = [
        format!("github_pat_{}_{}", "A1b2".repeat(10), "c3D4".repeat(10)),
        format!("xapp-1-A0SYNTH-{}-{}", "1".repeat(12), "a1".repeat(16)),
        format!("xoxe.xoxp-1-{}", "Zz9".repeat(12)),
        format!("xoxe-1-{}", "Zz9".repeat(12)),
        "AKIAIOSFODNN7SYNTHET".to_owned(),
        format!("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzeW50aCJ9.{}", "c2lnbmF0dXJl".repeat(2)),
        format!("figd_{}", "AbC1".repeat(10)),
        format!("ntn_{}", "aB3".repeat(14)),
        format!("secret_{}", "aB3".repeat(14)),
    ];
    for token in tokens {
        assert_eq!(sanitize_head(&format!("앞 {token} 뒤")), "앞 [redacted] 뒤", "{}", &token[..8]);
    }
    let plain = "secret_manager 와 ntn_x, figd 는 평문";
    assert_eq!(sanitize_head(plain), plain);
}
