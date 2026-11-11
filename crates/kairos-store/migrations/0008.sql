  CREATE TABLE outsider_marks (
    object_key TEXT PRIMARY KEY NOT NULL,
    first_seen_at TEXT NOT NULL
  );
  INSERT OR IGNORE INTO outsider_marks (object_key, first_seen_at)
    SELECT object_key, min(first_seen_at) FROM events
    WHERE source = 'github' AND object_key IS NOT NULL AND actor_kind = 'human'
      AND coalesce(trust_tier, '') NOT IN ('T0', 'T1')
    GROUP BY object_key;
