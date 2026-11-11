//! collect, doctor, CLI를 쓰는 시험은 3단계로 넘겼다
#![allow(clippy::unwrap_used)]

mod common;

use common::*;
use kairos_agenda::{
    ActionContext, ControlFile, DisplayMode, FocusTracker, GateReason, ItemRepo, ItemState, ItemTier, KairosControl,
    NextOptions, P1_CATCHUP_TITLE, P1_TITLE, P1Alerts, P1Line,
};
use kairos_core::model::{EventType, NormalizedEvent, SourceName};
use kairos_core::ports::AlertTier;
use kairos_policy::ItemReason;
use kairos_store::Database;

fn seeded() -> Database {
    let db = test_db();
    prime_slack(&db);
    prime(&db, SourceName::Github, "tick");
    db
}

fn review_event(id: &str, when: &str, event_type: EventType) -> NormalizedEvent {
    NormalizedEvent {
        dedupe_key: format!("gh:rr:{id}"),
        event_type,
        object_key: Some("gh:acme/widget#7".into()),
        ..comment(id, when)
    }
}

fn counts(db: &Database, now: &str) -> kairos_agenda::ItemCounts {
    ItemRepo::new(db.connection()).counts(at(now)).unwrap()
}

fn seen_card(db: &Database, now: &str) -> Option<ItemTier> {
    let response = next_with(db, &context(now, &control_on(), Over::default()), NextOptions { seen: true, ack: false });
    response.card.map(|card| card.tier)
}

fn line(source: &str, rule_id: Option<&str>, reason: ItemReason) -> String {
    let row = P1Line { source: Some(source.into()), reason, rule_id: rule_id.map(str::to_owned) };
    P1Alerts::message(&[row])
}

#[test]
fn 한_틱의_여러_건은_한_번의_알림에_소스별_건수로_합친다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    say(&db, &when, "1", "C1");
    say(&db, &when, "2", "C2");
    github(&when).events(vec![review_event("a", &when, EventType::GhReviewRerequested)]).run(&db);
    let recorder = Recorder::new();
    assert_eq!(tick(&db, &when, &recorder), report(3, 0, 0, 0));
    assert_eq!(recorder.messages(), ["GitHub 리뷰 요청 1건, Slack 멘션 2건"]);
    let alert = &recorder.alerts()[0];
    assert_eq!((alert.tier, alert.title.as_str()), (AlertTier::P1, P1_TITLE));
    let mut items = alert.items.clone();
    items.sort_unstable();
    let mut statement = db.connection().prepare("SELECT id FROM items WHERE tier = 'P1' ORDER BY id").unwrap();
    let sent: Vec<i64> = statement.query_map([], |row| row.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!((items.len(), items), (3, sent), "보낸 항목의 ID를 싣는다");
}

#[test]
fn 문구는_고정이고_제목_이름_채널_이름_같은_외부_문자열이_들어가지_않는다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    let event = NormalizedEvent {
        text_head: Some("$(비밀 제목) \"; do shell script".into()),
        actor_id: Some("비밀이름".into()),
        permalink: Some("https://synthetic.example/비밀".into()),
        ..mention_in("1", &when, "C비밀채널")
    };
    slack(&when).events(vec![event]).run(&db);
    let recorder = Recorder::new();
    tick(&db, &when, &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    assert_eq!(line("외부 문자열", Some("외부 규칙"), ItemReason::ReplyOwed), "kairos 답장 필요 1건");
}

