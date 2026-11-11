//! 중 collect와 doctor를 지나는 시험. 잠금 시험은 .rs에 있다
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use common::fakes::*;
use common::*;
use kairos_agenda::{ControlFile, ItemRepo, KairosControl};
use kairos_core::KairosHome;
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, NormalizedObject, Source, SourceName, SourceResult};
use kairos_core::ports::Notifier;
use kairos_store::{Database, params};
use kairos_tick::{CollectReport, CollectStatus, SleepProbe};
use tempfile::TempDir;

/// 금요일에서 쉬는 월요일을 지나 화요일로 가는 주. 기준 시각 BEFORE 뒤의 날짜다.
const FRI: &str = "2026-03-06";
const OFF_MON: &str = "2026-03-09";
const NEXT_TUE: &str = "2026-03-10";

fn fed(name: SourceName, when: &str, events: Vec<NormalizedEvent>, objects: Vec<NormalizedObject>) -> Arc<dyn Source> {
    let row = SourceResult {
        source: name,
        started_at: when.into(),
        finished_at: when.into(),
        watermark: Some(when.into()),
        events,
        objects,
        overlap_seconds: 0,
        ..result()
    };
    direct_with(name, vec![kairos_core::model::QuerySpec::new("tick", 180, 0)], move |_| vec![row.clone()])
}

#[derive(Default)]
struct Feed {
    slack: Vec<NormalizedEvent>,
    github: Vec<NormalizedEvent>,
    objects: Vec<NormalizedObject>,
}

fn slack_feed(events: Vec<NormalizedEvent>) -> Feed {
    Feed { slack: events, ..Feed::default() }
}

fn github_feed(events: Vec<NormalizedEvent>) -> Feed {
    Feed { github: events, ..Feed::default() }
}

fn beat_with(
    home: &KairosHome,
    when: &str,
    feed: Feed,
    notifier: Arc<dyn Notifier>,
    sleep: Arc<dyn SleepProbe>,
) -> CollectReport {
    let sources = vec![
        fed(SourceName::Slack, when, feed.slack, Vec::new()),
        fed(SourceName::Github, when, feed.github, feed.objects),
    ];
    tick(home, when, sources).notifier(notifier).sleep(sleep).run()
}

fn beat(home: &KairosHome, when: &str, feed: Feed, notifier: Arc<dyn Notifier>) -> CollectReport {
    beat_with(home, when, feed, notifier, frozen(0))
}

fn collect_home(control: &KairosControl) -> (TempDir, KairosHome) {
    let (dir, home) = temp_home();
    ControlFile::new(&home).write(control).unwrap();
    let db = Database::open(&home).unwrap();
    prime_at(&db, SourceName::Slack, "tick", BEFORE);
    prime_at(&db, SourceName::Github, "tick", BEFORE);
    drop(db);
    (dir, home)
}

fn fresh() -> (TempDir, KairosHome, Arc<Recorder>) {
    let (dir, home) = collect_home(&control_on());
    (dir, home, Arc::new(Recorder::new()))
}

fn m(date: &str, time: &str) -> String {
    kst(date, time)
}

fn say(ts: &str, when: &str, channel: &str) -> NormalizedEvent {
    mention_in(ts, when, channel)
}

fn gh_event(key: String, kind: EventType, when: &str, number: i64) -> NormalizedEvent {
    NormalizedEvent {
        dedupe_key: key,
        event_type: kind,
        object_key: Some(format!("gh:acme/widget#{number}")),
        ..comment("x", when)
    }
}

fn rereq(id: &str, when: &str, number: i64) -> NormalizedEvent {
    gh_event(format!("gh:rr:{id}"), EventType::GhReviewRerequested, when, number)
}

fn rereq_gone(id: &str, when: &str, number: i64) -> NormalizedEvent {
    gh_event(format!("gh:rrr:{id}"), EventType::GhReviewRequestRemoved, when, number)
}

