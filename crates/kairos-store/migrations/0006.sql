
  ALTER TABLE items ADD COLUMN p1_notified_at TEXT;
  ALTER TABLE items ADD COLUMN p1_notify_failed_at TEXT;
  UPDATE items SET p1_notified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE tier = 'P1';
  