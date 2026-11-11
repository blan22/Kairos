
  ALTER TABLE items ADD COLUMN p1_held_at TEXT;
  ALTER TABLE items ADD COLUMN p1_notify_attempts INTEGER NOT NULL DEFAULT 0 CHECK (p1_notify_attempts >= 0);
  ALTER TABLE items ADD COLUMN p1_notice TEXT CHECK (p1_notice IS NULL OR p1_notice IN ('sent', 'merged', 'dropped', 'handled'));
  CREATE TABLE tick_clock (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_tick_at TEXT NOT NULL,
    resumed_at TEXT
  );
  UPDATE items SET p1_notify_attempts = 1 WHERE p1_notify_failed_at IS NOT NULL;
  UPDATE items SET p1_held_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE tier = 'P1' AND state = 'open' AND baseline = 0 AND p1_notified_at IS NULL AND p1_notify_failed_at IS NULL;
  