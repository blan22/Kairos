//! next 시험의 도우미. 고정값은 모두 합성이다.
#![allow(dead_code, clippy::unwrap_used)]

use kairos_agenda::{KairosControl, ListResponse, NextContext, NextOptions, NextResponse, Presenter};
use kairos_core::model::{ActorKind, EventType, NormalizedEvent, SourceName};
use kairos_store::Database;

use super::common::*;

/// 표시를 켜고 세션 잠금을 푼 제어 값.
pub fn control() -> KairosControl {
    control_on().with_sessions(Some(BEFORE))
}

pub fn ctx(now: &str) -> NextContext {
    context(now, &control(), Over::default())
}

pub fn ctx_over(now: &str, over: Over) -> NextContext {
    context(now, &control(), over)
}

pub fn peek(db: &Database, now: &str) -> NextResponse {
    peek_over(db, now, Over::default())
}

pub fn peek_over(db: &Database, now: &str, over: Over) -> NextResponse {
    next_with(db, &ctx_over(now, over), NextOptions::default())
}

pub fn see(db: &Database, now: &str) -> NextResponse {
    see_over(db, now, Over::default())
}

pub fn see_over(db: &Database, now: &str, over: Over) -> NextResponse {
    next_with(db, &ctx_over(now, over), NextOptions { seen: true, ack: false })
}

pub fn ack(db: &Database, now: &str) -> NextResponse {
    next_with(db, &ctx(now), NextOptions { seen: false, ack: true })
}

pub fn list(db: &Database, now: &str) -> ListResponse {
    Presenter::new(db).list_response(&ctx(now), false).unwrap()
}

/// 채널마다 다른 P1 멘션을 연다.
pub fn p1(db: &Database, when: &str, channels: &[&str]) {
    let events = channels
        .iter()
        .enumerate()
        .map(|(index, channel)| mention_in(&format!("{}.{when}", index + 1), when, channel))
        .collect();
    slack(when).events(events).run(db);
    refresh(db, when);
}

pub fn p2(db: &Database, when: &str, numbers: &[i64]) {
    github(when).objects(numbers.iter().map(|number| issue(*number)).collect()).run(db);
    let events = numbers
        .iter()
        .map(|number| NormalizedEvent {
            object_key: Some(format!("gh:acme/widget#{number}")),
            ..NormalizedEvent::new(
                format!("gh:assigned:{number}"),
                SourceName::Github,
                EventType::GhAssigned,
                when,
                ActorKind::Human,
            )
        })
        .collect();
    github(when).events(events).run(db);
    refresh(db, when);
}

pub fn seeded() -> Database {
    let db = test_db();
    prime_slack(&db);
    prime(&db, SourceName::Github, "tick");
    db
}

pub fn card_ids(response: &NextResponse) -> Vec<i64> {
    response.card.as_ref().unwrap().items.iter().map(|row| row.id).collect()
}
