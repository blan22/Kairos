//! 세션 시험(R4~R6, R13, R15, R18~R20).
//! 번호는 .codex/runs/2026-11-04-stage2.md의 표와 같다. 실제 orca, claude를 부르지 않는다.
#![allow(clippy::unwrap_used)]

#[path = "../../kairos-agenda/tests/common/mod.rs"]
mod agenda;
#[path = "../../kairos-sessions/tests/common/mod.rs"]
mod common;

use std::path::Path;

use agenda::github;
use common::*;
use kairos_agenda::{
    CardAction, ItemActions, ItemCloseReason, ItemState, ItemView, ListResponse, NextOptions, NextResponse, Presenter,
    ReadyKind,
};
use kairos_core::KairosHome;
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, SourceName, TrustTier};
use kairos_core::ports::FixedClock;
use kairos_policy::ItemReason;
use kairos_sessions::{
    ClaimRefusal, RunState, SessionClaim, SessionCommand, SessionProfile, SessionRunner, SessionSpawner, SessionStatus,
    SessionWorkspace,
};
use kairos_store::Database;
use serde_json::json;

const RUN_A: &str = RUN;
const RUN_B: &str = "22222222-2222-4333-8444-555555555555";
const RUN_C: &str = "33333333-2222-4333-8444-555555555555";

fn control() -> kairos_agenda::KairosControl {
    control_on().with_sessions(Some(AT))
}

fn next_at(db: &Database, now: kairos_core::Instant, seen: bool) -> NextResponse {
    let options = NextOptions { seen, ack: false };
    Presenter::new(db).next_response(&context(now, &control()), options).unwrap()
}

fn next(db: &Database) -> NextResponse {
    next_at(db, at(AT), false)
}

fn list(db: &Database) -> ListResponse {
    Presenter::new(db).list_response(&context(at(AT), &control()), false).unwrap()
}

fn starts_of(views: &[ItemView]) -> Vec<i64> {
    views.iter().filter(|view| view.actions.iter().any(|action| action.id == "start")).map(|view| view.id).collect()
}

fn has_action(view: &ItemView, id: &str) -> bool {
    view.actions.iter().any(|action: &CardAction| action.id == id)
}

fn runs(db: &Database) -> Vec<kairos_sessions::Run> {
    let ids: Vec<String> = {
        let mut statement = db.connection().prepare("SELECT id FROM runs ORDER BY created_at, rowid").unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect()
    };
    ids.iter().map(|id| kairos_sessions::RunRepo::new(db.connection()).get(id).unwrap().unwrap()).collect()
}

fn claim(db: &Database, run_id: &str) -> SessionClaim {
    SessionRunner::claim(db, run_id, at(AT)).unwrap()
}

fn finish(db: &Database, run_id: &str, exit_code: i32) {
    let found = runs(db).into_iter().find(|run| run.id == run_id).unwrap();
    SessionRunner::finish(db, &found, exit_code, at(AT)).unwrap();
}

fn done(db: &Database, ids: &[i64]) -> Vec<kairos_agenda::ActionResult> {
    ItemActions::new(db).done(ids, at(AT), false).unwrap()
}

fn on_issue(id: &str, tier: TrustTier) -> NormalizedEvent {
    NormalizedEvent { trust_tier: Some(tier), ..agenda::comment(id, AT) }
}

fn push(db: &Database, events: Vec<NormalizedEvent>) {
    github(AT).events(events).run(db);
}

#[test]
fn r4_이미_내려간_옛_run이_끝나도_새_run의_항목을_되돌리지_않는다() {
    let (db, orca) = seeded();
    accept_as(&db, &orca, 1, RUN_A);
    assert!(matches!(claim(&db, RUN_A), SessionClaim::Won { .. }));
    cancel(&db, 1, 0);
    accept_as(&db, &orca, 1, RUN_B);
    finish(&db, RUN_A, 1);
    assert_eq!(item(&db, 1).state, ItemState::Accepted);
    let states: Vec<RunState> = runs(&db).iter().map(|row| row.state).collect();
    assert_eq!(states, [RunState::Failed, RunState::Claimed]);
    assert_eq!(accept_as(&db, &orca, 1, RUN_C).result.status, SessionStatus::Unchanged, "던지지 않는다");
}