#[test]
fn 규칙별_문구_답변_대기_변경_요청_ci_실패_메일_규칙_없는_항목() {
    assert_eq!(line("github", Some("p1.deploy_owned_comment"), ItemReason::ReplyOwed), "GitHub 답변 대기 1건");
    assert_eq!(line("github", Some("p1.changes_requested"), ItemReason::MyPrAttention), "GitHub 변경 요청 1건");
    assert_eq!(line("github", Some("p1.ci_failed"), ItemReason::MyPrAttention), "GitHub CI 실패 1건");
    assert_eq!(line("notion", Some("p1.notion_mention"), ItemReason::WorkOwed), "Notion 멘션 1건");
    assert_eq!(line("gmail", Some("p1.sentry_mail"), ItemReason::WorkOwed), "Gmail Sentry 메일 1건");
    assert_eq!(line("gmail", Some("p1.app_review_mail"), ItemReason::WorkOwed), "Gmail App Review 메일 1건");
    assert_eq!(line("calendar", Some("p1.deploy_d1"), ItemReason::DeployRisk), "Calendar 배포 위험 1건");
    assert_eq!(
        line("github", None, ItemReason::Assigned),
        "GitHub 담당 이슈 1건",
        "배포 때 다시로 깨어난 항목에는 P1 사건이 없다"
    );
}

#[test]
fn 각_항목은_한_번만_알리고_같은_항목에_사건이_더_붙어도_다시_울리지_않는다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    let thread = format!("slack:{CHANNEL}:root");
    let in_thread = |ts: &str, occurred: &str| NormalizedEvent {
        thread_key: Some(thread.clone()),
        channel_id: None,
        ..mention(ts, occurred)
    };
    slack(&when).events(vec![in_thread("1", &when)]).run(&db);
    let recorder = Recorder::new();
    tick(&db, &when, &recorder);
    assert_eq!(recorder.messages().len(), 1);
    assert!(item(&db, 1).p1_notified_at.is_some());
    let later = kst(MON, "11:00");
    slack(&later).events(vec![in_thread("2", &later)]).run(&db);
    assert_eq!(tick(&db, &later, &recorder), report(0, 0, 0, 0));
    assert_eq!(recorder.messages().len(), 1);
    assert_eq!(count(&db, "items"), 1);
}

#[test]
fn 같은_채널의_30분_안_알림은_한_번이고_구간은_알림_시각에서_늘어나지_않는다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:00"), "1", CHANNEL);
    tick(&db, &kst(MON, "10:00"), &recorder);
    say(&db, &kst(MON, "10:12"), "2", CHANNEL);
    assert_eq!(tick(&db, &kst(MON, "10:12"), &recorder), report(0, 1, 0, 0));
    say(&db, &kst(MON, "10:30"), "3", CHANNEL);
    assert_eq!(tick(&db, &kst(MON, "10:30"), &recorder), report(0, 1, 0, 0), "30분째까지 같은 구간이다");
    assert_eq!(recorder.messages().len(), 1);
    assert!(item(&db, 2).p1_notified_at.is_some(), "합쳐진 건은 다시 울리지 않는다");
    say(&db, &kst(MON, "10:33"), "4", CHANNEL);
    assert_eq!(tick(&db, &kst(MON, "10:33"), &recorder), report(1, 0, 0, 0), "합쳐진 건이 구간을 늘리지 않는다");
    assert_eq!(recorder.messages(), ["Slack 멘션 1건", "Slack 멘션 1건"]);
}

#[test]
fn 다른_채널과_채널이_없는_소스의_다른_객체는_30분_합치기에_걸리지_않는다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:00"), "1", "C1");
    tick(&db, &kst(MON, "10:00"), &recorder);
    say(&db, &kst(MON, "10:05"), "2", "C2");
    assert_eq!(tick(&db, &kst(MON, "10:05"), &recorder).sent, 1);
    for (id, when) in [("a", kst(MON, "10:10")), ("b", kst(MON, "10:15"))] {
        let event = NormalizedEvent {
            object_key: Some(format!("gh:acme/widget#{id}")),
            ..review_event(id, &when, EventType::GhReviewRerequested)
        };
        github(&when).events(vec![event]).run(&db);
        assert_eq!(tick(&db, &when, &recorder).sent, 1, "{id}");
    }
    assert_eq!(recorder.messages().len(), 4);
}

