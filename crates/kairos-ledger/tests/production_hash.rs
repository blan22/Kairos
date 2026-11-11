//! 저장된 객체의 `snapshot_hash`를 행 값으로 재계산한다. 어긋나면 전환 순간 모든 일정이 변경으로 잡힌다.
//! 운영 DB 사본 시험은 평소에 돌지 않는다: `KAIROS_HASH_DB=<사본> cargo test -p kairos-ledger --test production_hash -- --ignored --nocapture`
//! golden 시드 시험은 늘 돈다. 시드의 값을 합성 값으로 바꾸고 해시를 그대로 두면, 공개 저장소의 해시로 지운 값을 맞춰 볼 수 있다.
#![allow(clippy::unwrap_used)]

use kairos_core::model::{NormalizedObject, ObjectKind, SourceName, TrustTier};
use kairos_ledger::snapshot_hash;
use kairos_store::Database;
use rusqlite::{Connection, OpenFlags, Row};

fn flag(row: &Row<'_>, name: &str) -> Option<bool> {
    row.get::<_, Option<i64>>(name).unwrap().map(|value| value != 0)
}

fn text(row: &Row<'_>, name: &str) -> Option<String> {
    row.get(name).unwrap()
}

fn object_of(row: &Row<'_>) -> (NormalizedObject, String) {
    let source: SourceName = row.get::<_, String>("source").unwrap().parse().unwrap();
    let kind: ObjectKind = row.get::<_, String>("kind").unwrap().parse().unwrap();
    let key: String = row.get("object_key").unwrap();
    let external: String = row.get("external_id").unwrap();
    let object = NormalizedObject {
        permalink: text(row, "permalink"),
        actor_id: text(row, "actor_id"),
        channel_id: text(row, "channel_id"),
        ts: text(row, "ts"),
        text_head: text(row, "text_head"),
        trust_tier: text(row, "trust_tier").map(|value| value.parse::<TrustTier>().unwrap()),
        all_day: flag(row, "all_day"),
        start_date: text(row, "start_date"),
        end_date: text(row, "end_date"),
        start_at: text(row, "start_at"),
        end_at: text(row, "end_at"),
        series_id: text(row, "series_id"),
        attendee_count: row.get("attendee_count").unwrap(),
        my_response: text(row, "my_response"),
        busy: flag(row, "busy"),
        state: text(row, "state"),
        milestone_title: text(row, "milestone_title"),
        assigned_to_me: flag(row, "assigned_to_me"),
        authored_by_me: flag(row, "authored_by_me"),
        review_requested_me: flag(row, "review_requested_me"),
        is_draft: flag(row, "is_draft"),
        review_decision: text(row, "review_decision"),
        checks_state: text(row, "checks_state"),
        head_ref_oid: text(row, "head_ref_oid"),
        ..NormalizedObject::new(key, source, kind, external)
    };
    (object, row.get("snapshot_hash").unwrap())
}

#[test]
#[ignore = "운영 DB 사본이 있어야 돈다"]
fn 운영_객체의_해시가_모두_같다() {
    let path = std::env::var("KAIROS_HASH_DB").unwrap();
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut statement = db.prepare("SELECT * FROM objects").unwrap();
    let rows = statement.query_map([], |row| Ok(object_of(row))).unwrap();
    let (mut total, mut differ) = (0, 0);
    let mut by_kind = std::collections::BTreeMap::<String, (u32, u32)>::new();
    for row in rows {
        let (object, stored) = row.unwrap();
        total += 1;
        let entry = by_kind.entry(object.kind.as_str().to_owned()).or_default();
        entry.0 += 1;
        if snapshot_hash(&object) != stored {
            differ += 1;
            entry.1 += 1;
        }
    }
    println!("전체 {total}, 불일치 {differ}, 종류별 {by_kind:?}");
    assert!(total > 0);
    assert_eq!(differ, 0);
}

#[test]
fn golden_시드의_해시는_행_값으로_계산한_값과_같다() {
    let seeds = [
        ("agenda", include_str!("../../kairos-agenda/tests/fixtures/golden/seed.sql")),
        ("sessions", include_str!("../../kairos-sessions/tests/fixtures/golden/seed.sql")),
        ("tick", include_str!("../../kairos-tick/tests/fixtures/golden/seed.sql")),
    ];
    let mut checked = 0;
    for (name, seed) in seeds {
        let db = Database::in_memory().unwrap();
        db.connection().execute_batch(seed).unwrap();
        let mut statement = db.connection().prepare("SELECT * FROM objects").unwrap();
        let rows = statement.query_map([], |row| Ok(object_of(row))).unwrap();
        for row in rows {
            let (object, stored) = row.unwrap();
            assert_eq!(snapshot_hash(&object), stored, "{name} {}", object.object_key);
            checked += 1;
        }
    }
    assert!(checked > 0);
}