fn same_issue_twice() -> Database {
    let db = test_db();
    seed_issue(&db);
    let assigned = NormalizedEvent {
        object_key: Some("gh:acme/widget#1".into()),
        trust_tier: Some(TrustTier::T1),
        ..NormalizedEvent::new("gh:assigned:1", SourceName::Github, EventType::GhAssigned, AT, ActorKind::Human)
    };
    push(&db, vec![assigned]);
    refresh(&db, AT);
    let mut reasons: Vec<ItemReason> = agenda::items(&db).iter().map(|row| row.reason).collect();
    reasons.sort_by_key(|reason| reason.as_str());
    assert_eq!(reasons, [ItemReason::Assigned, ItemReason::ReplyOwed]);
    db
}

#[test]
fn r5_같은_이슈의_항목_둘에_시작_버튼은_하나만_나온다() {
    assert_eq!(starts_of(&list(&same_issue_twice()).items), [1]);
}

#[test]
fn r5_같은_이슈의_다른_항목을_수락하면_세션을_새로_열지_않고_이동으로_보낸다() {
    let db = same_issue_twice();
    let orca = FakeOrca::new();
    accept_as(&db, &orca, 1, RUN_A);
    let second = accept_as(&db, &orca, 2, RUN_B);
    assert_eq!(second.result.status, SessionStatus::Unchanged);
    assert_eq!(second.run.as_ref().map(|run| run.id.as_str()), Some(RUN_A));
    assert_eq!(orca.count("worktree create"), 1);
    assert_eq!(orca.count("terminal create"), 1);
    assert_eq!(runs(&db).len(), 1);
    assert_eq!(item(&db, 2).state, ItemState::Open);
    let shown = list(&db);
    let view = shown.items.iter().find(|row| row.id == 2).unwrap();
    assert_eq!(view.run.as_ref().map(|run| run.id.as_str()), Some(RUN_A));
    assert_eq!(move_to(&db, &orca, 2).result.status, SessionStatus::Changed);
}

/// 파일 DB 위의 담당 이슈. 호출 사이의 다른 프로세스가 두 번째 연결로 끼어든다.
fn file_seeded() -> (tempfile::TempDir, KairosHome, Database) {
    let (dir, home) = temp_home();
    let db = Database::open(&home).unwrap();
    seed_issue(&db);
    (dir, home, db)
}

#[test]
fn r6_worktree를_만드는_사이에_끝내면_터미널을_만들지_않고_그_사실을_답한다() {
    let (_dir, home, db) = file_seeded();
    let orca = FakeOrca::new();
    orca.script("worktree create", move |_| {
        done(&Database::open(&home).unwrap(), &[1]);
        ok(json!({ "worktree": worktree() }))
    });
    let value = accept(&db, &orca);
    assert_eq!(orca.count("terminal create"), 0);
    assert_eq!((value.result.status, value.result.state), (SessionStatus::AlreadyClosed, Some(ItemState::Closed)));
    assert_eq!(item(&db, 1).close_reason, Some(ItemCloseReason::UserDone));
}

#[test]
fn r6_worktree를_만드는_사이에_취소해도_멈추고_launched를_남기지_않는다() {
    let (_dir, home, db) = file_seeded();
    let orca = FakeOrca::new();
    orca.script("worktree create", move |_| {
        cancel(&Database::open(&home).unwrap(), 1, 0);
        ok(json!({ "worktree": worktree() }))
    });
    let value = accept(&db, &orca);
    assert_eq!(orca.count("terminal create"), 0);
    assert_eq!((value.result.status, value.result.state), (SessionStatus::Aborted, Some(ItemState::Open)));
    let row = run(&db);
    assert_eq!((row.state, row.launched_at, row.pane_key), (RunState::Failed, None, None));
}