#[test]
fn 밀린_수집은_틱이_나뉘어도_한_번으로_합치고_새_사건은_따로_알린다() {
    let db = seeded();
    let recorder = Recorder::new();
    let woke = kst(MON, "15:00");
    let backlog = kst(MON, "11:00");
    for (ts, channel) in [("1", "C1"), ("2", "C2"), ("3", "C3")] {
        say_in(&db, &woke, ts, channel, &backlog);
    }
    assert_eq!(tick_resumed(&db, &woke, &recorder, Over::default(), Some(&woke)).sent, 3);
    assert_eq!(recorder.messages(), ["Slack 멘션 3건"]);
    let alert = &recorder.alerts()[0];
    assert_eq!((alert.title.as_str(), alert.items.len()), (P1_CATCHUP_TITLE, 3));
    let next = kst(MON, "15:03");
    say_in(&db, &next, "4", "C4", &backlog);
    assert_eq!(
        tick_resumed(&db, &next, &recorder, Over::default(), Some(&woke)),
        report(0, 1, 0, 0),
        "쪽 상한으로 다음 틱에 온 밀린 건"
    );
    let fresh = kst(MON, "15:06");
    say(&db, &fresh, "5", "C5");
    assert_eq!(
        tick_resumed(&db, &fresh, &recorder, Over::default(), Some(&woke)).sent,
        1,
        "새 사건은 밀린 건이 아니다"
    );
    assert_eq!(recorder.messages().len(), 2);
}

#[test]
fn 기준선_항목은_알리지_않고_사람의_새_사건으로_올라오면_알린다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    db.connection().execute_batch(&format!(
        "INSERT INTO items (object_key, reason, tier, baseline, source, opened_at) VALUES ('gh:acme/widget#7', 'review_owed', 'P2', 1, 'github', '{BEFORE}');
         INSERT INTO items (object_key, reason, tier, baseline, source, opened_at) VALUES ('gh:acme/widget#8', 'review_owed', 'P1', 1, 'github', '{BEFORE}');"
    ))
    .unwrap();
    let recorder = Recorder::new();
    tick(&db, &when, &recorder);
    assert!(recorder.messages().is_empty());
    github(&when).events(vec![review_event("a", &when, EventType::GhReviewRerequested)]).run(&db);
    tick(&db, &when, &recorder);
    assert_eq!(recorder.messages(), ["GitHub 리뷰 요청 1건"]);
    assert_eq!(item(&db, 2).p1_notified_at, None, "기준선 표시를 건드리지 않는다");
}

#[test]
fn p2_항목이_p1로_오르면_새_p1이다_올라온_항목도_한_번만_알린다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    github(&when).events(vec![review_event("a", &when, EventType::GhReviewRequested)]).run(&db);
    let recorder = Recorder::new();
    tick(&db, &when, &recorder);
    assert!(recorder.messages().is_empty(), "P2는 알리지 않는다");
    let later = kst(MON, "11:00");
    github(&later).events(vec![review_event("b", &later, EventType::GhReviewRerequested)]).run(&db);
    tick(&db, &later, &recorder);
    tick(&db, &kst(MON, "11:03"), &recorder);
    assert_eq!(recorder.messages(), ["GitHub 리뷰 요청 1건"]);
    assert_eq!(item(&db, 1).tier, ItemTier::P1);
}

#[test]
fn 나중에로_보낸_p2가_대기_중_p1로_오르면_대기_동안은_조용하고_깨어난_뒤_알린다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    github(&when).events(vec![review_event("a", &when, EventType::GhReviewRequested)]).run(&db);
    refresh(&db, &when);
    let none = none();
    actions(&db).later(&[1], &ActionContext { now: at(&when), off_dates: &none }).unwrap();
    assert_eq!(item(&db, 1).p1_notified_at, None, "P2에는 표시를 남기지 않는다");
    let up = kst(MON, "11:00");
    github(&up).events(vec![review_event("b", &up, EventType::GhReviewRerequested)]).run(&db);
    let recorder = Recorder::new();
    tick(&db, &up, &recorder);
    assert_eq!((item(&db, 1).state, item(&db, 1).tier), (ItemState::Parked, ItemTier::P1));
    assert!(recorder.messages().is_empty());
    tick(&db, &kst(MON, "13:33"), &recorder);
    assert_eq!(item(&db, 1).state, ItemState::Open);
    assert_eq!(recorder.messages(), ["GitHub 리뷰 요청 1건"]);
}