fn ci(id: &str, when: &str, kind: EventType) -> NormalizedEvent {
    NormalizedEvent {
        actor_kind: ActorKind::Bot,
        actor_id: Some("checks".into()),
        ..gh_event(format!("gh:ci:abc:{id}"), kind, when, 7)
    }
}

fn own_pr() -> Vec<NormalizedObject> {
    vec![NormalizedObject { head_ref_oid: Some("abc".into()), ..pull_request(7) }]
}

fn with_db<T>(home: &KairosHome, read: impl FnOnce(&Database) -> T) -> T {
    read(&Database::open(home).unwrap())
}

#[test]
fn w1_복귀_뒤_첫_알림에_다음_틱의_공백_안_사건이_나이와_무관하게_묶인다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "11:00"), Feed::default(), recorder.clone());
    let late = slack_feed(vec![say("1", &m(MON, "11:10"), "C1"), say("2", &m(MON, "12:40"), "C2")]);
    beat(&home, &m(MON, "14:00"), late, recorder.clone());
    let next = beat(&home, &m(MON, "14:03"), github_feed(vec![rereq("a", &m(MON, "13:40"), 7)]), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 2건"]);
    assert_eq!(next.merged_p1, 1);
}

#[test]
fn w2_복귀_뒤_첫_알림이_밀린_건이_아니어도_다음_틱의_밀린_건을_묶는다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "11:00"), Feed::default(), recorder.clone());
    beat(&home, &m(MON, "14:00"), slack_feed(vec![say("1", &m(MON, "13:50"), "C1")]), recorder.clone());
    beat(&home, &m(MON, "14:03"), github_feed(vec![rereq("a", &m(MON, "11:30"), 7)]), recorder.clone());
    beat(&home, &m(MON, "14:06"), github_feed(vec![rereq("b", &m(MON, "11:40"), 8)]), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn w3_30분_미만_잠자기라도_소스가_틱에_나뉘면_한_번이다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "15:00"), Feed::default(), recorder.clone());
    beat(&home, &m(MON, "15:25"), slack_feed(vec![say("1", &m(MON, "15:05"), "C1")]), recorder.clone());
    beat(&home, &m(MON, "15:28"), github_feed(vec![rereq("a", &m(MON, "15:10"), 7)]), recorder.clone());
    let both = Feed {
        slack: vec![say("2", &m(MON, "15:15"), "C2")],
        github: vec![rereq("b", &m(MON, "15:12"), 8)],
        ..Feed::default()
    };
    beat(&home, &m(MON, "15:31"), both, recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn 복귀_뒤에_일어난_새_사건은_밀린_건이_아니라_따로_알린다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "11:00"), Feed::default(), recorder.clone());
    beat(&home, &m(MON, "14:00"), slack_feed(vec![say("1", &m(MON, "12:00"), "C1")]), recorder.clone());
    beat(&home, &m(MON, "14:06"), github_feed(vec![rereq("a", &m(MON, "14:05"), 7)]), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건", "GitHub 리뷰 요청 1건"]);
}

#[test]
fn 틱_하나를_건너뛴_공백은_복귀가_아니다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "11:00"), Feed::default(), recorder.clone());
    beat(&home, &m(MON, "11:06"), slack_feed(vec![say("1", &m(MON, "11:05"), "C1")]), recorder.clone());
    beat(&home, &m(MON, "11:09"), github_feed(vec![rereq("a", &m(MON, "11:02"), 7)]), recorder.clone());
    assert_eq!(recorder.messages().len(), 2);
}

#[test]
fn r4_근무일_아침의_첫_틱은_복귀가_아니라_근무_전_건이_다른_소스의_새_건을_먹지_않는다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "19:57"), Feed::default(), recorder.clone());
    beat(&home, &m(TUE, "09:00"), slack_feed(vec![say("1", &m(MON, "22:00"), "C1")]), recorder.clone());
    for minutes in (3..=60).step_by(3) {
        beat(&home, &plus_minutes(&m(TUE, "09:00"), minutes), Feed::default(), recorder.clone());
    }
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    beat(&home, &m(TUE, "10:03"), github_feed(vec![rereq("a", &m(TUE, "09:10"), 7)]), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건", "GitHub 리뷰 요청 1건"]);
}