#[test]
fn r6_reference_find_사이에_취소하면_취소한_run을_되살려_입양하지_않는다() {
    let (_dir, home, db) = file_seeded();
    let orca = FakeOrca::new();
    orca.script("reference find", move |_| {
        cancel(&Database::open(&home).unwrap(), 1, 0);
        let agents = json!([{ "agent": "claude", "liveness": "live", "paneKey": "tab-x:leaf-x", "terminal": "term_x", "linked": true }]);
        ok(json!({ "matches": [{ "workspace": { "id": "repo::/x" }, "agents": agents }] }))
    });
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::Aborted);
    assert_eq!(run(&db).state, RunState::Failed);
}

#[test]
fn r13_항목이_닫혀도_살아_있는_세션의_준비됨_카드와_이동_경로는_남는다() {
    let db = test_db();
    seed_issue(&db);
    let orca = FakeOrca::with("worktree ps", ps_with(&["waiting"]));
    accept(&db, &orca);
    sync(&db, &orca, 0);
    let results = done(&db, &[1]);
    assert!(results[0].message.contains("세션"), "끝낼 때 세션이 살아 있음을 알린다");
    let response = next(&db);
    let ready: Vec<_> = response.ready.iter().map(|card| (card.item_id, card.kind)).collect();
    assert_eq!(ready, [(1, Some(ReadyKind::Question))]);
    assert_eq!(response.runs.active, 1);
    let shown = list(&db);
    let row = shown.items.iter().find(|view| view.id == 1).unwrap();
    assert_eq!(row.run.as_ref().map(|run| run.id.as_str()), Some(RUN_A));
    assert!(row.actions.is_empty(), "닫힌 항목에는 조작이 없고 패널이 이동만 더한다");
    assert_eq!(shown.counts.accepted, 0);
}

#[test]
fn r13_대조_세션이_끝난_닫힌_항목은_목록과_카드에_나오지_않는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    claim(&db, RUN_A);
    done(&db, &[1]);
    finish(&db, RUN_A, 0);
    assert!(next(&db).ready.is_empty());
    assert!(list(&db).items.is_empty());
}

#[test]
fn r15_t3_사람의_댓글이_있는_이슈는_시작을_내지_않고_수락과_kairos_run을_거절한다() {
    let (db, orca) = seeded();
    push(&db, vec![on_issue("t3c", TrustTier::T3)]);
    refresh(&db, AT);
    assert!(starts_of(&list(&db).items).is_empty());
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::NotEligible);
    assert!(orca.calls().is_empty());
}

#[test]
fn r15_수락_뒤에_t3_글이_들어오면_kairos_run이_띄우지_않는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    push(&db, vec![on_issue("t3c", TrustTier::T3)]);
    assert_eq!(claim(&db, RUN_A), SessionClaim::Refused(ClaimRefusal::BadUrl));
}

#[test]
fn r15_정비가_지운_t3_글도_시작을_막는다() {
    let (db, orca) = seeded();
    let old = at(AT).minus_ms(29 * 86_400_000).to_iso();
    push(&db, vec![NormalizedEvent { occurred_at: old, ..on_issue("t3old", TrustTier::T3) }]);
    // 항목을 열지 않은 댓글이다. 정비가 30일 뒤에 지운다.
    db.connection()
        .execute("UPDATE events SET disposition = 'folded' WHERE dedupe_key = 'gh:comment:t3old'", [])
        .unwrap();
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::NotEligible);
    assert_eq!(
        kairos_ops::MaintenanceRepo::new(db.connection()).prune_events(at(AT).plus_ms(2 * 86_400_000)).unwrap(),
        1
    );
    let left: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM events WHERE dedupe_key = 'gh:comment:t3old'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(left, 0);
    refresh(&db, AT);
    assert!(starts_of(&list(&db).items).is_empty());
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::NotEligible);
    assert!(orca.calls().is_empty());
}