#[test]
fn 수락한_항목은_p1로_올라도_알리지_않는다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    github(&when).events(vec![review_event("a", &when, EventType::GhReviewRequested)]).run(&db);
    refresh(&db, &when);
    db.connection().execute_batch("UPDATE items SET state = 'accepted'").unwrap();
    let up = kst(MON, "11:00");
    github(&up).events(vec![review_event("b", &up, EventType::GhReviewRerequested)]).run(&db);
    let recorder = Recorder::new();
    tick(&db, &up, &recorder);
    assert_eq!(item(&db, 1).tier, ItemTier::P1);
    assert!(recorder.messages().is_empty());
}

#[test]
fn 표시_게이트가_닫혀_있거나_일시정지_쉬는_날에_잡힌_건은_다시_열려도_울리지_않는다() {
    let cases = [
        ("표시 꺼짐", Over::control(control_on().with_display_mode(DisplayMode::Off))),
        ("일시정지", Over::control(control_on().with_paused_until(Some("indefinite")))),
        ("쉬는 날", Over::off_dates(off(&[MON]))),
        ("P1 알림 끔", Over::control(control_on().with_p1_notify(false))),
    ];
    for (name, over) in cases {
        let db = seeded();
        let recorder = Recorder::new();
        let when = kst(MON, "11:00");
        say(&db, &when, "1", CHANNEL);
        assert_eq!(tick_over(&db, &when, &recorder, over), report(0, 0, 1, 0), "{name}");
        assert_eq!(tick(&db, &kst(TUE, "11:00"), &recorder), report(0, 0, 0, 0), "{name}");
        assert!(recorder.messages().is_empty(), "{name}");
        assert_eq!(counts(&db, &when).p1_undelivered, 0, "{name}");
        assert_eq!(counts(&db, &when).open.p1, 1, "{name}: 항목은 패널에 남는다");
    }
}

#[test]
fn 기록_기간의_게이트도_알리지_않는다() {
    let db = seeded();
    let when = kst(MON, "11:00");
    say(&db, &when, "1", CHANNEL);
    let auto = KairosControl::default();
    let gate = gate_at(Some(&kst(MON, "09:00")), &auto, &when, &none());
    assert_eq!((gate.open, gate.reason), (false, Some(GateReason::Recording)));
    let recorder = Recorder::new();
    tick_over(&db, &when, &recorder, Over { control: Some(auto), gate: Some(gate), ..Over::default() });
    assert!(recorder.messages().is_empty());
}

#[test]
fn 포커스_중에는_쌓았다가_끝난_뒤_첫_틱에_한_번에_알린다() {
    let db = seeded();
    let focused = Over::control(FocusTracker::start(control_on(), 50, at(&kst(MON, "10:00"))));
    let recorder = Recorder::new();
    for (ts, time, channel) in [("1", "10:03", "C1"), ("2", "10:20", "C2"), ("3", "10:45", "C3")] {
        say(&db, &kst(MON, time), ts, channel);
        assert_eq!(tick_over(&db, &kst(MON, time), &recorder, focused.clone()), report(0, 0, 0, 0), "{time}");
    }
    assert!(recorder.messages().is_empty());
    assert_eq!(counts(&db, &kst(MON, "10:45")).p1_held, 3);
    assert_eq!(counts(&db, &kst(MON, "10:45")).p1_undelivered, 0, "보류는 미전달이 아니다");
    assert_eq!(tick_over(&db, &kst(MON, "10:51"), &recorder, focused.clone()).sent, 3);
    assert_eq!(recorder.messages(), ["Slack 멘션 3건"]);
    assert_eq!(tick_over(&db, &kst(MON, "10:54"), &recorder, focused).sent, 0);
}

