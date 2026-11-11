//! W, R, S, RT는 리뷰 재현 번호다
//! collect, doctor, 잠금을 쓰는 시험은 3단계로 넘겼다.
#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use common::*;
use kairos_agenda::{ActionContext, DisplayMode, ItemRepo, ItemState, KairosControl, P1Notice};
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, SourceName};
use kairos_store::{Database, params};
use rusqlite::functions::FunctionFlags;

fn seeded() -> Database {
    let db = test_db();
    for source in [SourceName::Slack, SourceName::Github, SourceName::Gmail, SourceName::Notion] {
        prime(&db, source, "tick");
    }
    db
}

fn counts(db: &Database, now: &str) -> kairos_agenda::ItemCounts {
    ItemRepo::new(db.connection()).counts(at(now)).unwrap()
}

/// 포커스가 잡힌 제어 값. 시각은 월요일 KST다.
fn focusing(start: &str, end: &str) -> KairosControl {
    control_on().with_focus(&kst(MON, start), &kst(MON, end))
}

fn paused_to(when: &str) -> KairosControl {
    control_on().with_paused_until(Some(when))
}

fn sentry_mail(id: &str, when: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("noreply@sentry.io".into()),
        object_key: Some(format!("gmail:{id}")),
        text_head: Some("합성 메일 제목".into()),
        ..NormalizedEvent::new(format!("gmail:{id}"), SourceName::Gmail, EventType::MailThread, when, ActorKind::Bot)
    }
}

fn notion_mention(id: &str, when: &str) -> NormalizedEvent {
    NormalizedEvent {
        actor_id: Some("synthetic".into()),
        object_key: Some(format!("notion-mail:{id}")),
        mentions_me: Some(true),
        ..NormalizedEvent::new(
            format!("notion-mail:{id}:0"),
            SourceName::Notion,
            EventType::NotionMention,
            when,
            ActorKind::Human,
        )
    }
}

#[test]
fn r4b_s2_복귀가_없으면_주기가_긴_소스의_묵은_건은_다른_소스를_먹지도_먹히지도_않는다() {
    let db = seeded();
    let recorder = Recorder::new();
    Push::from(SourceName::Gmail, "tick", &kst(MON, "11:00"))
        .events(vec![sentry_mail("t1", &kst(MON, "10:20"))])
        .run(&db);
    assert_eq!(tick(&db, &kst(MON, "11:00"), &recorder).sent, 1);
    Push::from(SourceName::Notion, "tick", &kst(MON, "11:15"))
        .events(vec![notion_mention("t9", &kst(MON, "10:40"))])
        .run(&db);
    assert_eq!(tick(&db, &kst(MON, "11:15"), &recorder), report(1, 0, 0, 0));
    assert_eq!(recorder.messages(), ["Gmail Sentry 메일 1건", "Notion 멘션 1건"]);
}

#[test]
fn 복귀_구간이_지난_뒤의_보류_끝_알림은_밀린_건_구간을_만들지_않는다() {
    let db = seeded();
    let recorder = Recorder::new();
    let resumed = kst(MON, "12:10");
    say_in(&db, &resumed, "1", "C1", &kst(MON, "11:00"));
    tick_resumed(&db, &resumed, &recorder, Over::default(), Some(&resumed));
    assert!(recorder.messages().is_empty());
    tick_resumed(&db, &kst(MON, "13:30"), &recorder, Over::default(), Some(&resumed));
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    say_in(&db, &kst(MON, "13:33"), "2", "C2", &kst(MON, "11:05"));
    tick_resumed(&db, &kst(MON, "13:33"), &recorder, Over::default(), Some(&resumed));
    assert_eq!(recorder.messages().len(), 2);
}

#[test]
fn s1_전달_실패_뒤_게이트가_닫혀도_미전달로_남고_열린_뒤_다시_알린다() {
    let closed = [
        ("일시정지", Over::control(paused_to("indefinite"))),
        ("표시 꺼짐", Over::control(control_on().with_display_mode(DisplayMode::Off))),
        ("쉬는 날", Over::off_dates(off(&[MON]))),
    ];
    for (name, over) in closed {
        let db = seeded();
        say(&db, &kst(MON, "11:00"), "1", "C1");
        assert_eq!(tick(&db, &kst(MON, "11:00"), &Recorder::failing()).failed, 1, "{name}");
        assert_eq!(tick_over(&db, &kst(MON, "11:03"), &Recorder::new(), over), report(0, 0, 0, 0), "{name}");
        assert_eq!(item(&db, 1).p1_notified_at, None, "{name}");
        assert_eq!(counts(&db, &kst(MON, "11:03")).p1_undelivered, 1, "{name}");
        let recorder = Recorder::new();
        tick(&db, &kst(TUE, "11:06"), &recorder);
        assert_eq!(recorder.messages(), ["Slack 멘션 1건"], "{name}");
        assert_eq!(counts(&db, &kst(TUE, "11:06")).p1_undelivered, 0, "{name}");
    }
}