#[test]
fn 전날_낮부터_잠들어_다음_날_근무_중에_깨어나면_복귀다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "15:00"), Feed::default(), recorder.clone());
    beat(&home, &m(TUE, "11:00"), slack_feed(vec![say("1", &m(MON, "16:00"), "C1")]), recorder.clone());
    beat(&home, &m(TUE, "11:03"), github_feed(vec![rereq("a", &m(TUE, "10:30"), 7)]), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn w6_틱이_잠자기에_얼어붙으면_그_틱은_알리지_않고_다음_틱이_그때의_시각으로_판단한다() {
    let (_dir, home, recorder) = fresh();
    let feed = slack_feed(vec![say("1", &m(MON, "11:57"), "C1")]);
    let report = beat_with(&home, &m(MON, "11:58"), feed, recorder.clone(), frozen(42 * 60_000));
    assert_eq!((report.status, report.slept), (CollectStatus::Ran, true));
    assert!(recorder.messages().is_empty());
    assert_eq!(with_db(&home, |db| item(db, 1).p1_notified_at), None, "표시를 남기지 않아 항목이 사라지지 않는다");
    beat(&home, &m(MON, "12:43"), Feed::default(), recorder.clone());
    assert!(recorder.messages().is_empty(), "깨어난 시각은 점심이다");
    beat(&home, &m(MON, "13:30"), Feed::default(), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    let shown: String = with_db(&home, |db| {
        db.connection()
            .query_row("SELECT shown_at FROM suggestions WHERE surface = ?", params!["osascript"], |row| row.get(0))
            .unwrap()
    });
    assert_eq!(shown, m(MON, "13:30"));
}

#[test]
fn w7_얼어붙은_틱의_건과_깨어난_뒤의_같은_채널_건은_한_번이다() {
    let (_dir, home, recorder) = fresh();
    let feed = slack_feed(vec![say("1", &m(MON, "13:59"), "C1")]);
    beat_with(&home, &m(MON, "14:00"), feed, recorder.clone(), frozen(2 * 3_600_000));
    beat(&home, &m(MON, "16:03"), slack_feed(vec![say("2", &m(MON, "15:50"), "C1")]), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 2건"]);
}

#[test]
fn 얼어붙은_틱도_복귀다_다음_틱의_알림_뒤로_공백_안_사건을_묶는다() {
    let (_dir, home, recorder) = fresh();
    let feed = slack_feed(vec![say("1", &m(MON, "13:59"), "C1")]);
    beat_with(&home, &m(MON, "14:00"), feed, recorder.clone(), frozen(60_000));
    beat(&home, &m(MON, "14:03"), Feed::default(), recorder.clone());
    beat(&home, &m(MON, "14:06"), github_feed(vec![rereq("a", &m(MON, "13:50"), 7)]), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn r1_같은_pr의_ci가_깜빡여도_30분_안에는_한_번이고_구간이_지나면_다시_알린다() {
    let (_dir, home, recorder) = fresh();
    let failed = EventType::GhCiFailed;
    let recovered = EventType::GhCiRecovered;
    let flips = [
        ("10:00", failed),
        ("10:06", recovered),
        ("10:09", failed),
        ("10:15", recovered),
        ("10:18", failed),
        ("10:24", recovered),
        ("10:27", failed),
    ];
    let ci_feed = |id: String, when: &str, kind: EventType| Feed {
        github: vec![ci(&id, &m(MON, when), kind)],
        objects: own_pr(),
        ..Feed::default()
    };
    for (index, (when, kind)) in flips.iter().enumerate() {
        beat(&home, &m(MON, when), ci_feed(format!("e{index}"), when, *kind), recorder.clone());
    }
    assert_eq!(recorder.messages(), ["GitHub CI 실패 1건"]);
    assert!(with_db(&home, |db| count(db, "items")) >= 4, "깜빡임은 항목을 새로 연다");
    beat(&home, &m(MON, "10:33"), ci_feed("e8".into(), "10:33", recovered), recorder.clone());
    beat(&home, &m(MON, "10:36"), ci_feed("e9".into(), "10:36", failed), recorder.clone());
    assert_eq!(recorder.messages().len(), 2);
}

#[test]
fn r2_같은_pr의_리뷰_재요청_철회_재요청은_한_번이다() {
    let (_dir, home, recorder) = fresh();
    beat(&home, &m(MON, "10:00"), github_feed(vec![rereq("a", &m(MON, "09:59"), 9)]), recorder.clone());
    beat(&home, &m(MON, "10:03"), github_feed(vec![rereq_gone("a", &m(MON, "10:02"), 9)]), recorder.clone());
    beat(&home, &m(MON, "10:06"), github_feed(vec![rereq("b", &m(MON, "10:05"), 9)]), recorder.clone());
    assert_eq!(recorder.messages(), ["GitHub 리뷰 요청 1건"]);
}

#[test]
fn r5_사용자가_끝을_누른_pr에_같은_커밋의_다른_체크_실패가_와도_30분_안이면_묶는다() {
    let (_dir, home, recorder) = fresh();
    let feed = |id: &str, when: &str| Feed {
        github: vec![ci(id, &m(MON, when), EventType::GhCiFailed)],
        objects: own_pr(),
        ..Feed::default()
    };
    beat(&home, &m(MON, "10:00"), feed("f1", "09:59"), recorder.clone());
    with_db(&home, |db| actions(db).done(&[1], at(&m(MON, "10:01")), false).unwrap());
    beat(&home, &m(MON, "10:03"), feed("f2", "10:02"), recorder.clone());
    assert_eq!(recorder.messages(), ["GitHub CI 실패 1건"]);
}

fn slack_stub() -> Vec<Arc<dyn Source>> {
    vec![fed(SourceName::Slack, T1, Vec::new(), Vec::new())]
}

#[test]
fn s1_실제_collect_실패_뒤_일시정지_동안_doctor에_미전달이_남고_해제_뒤_알린다() {
    let (_dir, home) = collect_home(&control_on());
    let file = ControlFile::new(&home);
    let feed = slack_feed(vec![mention("1", &m(MON, "11:00"))]);
    beat(&home, &m(MON, "11:00"), feed, Arc::new(Recorder::failing()));
    file.write(&control_on().with_paused_until(Some(&m(MON, "11:05")))).unwrap();
    beat(&home, &m(MON, "11:03"), Feed::default(), Arc::new(Recorder::new()));
    let during = diagnose(&home, &m(MON, "11:03"), slack_stub());
    assert!(during.problems.iter().any(|line| line == "P1 알림 미전달 1건"), "{:?}", during.problems);
    file.write(&control_on()).unwrap();
    let recorder = Arc::new(Recorder::new());
    beat(&home, &m(MON, "11:06"), Feed::default(), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn r3_금요일_저녁_보류_건은_월요일이_쉬는_날이어도_다음_근무일에_알린다() {
    let (_dir, home) = collect_home(&control_on().with_off_dates(&[OFF_MON.to_owned()]));
    let recorder = Arc::new(Recorder::new());
    beat(&home, &m(FRI, "19:45"), slack_feed(vec![mention("1", &m(FRI, "19:45"))]), recorder.clone());
    beat(&home, &m(OFF_MON, "10:03"), Feed::default(), recorder.clone());
    assert!(recorder.messages().is_empty());
    let held = with_db(&home, |db| ItemRepo::new(db.connection()).counts(at(&m(OFF_MON, "10:03"))).unwrap().p1_held);
    assert_eq!(held, 1);
    beat(&home, &m(NEXT_TUE, "10:03"), Feed::default(), recorder.clone());
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn 재시도가_끝난_건은_doctor에_따로_나온다() {
    let (_dir, home) = collect_home(&control_on());
    let failing = Arc::new(Recorder::failing());
    beat(&home, &m(MON, "10:00"), slack_feed(vec![mention("1", &m(MON, "10:00"))]), failing.clone());
    for when in ["10:03", "10:06", "10:36", "11:06"] {
        beat(&home, &m(MON, when), Feed::default(), failing.clone());
    }
    let doctor = diagnose(&home, &m(MON, "11:06"), slack_stub());
    assert!(doctor.problems.iter().any(|line| line == "P1 알림 미전달 1건"), "{:?}", doctor.problems);
    let line = "P1 알림 재시도 끝남 1건: 항목은 패널에 있습니다";
    assert!(doctor.lines.iter().any(|row| row == line), "{:?}", doctor.lines);
}

fn counts(report: &CollectReport) -> [usize; 4] {
    [report.notified_p1, report.merged_p1, report.dropped_p1, report.failed_p1]
}

#[test]
fn 묶음과_버림이_수집_보고와_doctor에_남고_항목_표시로_이유를_가른다() {
    let (_dir, home, recorder) = fresh();
    let file = ControlFile::new(&home);
    let when = m(MON, "11:00");
    let first = beat(&home, &when, slack_feed(vec![say("1", &when, "C1")]), recorder.clone());
    assert_eq!(counts(&first), [1, 0, 0, 0]);
    let second = beat(&home, &m(MON, "11:03"), slack_feed(vec![say("2", &m(MON, "11:03"), "C1")]), recorder.clone());
    assert_eq!(counts(&second), [0, 1, 0, 0]);
    file.write(&control_on().with_paused_until(Some("indefinite"))).unwrap();
    let third = beat(&home, &m(MON, "11:06"), slack_feed(vec![say("3", &m(MON, "11:06"), "C2")]), recorder.clone());
    assert_eq!(counts(&third), [0, 0, 1, 0]);
    file.write(&control_on()).unwrap();
    let feed = slack_feed(vec![say("4", &m(MON, "11:09"), "C3")]);
    let fourth = beat(&home, &m(MON, "11:09"), feed, Arc::new(Recorder::failing()));
    assert_eq!(counts(&fourth), [0, 0, 0, 1]);
    let notices = with_db(&home, |db| [1, 2, 3].map(|id| item(db, id).p1_notice.map(|notice| notice.as_str())));
    assert_eq!(notices, [Some("sent"), Some("merged"), Some("dropped")]);
    with_db(&home, |db| actions(db).done(&[4], at(&m(MON, "11:10")), false).unwrap());
    assert_eq!(with_db(&home, |db| item(db, 4).p1_notice.map(|notice| notice.as_str())), Some("handled"));
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    let doctor = diagnose(&home, &m(MON, "11:10"), slack_stub());
    let line = "P1 알림 오늘: 전달 1건, 묶음 1건, 미발송 1건";
    assert!(doctor.lines.iter().any(|row| row == line), "{:?}", doctor.lines);
}

#[test]
fn 얼어붙은_틱이_다음_틱으로_미룬_건은_그사이_끈_설정을_따른다() {
    let (_dir, home, recorder) = fresh();
    let feed = slack_feed(vec![mention("1", &m(MON, "11:00"))]);
    beat_with(&home, &m(MON, "11:00"), feed, recorder.clone(), frozen(60_000));
    let file = ControlFile::new(&home);
    file.write(&file.read().with_p1_notify(false)).unwrap();
    beat(&home, &m(MON, "11:03"), Feed::default(), recorder.clone());
    assert!(recorder.messages().is_empty());
}
