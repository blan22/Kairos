-- 차등 시험의 합성 run. Node와 Rust가 같은 파일을 실행한다. 항목 1은 P0, 2~은 parity.mjs의 순서를 따른다.
UPDATE items SET state = 'accepted' WHERE id IN (5, 6);
INSERT INTO runs (id, item_id, state, playbook, created_at, ready_kind, ready_at)
  VALUES ('00000000-0000-4000-8000-000000000001', 5, 'waiting_user', 'qa_issue', '2026-03-02T01:00:00.000Z', 'question', '2026-03-02T01:01:00.000Z');
INSERT INTO runs (id, item_id, state, playbook, created_at, ended_at, ready_kind, ready_at)
  VALUES ('00000000-0000-4000-8000-000000000002', 6, 'ended', 'qa_issue', '2026-03-02T01:00:00.000Z', '2026-03-02T01:02:00.000Z', 'ended', '2026-03-02T01:02:00.000Z');
INSERT INTO runs (id, item_id, state, playbook, created_at, ended_at, ready_at, fail_code)
  VALUES ('00000000-0000-4000-8000-000000000003', 7, 'failed', 'qa_issue', '2026-03-02T01:00:00.000Z', '2026-03-02T01:03:00.000Z', '2026-03-02T01:03:00.000Z', 'session_lost');
INSERT INTO runs (id, item_id, state, playbook, created_at, ended_at, ready_at, fail_code)
  VALUES ('00000000-0000-4000-8000-000000000004', 8, 'failed', 'qa_issue', '2026-03-02T01:00:00.000Z', '2026-03-02T01:04:00.000Z', '2026-03-02T01:04:00.000Z', 'mystery');