#[test]
fn r2_포커스나_점심으로_보류_중인_건은_일시정지_틱을_지나도_끝날_때_알린다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:10"), "1", "C1");
    tick_over(&db, &kst(MON, "10:10"), &recorder, Over::control(focusing("10:00", "10:50")));
    let paused = focusing("10:00", "10:50").with_paused_until(Some(&kst(MON, "10:35")));
    assert_eq!(tick_over(&db, &kst(MON, "10:30"), &recorder, Over::control(paused)), report(0, 0, 0, 0));
    assert_eq!(counts(&db, &kst(MON, "10:30")).p1_held, 1);
    tick(&db, &kst(MON, "10:51"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);

    let lunch = seeded();
    let after = Recorder::new();
    say(&lunch, &kst(MON, "12:10"), "1", "C1");
    tick(&lunch, &kst(MON, "12:10"), &after);
    tick_over(&lunch, &kst(MON, "12:40"), &after, Over::control(paused_to(&kst(MON, "12:45"))));
    tick(&lunch, &kst(MON, "13:31"), &after);
    assert_eq!(after.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn 닫힌_틱에_처음_잡힌_건만_버리고_이미_보류_중인_건은_남긴다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "12:10"), "1", "C1");
    tick(&db, &kst(MON, "12:10"), &recorder);
    say(&db, &kst(MON, "12:40"), "2", "C2");
    let paused = Over::control(paused_to(&kst(MON, "12:45")));
    assert_eq!(tick_over(&db, &kst(MON, "12:40"), &recorder, paused).dropped, 1);
    tick(&db, &kst(MON, "13:31"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn kairos_notify_off는_사용자가_끈_것이라_보류와_미전달도_버린다_다시_켜도_울리지_않는다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "11:00"), "1", "C1");
    tick(&db, &kst(MON, "11:00"), &Recorder::failing());
    say(&db, &kst(MON, "12:10"), "2", "C2");
    tick(&db, &kst(MON, "12:10"), &recorder);
    let notify_off = Over::control(control_on().with_p1_notify(false));
    assert_eq!(tick_over(&db, &kst(MON, "12:13"), &recorder, notify_off).dropped, 2);
    assert_eq!(counts(&db, &kst(MON, "12:13")).p1_undelivered, 0);
    tick(&db, &kst(MON, "14:00"), &recorder);
    assert!(recorder.messages().is_empty());
}

#[test]
fn r5_포커스_전_알림의_30분_구간이_포커스_중_쌓인_건을_먹지_않는다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:00"), "1", "C1");
    tick(&db, &kst(MON, "10:00"), &recorder);
    say(&db, &kst(MON, "10:10"), "2", "C1");
    say(&db, &kst(MON, "10:12"), "3", "C1");
    tick_over(&db, &kst(MON, "10:12"), &recorder, Over::control(focusing("10:05", "10:20")));
    assert_eq!(tick(&db, &kst(MON, "10:21"), &recorder), report(2, 0, 0, 0));
    assert_eq!(recorder.messages(), ["Slack 멘션 1건", "Slack 멘션 2건"]);
}

#[test]
fn s4_포커스_끝_알림의_건수에_앞선_알림과_같은_채널의_건도_든다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:00"), "1", "C1");
    tick(&db, &kst(MON, "10:00"), &recorder);
    say(&db, &kst(MON, "10:10"), "2", "C1");
    say(&db, &kst(MON, "10:12"), "3", "C2");
    tick_over(&db, &kst(MON, "10:12"), &recorder, Over::control(focusing("10:05", "10:20")));
    tick(&db, &kst(MON, "10:21"), &recorder);
    assert_eq!(recorder.messages(), ["Slack 멘션 1건", "Slack 멘션 2건"]);
}

#[test]
fn 보류_끝_알림은_그_뒤_같은_채널의_새_건에_30분_구간을_연다() {
    let db = seeded();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:10"), "1", "C1");
    tick_over(&db, &kst(MON, "10:10"), &recorder, Over::control(focusing("10:05", "10:20")));
    tick(&db, &kst(MON, "10:21"), &recorder);
    say(&db, &kst(MON, "10:24"), "2", "C1");
    assert_eq!(tick(&db, &kst(MON, "10:24"), &recorder), report(0, 1, 0, 0));
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
}

#[test]
fn rt1_재시도는_세_번은_틱마다_뒤의_두_번은_30분_간격이고_그_뒤로는_doctor에만_남는다() {
    let db = seeded();
    let recorder = Recorder::failing();
    say(&db, &kst(MON, "10:00"), "1", CHANNEL);
    let times =
        ["10:00", "10:03", "10:06", "10:09", "10:33", "10:36", "10:39", "11:03", "11:06", "11:09", "12:00", "16:00"];
    for time in times {
        tick(&db, &kst(MON, time), &recorder);
    }
    assert_eq!(recorder.messages().len(), 5);
    assert_eq!(item(&db, 1).p1_notified_at, None);
    let counted = counts(&db, &kst(MON, "16:00"));
    assert_eq!((counted.p1_undelivered, counted.p1_gave_up, counted.p1_held), (1, 1, 0));
}

