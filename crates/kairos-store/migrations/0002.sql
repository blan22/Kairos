
  ALTER TABLE cursor ADD COLUMN first_run_at TEXT;
  ALTER TABLE cursor ADD COLUMN progress_at TEXT;

  CREATE TABLE thread_read (
    source TEXT NOT NULL,
    query TEXT NOT NULL,
    external_id TEXT NOT NULL,
    message_count INTEGER,
    failures INTEGER NOT NULL DEFAULT 0,
    first_failed_at TEXT,
    quarantined_at TEXT,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (source, query, external_id)
  );
  