#[test]
fn r15_받을_때_이미_30일이_지난_t3_글도_시작을_막는다() {
    let (db, orca) = seeded();
    let old = at(AT).minus_ms(31 * 86_400_000).to_iso();
    let event = NormalizedEvent { occurred_at: old, ..on_issue("t3old", TrustTier::T3) };
    assert_eq!(github(AT).events(vec![event]).run(&db).events_inserted, 0);
    let stored: i64 = db
        .connection()
        .query_row("SELECT count(*) FROM events WHERE dedupe_key = 'gh:comment:t3old'", [], |row| row.get(0))
        .unwrap();
    assert_eq!(stored, 0, "보존 기한 밖의 사건은 여전히 이벤트로 저장하지 않는다");
    refresh(&db, AT);
    assert!(starts_of(&list(&db).items).is_empty());
    assert_eq!(accept(&db, &orca).result.status, SessionStatus::NotEligible);
    assert!(orca.calls().is_empty());
}

#[test]
fn r15_kairos_run이_띄우기_직전에_본_외부_글은_실패_카드가_되고_시작을_다시_내지_않는다() {
    let (_dir, home, db) = file_seeded();
    accept(&db, &FakeOrca::new());
    write_session_token(&home, SYNTHETIC_TOKEN, 0o600);
    let profile = SessionProfile::new(
        Path::new("/synthetic/kairos"),
        Path::new("/synthetic/kairos-home"),
        Path::new("/synthetic/home"),
    )
    .with_workspace(
        SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap(),
    );
    let clock = FixedClock(at(AT));
    let (exits, live) = (Exits(0), FakeLive::new(Ok(kairos_source_github::LiveAuthors::Outsider)));
    let runner = SessionRunner::new(home, Some("/c".to_owned()), &profile, &exits, &live, &clock);
    assert_eq!(runner.run(&[RUN_A.to_owned()]).unwrap(), 1);
    let first = next_at(&db, at(AT), true);
    let cards: Vec<_> = first.ready.iter().map(|card| (card.kind, card.headline.clone())).collect();
    assert_eq!(cards, [(Some(ReadyKind::Failed), Some("외부 사람의 글이 있어 세션을 열지 않았습니다".to_owned()))]);
    assert_eq!(item(&db, 1).state, ItemState::Open);
    assert!(starts_of(&list(&db).items).is_empty(), "되돌린 항목에 시작 버튼을 다시 내지 않는다");
}

#[test]
fn r15_대조_t1_사람의_글과_t2_봇의_글은_시작을_막지_않는다() {
    let (db, _orca) = seeded();
    let bot = NormalizedEvent {
        actor_kind: ActorKind::Bot,
        actor_id: Some("synthetic[bot]".into()),
        ..on_issue("bot", TrustTier::T2)
    };
    push(&db, vec![on_issue("t1c", TrustTier::T1), bot]);
    refresh(&db, AT);
    assert_eq!(starts_of(&list(&db).items), [1]);
}

/// 띄우지 않고 정한 코드로 끝난다.
struct Exits(i32);

impl SessionSpawner for Exits {
    fn spawn(&self, _command: &SessionCommand) -> i32 {
        self.0
    }
}

#[test]
fn r18_세션이_뒤에서_실패하면_실패_카드가_한_번_나오고_묶음에_다시_오른다() {
    let (_dir, home, db) = file_seeded();
    accept(&db, &FakeOrca::new());
    write_session_token(&home, SYNTHETIC_TOKEN, 0o600);
    let profile = SessionProfile::new(
        Path::new("/synthetic/kairos"),
        Path::new("/synthetic/kairos-home"),
        Path::new("/synthetic/home"),
    )
    .with_workspace(
        SessionWorkspace::discover(Path::new("/synthetic/workspace"), Path::new("/synthetic/home")).unwrap(),
    );
    let clock = FixedClock(at(AT));
    let (exits, live) = (Exits(1), FakeLive::insiders());
    let runner = SessionRunner::new(home, Some("/c".to_owned()), &profile, &exits, &live, &clock);
    assert_eq!(runner.run(&[RUN_A.to_owned()]).unwrap(), 1);
    assert_eq!((item(&db, 1).state, item(&db, 1).bundle_queued), (ItemState::Open, true));
    let first = next_at(&db, at(AT), true);
    let ready: Vec<_> = first.ready.iter().map(|card| (card.item_id, card.kind)).collect();
    assert_eq!(ready, [(1, Some(ReadyKind::Failed))]);
    assert!(first.ready[0].headline.as_deref().unwrap_or_default().contains("세션"));
    assert!(next(&db).ready.is_empty(), "1회만 낸다");
    let evening = at("2026-03-02T09:05:00.000Z"); // 18:05 KST
    assert_eq!(next_at(&db, evening, false).card.unwrap().item_ids, [1], "다음 묶음에 다시 나온다");
}