#[test]
fn rt1_재시도가_끝나도_항목별이다_뒤에_온_건은_새로_시도하고_앞선_건은_건드리지_않는다() {
    let db = seeded();
    let failing = Recorder::failing();
    say(&db, &kst(MON, "10:00"), "1", "C1");
    for time in ["10:00", "10:03", "10:06", "10:36", "11:06"] {
        tick(&db, &kst(MON, time), &failing);
    }
    say(&db, &kst(MON, "14:00"), "2", "C2");
    let recorder = Recorder::new();
    assert_eq!(tick(&db, &kst(MON, "14:00"), &recorder).sent, 1);
    assert_eq!(recorder.messages(), ["Slack 멘션 1건"]);
    assert_eq!(counts(&db, &kst(MON, "14:00")).p1_undelivered, 1);
}

#[test]
fn rt3_전달_뒤_기록이_계속_실패해도_알림_횟수는_상한을_넘지_않고_미전달로_남는다() {
    let db = seeded();
    db.connection()
        .execute_batch(
            "CREATE TRIGGER block_alert BEFORE INSERT ON suggestions WHEN NEW.surface = 'osascript'
         BEGIN SELECT RAISE(ABORT, 'synthetic'); END",
        )
        .unwrap();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:00"), "1", CHANNEL);
    for time in ["10:00", "10:03", "10:06", "10:09", "10:12", "10:15"] {
        tick(&db, &kst(MON, time), &recorder);
    }
    assert_eq!(recorder.messages().len(), 3);
    assert_eq!(counts(&db, &kst(MON, "10:15")).p1_undelivered, 1);
}

#[test]
fn rt3_전달_뒤_기록이_한_번_실패하면_프로세스_안에서_다시_써서_다음_틱에_울리지_않는다() {
    let db = seeded();
    let failures = Arc::new(AtomicI64::new(1));
    let left = Arc::clone(&failures);
    db.connection()
        .create_scalar_function("flaky_write", 0, FunctionFlags::SQLITE_UTF8, move |_| {
            if left.fetch_sub(1, Ordering::SeqCst) > 0 {
                return Err(rusqlite::Error::UserFunctionError("합성 실패".into()));
            }
            Ok(1)
        })
        .unwrap();
    db.connection()
        .execute_batch("CREATE TEMP TRIGGER flaky_alert BEFORE INSERT ON suggestions BEGIN SELECT flaky_write(); END")
        .unwrap();
    let recorder = Recorder::new();
    say(&db, &kst(MON, "10:00"), "1", CHANNEL);
    assert_eq!(tick(&db, &kst(MON, "10:00"), &recorder), report(1, 0, 0, 0));
    assert!(item(&db, 1).p1_notified_at.is_some());
    tick(&db, &kst(MON, "10:03"), &recorder);
    assert_eq!(recorder.messages().len(), 1);
}

#[test]
fn 어제의_묶음과_버림은_오늘_건수에_들지_않는다() {
    let db = seeded();
    say(&db, &kst(MON, "11:00"), "1", CHANNEL);
    tick_over(&db, &kst(MON, "11:00"), &Recorder::new(), Over::control(paused_to("indefinite")));
    assert_eq!(counts(&db, &kst(MON, "11:00")).p1_dropped_today, 1);
    assert_eq!(counts(&db, &kst(TUE, "11:00")).p1_dropped_today, 0);
    assert_eq!(item(&db, 1).p1_notice, Some(P1Notice::Dropped));
}

#[test]
fn r6_s3_이미_알린_p1이_다시_깨어나면_울리지_않는다_각_항목은_한_번이다() {
    let db = seeded();
    let recorder = Recorder::new();
    let thread = "slack:C1:100".to_owned();
    let in_thread =
        |ts: &str, when: &str| NormalizedEvent { thread_key: Some(thread.clone()), ..mention_in(ts, when, "C1") };
    slack(&kst(MON, "10:00")).events(vec![in_thread("100", &kst(MON, "10:00"))]).run(&db);
    tick(&db, &kst(MON, "10:00"), &recorder);
    let none = none();
    actions(&db).keep(&[1], &ActionContext { now: at(&kst(MON, "10:10")), off_dates: &none }, false).unwrap();
    slack(&kst(MON, "15:00")).events(vec![in_thread("200", &kst(MON, "15:00"))]).run(&db);
    tick(&db, &kst(MON, "15:00"), &recorder);
    assert_eq!(item(&db, 1).state, ItemState::Open);
    assert_eq!(recorder.messages().len(), 1);

    db.connection()
        .execute(
            "UPDATE items SET state = 'parked', park_trigger = 'deploy', parked_until = ? WHERE id = 1",
            params![kst(TUE, "10:00")],
        )
        .unwrap();
    tick(&db, &kst(TUE, "10:03"), &recorder);
    assert_eq!(item(&db, 1).state, ItemState::Open);
    assert_eq!(recorder.messages().len(), 1);
}
