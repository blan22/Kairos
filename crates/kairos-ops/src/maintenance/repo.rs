use std::path::Path;

use kairos_core::{Instant, KairosResult};
use kairos_ledger::EVENT_RETENTION_DAYS;
use kairos_store::row::sql;
use kairos_store::{Connection, params};

/// 닫히지 않은 사건의 제목을 지우는 기한. 사건은 남기고 외부 문자열만 지운다(설계 3.7).
pub const OPEN_TEXT_RETENTION_DAYS: i64 = 90;
const DAY_MS: i64 = 86_400_000;

fn days_before(now: Instant, days: i64) -> String {
    now.minus_ms(days * DAY_MS).to_iso()
}

/// 보존 정리의 SQL. 순서가 뜻을 가지므로 각 메서드의 설명을 따른다.
#[derive(Clone, Copy, Debug)]
pub struct MaintenanceRepo<'a> {
    connection: &'a Connection,
}

impl<'a> MaintenanceRepo<'a> {
    pub const fn new(connection: &'a Connection) -> Self {
        Self { connection }
    }

    fn run(&self, statement: &str, cutoff: &str) -> KairosResult<i64> {
        let changes = self.connection.execute(statement, params![cutoff]).map_err(sql)?;
        Ok(changes as i64)
    }

    /// 30일이 지난 events를 지운다. 다음은 지우지 않는다.
    /// - 아직 열려 있는 사건(처분 opened, 종료 없음). 증거 없이 사라지면 안 된다.
    ///   항목이 없거나 살아 있는 항목에 붙은 경우만이다. 항목이 닫혔거나 이미 지워졌으면 끝난 사건이다.
    ///   이벤트에는 사용자 종료를 적을 수 없어 항목으로 판단한다.
    /// - 살아 있는 항목을 연 이벤트. 남김으로 만든 할 일은 붙은 이벤트가 없어 제목과 링크를 여기서 읽는다.
    /// - 남아 있는 다른 이벤트나 항목의 종료 증거. 지우면 근거 없는 종료가 생긴다.
    pub fn prune_events(&self, now: Instant) -> KairosResult<i64> {
        self.run(
            "DELETE FROM events
             WHERE occurred_at < ?1
               AND NOT (
                 disposition = 'opened' AND close_reason IS NULL
                 AND (item_id IS NULL OR item_id IN (SELECT id FROM items WHERE state != 'closed'))
               )
               AND dedupe_key NOT IN (SELECT opened_by_event FROM items WHERE state != 'closed' AND opened_by_event IS NOT NULL)
               AND dedupe_key NOT IN (
                 SELECT close_evidence_event FROM events
                 WHERE close_evidence_event IS NOT NULL AND occurred_at >= ?1
               )
               AND dedupe_key NOT IN (SELECT close_evidence_event FROM items WHERE close_evidence_event IS NOT NULL)",
            &days_before(now, EVENT_RETENTION_DAYS),
        )
    }

    /// 30일 동안 어떤 수집에도 보이지 않았고 남은 이벤트도 없는 객체를 지운다.
    /// 메일과 DM은 조회 창에서 벗어난 뒤, 일정은 끝난 뒤, GitHub은 닫히거나 내 목록에서 빠진 뒤부터 잰다.
    /// events를 먼저 지워야 하고, 마지막 관측 시각을 source_run에서 읽으므로 `prune_runs`보다 먼저 부른다.
    pub fn prune_objects(&self, now: Instant) -> KairosResult<i64> {
        self.run(
            "DELETE FROM objects
             WHERE coalesce((SELECT finished_at FROM source_run WHERE id = objects.last_seen_run), '') < ?
               AND object_key NOT IN (SELECT object_key FROM events WHERE object_key IS NOT NULL)
               AND object_key NOT IN (SELECT object_key FROM items WHERE state != 'closed')",
            &days_before(now, EVENT_RETENTION_DAYS),
        )
    }

    /// 닫힌 지 30일이 지난 항목을 지운다. 제안과 세션 기록은 외래 키로 함께 지워진다.
    /// 활성 세션이 있는 항목은 남긴다. 기준선 객체의 항목은 객체가 있는 동안 남긴다. 지우면 triage에 다시 나온다.
    /// 이벤트의 종료 근거를 풀어 주므로 `prune_events`보다 먼저 부른다.
    pub fn prune_items(&self, now: Instant) -> KairosResult<i64> {
        self.run(
            "DELETE FROM items
             WHERE state = 'closed' AND closed_at < ?
               AND id NOT IN (SELECT item_id FROM runs WHERE state NOT IN ('ended', 'failed'))
               AND object_key NOT IN (SELECT object_key FROM objects WHERE baseline = 1)",
            &days_before(now, EVENT_RETENTION_DAYS),
        )
    }

    /// 닫히지 않은 채 오래된 사건은 항목으로 남기되 제목과 발신자는 지운다. work_owed는 단계 1에서 닫을 방법이 없다.
    pub fn scrub_stale_open(&self, now: Instant) -> KairosResult<i64> {
        let cutoff = days_before(now, OPEN_TEXT_RETENTION_DAYS);
        let events = self.run(
            "UPDATE events SET text_head = NULL WHERE text_head IS NOT NULL AND occurred_at < ?
               AND (close_reason IS NULL OR dedupe_key IN (SELECT opened_by_event FROM items WHERE state != 'closed' AND opened_by_event IS NOT NULL))",
            &cutoff,
        )?;
        // Notion의 행위자는 주소가 아니라 사람 이름이다. 다른 소스의 actor_id는 리뷰어 대조 같은 종료 판정에 쓰여 남긴다.
        let actors = self.run(
            "UPDATE events SET actor_id = NULL
             WHERE source = 'notion' AND actor_id IS NOT NULL AND occurred_at < ? AND close_reason IS NULL",
            &cutoff,
        )?;
        let objects = self.run(
            "UPDATE objects SET text_head = NULL, actor_id = NULL
             WHERE (text_head IS NOT NULL OR actor_id IS NOT NULL)
               AND coalesce((SELECT finished_at FROM source_run WHERE id = objects.last_seen_run), '') < ?",
            &cutoff,
        )?;
        Ok(events + actors + objects)
    }

    /// 30일이 지난 실행 기록을 지운다. 질의별 마지막 ok는 건강 판정의 기준이라 남긴다.
    pub fn prune_runs(&self, now: Instant) -> KairosResult<i64> {
        self.run(
            "DELETE FROM source_run
             WHERE finished_at < ?
               AND id NOT IN (SELECT max(id) FROM source_run WHERE outcome = 'ok' GROUP BY source, query)",
            &days_before(now, EVENT_RETENTION_DAYS),
        )
    }

    /// 조회 창보다 오래 건드리지 않은 읽기 기록을 지운다. 이 스레드는 검색에 다시 오지 않는다.
    pub fn prune_thread_reads(&self, now: Instant) -> KairosResult<i64> {
        self.run("DELETE FROM thread_read WHERE updated_at < ?", &days_before(now, EVENT_RETENTION_DAYS + 1))
    }

    /// 지금 DB의 복사본을 만든다.
    pub(super) fn vacuum_into(&self, target: &Path) -> KairosResult<()> {
        // SQL 문자열 리터럴이므로 따옴표만 이스케이프한다. 경로는 kairos가 만든 값이다.
        let quoted = target.to_string_lossy().replace('\'', "''");
        self.connection.execute_batch(&format!("VACUUM INTO '{quoted}'")).map_err(sql)
    }
}
