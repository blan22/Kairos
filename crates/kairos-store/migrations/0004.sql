
  ALTER TABLE runs ADD COLUMN worktree_id TEXT;
  ALTER TABLE runs ADD COLUMN terminal_handle TEXT;
  ALTER TABLE runs ADD COLUMN adopted INTEGER NOT NULL DEFAULT 0 CHECK (adopted IN (0, 1));
  ALTER TABLE runs ADD COLUMN launched_at TEXT;
  ALTER TABLE runs ADD COLUMN session_started_at TEXT;
  ALTER TABLE runs ADD COLUMN seen_at TEXT;
  ALTER TABLE runs ADD COLUMN ready_kind TEXT CHECK (ready_kind IS NULL OR ready_kind IN ('question', 'done', 'ended', 'attention'));
  ALTER TABLE runs ADD COLUMN ready_at TEXT;
  ALTER TABLE runs ADD COLUMN ready_seen_at TEXT;
  