#[test]
fn 포커스를_일찍_끝내면_다음_틱에_알린다() {
    let db = seeded();
    let focus = FocusTracker::start(control_on(), 50, at(&kst(MON, "10:00")));
    say(&db, &kst(MON, "10:03"), "1", CHANNEL);
    let recorder = Recorder::new();
    tick_over(&db, &kst(MON, "10:03"), &recorder, Over::control(focus.clone()));
    assert!(recorder.messages().is_empty());
    let ended = FocusTracker::end(focus, at(&kst(MON, "10:05")));
    tick_over(&db, &kst(MON, "10:06"), &recorder, Over::control(ended));
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn 점심_시간은_포커스와_같이_보류했다가_끝난_뒤_한_번에_알린다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "12:10"), "1", "C1");
    say(&db, &kst(MON, "13:20"), "2", "C2");
    tick(&db, &kst(MON, "12:10"), &recorder);
    tick(&db, &kst(MON, "13:27"), &recorder);
    assert!(recorder.messages().is_empty());
    tick(&db, &kst(MON, "13:30"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 2건"]);
}

#[test]
fn 근무_시간_밖은_다음_근무_시작까지_보류한다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "19:45"), "1", CHANNEL);
    tick(&db, &kst(MON, "19:45"), &recorder);
    tick(&db, &kst(TUE, "09:30"), &recorder);
    assert!(recorder.messages().is_empty());
    tick(&db, &kst(TUE, "10:00"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn 보류_중_사용자가_처리한_항목은_끝나도_알리지_않는다() {
    let db = seeded();
    let hold = kst(MON, "12:10");
    for ts in ["1", "2", "3", "4"] {
        say(&db, &hold, ts, &format!("C{ts}"));
    }
    let recorder = Recorder::new();
    tick(&db, &hold, &recorder);
    let none = none();
    let acting = ActionContext { now: at(&hold), off_dates: &none };
    actions(&db).done(&[1], at(&hold), false).unwrap();
    actions(&db).later(&[2], &acting).unwrap();
    actions(&db).keep(&[3], &acting, false).unwrap();
    tick(&db, &kst(MON, "13:30"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"], "나중에로 깨어나도 다시 세지 않는다");
    tick(&db, &kst(MON, "18:30"), &recorder);
    assert_eq!(recorder.messages().len(), 1);
}

#[test]
fn 양성_증거로_닫힌_항목은_보류가_끝나도_알리지_않는다() {
    let db = seeded();
    say(&db, &kst(MON, "12:10"), "1", CHANNEL);
    let recorder = Recorder::new();
    tick(&db, &kst(MON, "12:10"), &recorder);
    db.connection()
        .execute_batch("UPDATE items SET state = 'closed', closed_at = 'x', close_reason = 'expired'")
        .unwrap();
    tick(&db, &kst(MON, "13:30"), &recorder);
    assert!(recorder.messages().is_empty());
}

#[test]
fn 패널에서_이미_본_p1은_늦게_울리지_않는다_본_p2가_나중에_p1로_오르면_알린다() {
    let db = seeded();
    let hold = kst(MON, "13:20");
    say(&db, &hold, "1", CHANNEL);
    let recorder = Recorder::new();
    tick(&db, &hold, &recorder);
    assert_eq!(seen_card(&db, &kst(MON, "13:31")), Some(ItemTier::P1));
    tick(&db, &kst(MON, "13:33"), &recorder);
    assert!(recorder.messages().is_empty());

    let when = kst(MON, "14:00");
    github(&when).events(vec![review_event("a", &when, EventType::GhReviewRequested)]).run(&db);
    refresh(&db, &when);
    db.connection().execute_batch(&format!("UPDATE items SET first_surfaced_at = '{when}' WHERE id = 2")).unwrap();
    let up = kst(MON, "14:30");
    github(&up).events(vec![review_event("b", &up, EventType::GhReviewRerequested)]).run(&db);
    tick(&db, &up, &recorder);
    assert_eq!(recorder.messages(), ["GitHub 리뷰 요청 1건"]);
}

#[test]
fn 전달에_실패하면_표시를_남기지_않고_미전달로_드러낸_뒤_다음_틱에_다시_알린다() {
    let db = seeded();
    let when = kst(MON, "10:03");
    say(&db, &when, "1", CHANNEL);
    let failing = Recorder::failing();
    assert_eq!(tick(&db, &when, &failing), report(0, 0, 0, 1));
    assert_eq!(item(&db, 1).p1_notified_at, None);
    assert_eq!(counts(&db, &when).p1_undelivered, 1);
    assert_eq!(count(&db, "suggestions"), 0);
    // 원본은 여기서 던지는 Notifier를 쓴다. Notifier가 bool만 돌려주므로 실패와 같은 경로다.
    assert_eq!(tick(&db, &kst(MON, "10:06"), &failing), report(0, 0, 0, 1));
    let working = Recorder::new();
    assert_eq!(tick(&db, &kst(MON, "10:09"), &working).sent, 1);
    assert_eq!(counts(&db, &kst(MON, "10:09")).p1_undelivered, 0);
    assert_eq!(tick(&db, &kst(MON, "10:12"), &working).sent, 0);
    assert_eq!(working.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn 실패한_알림은_30분_구간을_만들지_않는다() {
    let db = seeded();
    say(&db, &kst(MON, "10:03"), "1", CHANNEL);
    tick(&db, &kst(MON, "10:03"), &Recorder::failing());
    say(&db, &kst(MON, "10:06"), "2", CHANNEL);
    let recorder = Recorder::new();
    tick(&db, &kst(MON, "10:06"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 2건"]);
}

#[test]
fn mac_알림은_시간당_1건_제한을_받지_않고_지금_카드_예산도_쓰지_않는다() {
    let db = seeded();
    let recorder = Recorder::new();
    for (ts, time) in [("1", "10:00"), ("2", "10:03"), ("3", "10:06"), ("4", "10:09"), ("5", "10:12"), ("6", "10:15")] {
        say(&db, &kst(MON, time), ts, &format!("C{ts}"));
        tick(&db, &kst(MON, time), &recorder);
    }
    assert_eq!(recorder.messages().len(), 6);
    let view = next_with(&db, &context(&kst(MON, "10:15"), &control_on(), Over::default()), NextOptions::default());
    assert_eq!((view.budget.p1_hour_used, view.budget.p1_day_used, view.budget.shown_today), (0, 0, 0));
    assert!(!view.card.unwrap().pending);
    assert_eq!(item(&db, 1).first_surfaced_at, None, "알림 전달은 사용자가 봤다는 증거가 아니다");
    let mut statement = db.connection().prepare("SELECT DISTINCT kind, tier, surface, seen FROM suggestions").unwrap();
    let rows: Vec<(String, String, String, i64)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(rows, [("item".to_owned(), "P1".to_owned(), "osascript".to_owned(), 0)]);
}

#[test]
fn 깨진_설정_값은_기본인_켜짐으로_읽는다() {
    let (_dir, home) = temp_home();
    let text = r#"{"pausedUntil":null,"offDates":[],"displayMode":"on","p1NotifyOff":"yes"}"#;
    std::fs::write(home.control_path(), text).unwrap();
    let read = ControlFile::new(&home).read();
    assert!(read.p1_notify_on());
    assert!(serde_json::to_value(&read).unwrap().get("p1NotifyOff").is_none());
}