#[test]
fn r18_실패_카드는_표시_게이트가_닫혀_있어도_나온다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    finish(&db, RUN_A, 1);
    let off = control().with_display_mode(kairos_agenda::DisplayMode::Off);
    let response = Presenter::new(&db).next_response(&context(at(AT), &off), NextOptions::default()).unwrap();
    let kinds: Vec<_> = response.ready.iter().map(|card| card.kind).collect();
    assert_eq!(kinds, [Some(ReadyKind::Failed)]);
}

#[test]
fn r18_추적이_내린_실패와_오래된_선점도_실패_카드가_나온다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    sync(&db, &orca, 31_000);
    sync(&db, &orca, 6 * 60_000);
    assert_eq!(runs(&db)[0].state, RunState::Failed);
    let response = next_at(&db, later(6 * 60_000), false);
    let kinds: Vec<_> = response.ready.iter().map(|card| card.kind).collect();
    assert_eq!(kinds, [Some(ReadyKind::Failed)]);
}

#[test]
fn r18_대조_사용자가_취소한_run은_실패_카드를_내지_않는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    cancel(&db, 1, 0);
    assert!(next(&db).ready.is_empty());
}

#[test]
fn r19_끝난_세션의_항목에는_되돌리기가_나오고_다시_시작할_수_있다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    claim(&db, RUN_A);
    finish(&db, RUN_A, 0);
    let shown = list(&db);
    let actions: Vec<(&str, Vec<&str>)> = shown.items[0]
        .actions
        .iter()
        .map(|action| (action.id.as_str(), action.argv.iter().map(String::as_str).collect()))
        .collect();
    // 고치는 세션이라 끝난 뒤 커밋을 PR로 올릴 수 있다.
    assert_eq!(
        actions,
        [
            ("cancel", vec!["accept", "1", "--cancel", "--json"]),
            ("pr", vec!["pr", "1", "--json"]),
            ("done", vec!["done", "1", "--json"])
        ]
    );
    let again = accept_as(&db, &orca, 1, RUN_B);
    assert_eq!(again.result.status, SessionStatus::Unchanged);
    assert!(again.result.message.contains("끝났습니다"));
    cancel(&db, 1, 0);
    assert_eq!(accept_as(&db, &orca, 1, RUN_B).result.status, SessionStatus::Changed);
}

#[test]
fn r19_확인이_필요한_세션에도_되돌리기가_나오고_진행_중_세션에는_나오지_않는다() {
    let (db, orca) = seeded();
    accept(&db, &orca);
    assert!(!has_action(&list(&db).items[0], "cancel"));
    sync(&db, &orca, 31_000);
    assert!(has_action(&list(&db).items[0], "cancel"));
}

#[test]
fn r19_터미널이_없어진_세션으로_이동하면_run을_끝내_되돌리기로_이어진다() {
    let db = test_db();
    seed_issue(&db);
    let orca = FakeOrca::with("worktree ps", ps_with(&["working"]));
    accept(&db, &orca);
    sync(&db, &orca, 0);
    orca.set("terminal list", ok(json!({ "terminals": [] })));
    assert_eq!(move_to(&db, &orca, 1).result.status, SessionStatus::SessionGone);
    assert_eq!(runs(&db)[0].state, RunState::Ended);
    assert!(has_action(&list(&db).items[0], "cancel"));
}

#[test]
fn r20_next_json이_활성_run_수를_낸다() {
    let (db, orca) = seeded();
    assert_eq!(next(&db).runs.active, 0);
    accept(&db, &orca);
    done(&db, &[1]);
    let response = next(&db);
    assert_eq!(response.counts.accepted, 0);
    assert_eq!(response.runs.active, 1, "항목이 닫혀도 추적이 멈추지 않는다");
}
