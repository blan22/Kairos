
  ALTER TABLE events ADD COLUMN item_id INTEGER;
  CREATE INDEX events_item ON events (item_id) WHERE item_id IS NOT NULL;
  CREATE INDEX events_unassigned ON events (occurred_at, id)
    WHERE disposition = 'opened' AND baseline = 0 AND close_reason IS NULL AND item_id IS NULL;

  CREATE TABLE items (
    id INTEGER PRIMARY KEY,
    object_key TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (reason IN (
      'reply_owed', 'review_owed', 'assigned', 'my_pr_attention',
      'meeting_new', 'deploy_risk', 'work_owed', 'manual'
    )),
    tier TEXT NOT NULL CHECK (tier IN ('P0', 'P1', 'P2')),
    state TEXT NOT NULL DEFAULT 'open' CHECK (state IN ('open', 'parked', 'accepted', 'closed')),
    baseline INTEGER NOT NULL DEFAULT 0 CHECK (baseline IN (0, 1)),
    source TEXT,
    channel_id TEXT,
    title TEXT CHECK (title IS NULL OR (reason = 'manual' AND length(title) <= 120)),
    opened_by_event TEXT,
    opened_at TEXT NOT NULL,
    due_at TEXT,
    first_surfaced_at TEXT,
    push_count INTEGER NOT NULL DEFAULT 0 CHECK (push_count >= 0),
    bundle_queued INTEGER NOT NULL DEFAULT 0 CHECK (bundle_queued IN (0, 1)),
    parked_until TEXT,
    park_trigger TEXT CHECK (park_trigger IS NULL OR park_trigger IN ('next_bundle', 'deploy', 'kept', 'triage')),
    p0_notified_at TEXT,
    closed_at TEXT,
    close_reason TEXT CHECK (close_reason IS NULL OR close_reason IN (
      'done_evidence', 'withdrawn', 'expired', 'user_done', 'user_dropped'
    )),
    close_evidence_event TEXT,
    CHECK ((state = 'closed') = (closed_at IS NOT NULL)),
    CHECK ((state = 'closed') = (close_reason IS NOT NULL)),
    CHECK (close_reason NOT IN ('done_evidence', 'withdrawn') OR close_evidence_event IS NOT NULL),
    CHECK ((state = 'parked') = (park_trigger IS NOT NULL)),
    CHECK (tier != 'P0' OR (reason IN ('meeting_new', 'deploy_risk') AND opened_by_event IS NOT NULL))
  );
  CREATE UNIQUE INDEX items_live ON items (object_key, reason) WHERE state != 'closed';
  CREATE INDEX items_state ON items (state, tier, opened_at);

  CREATE TABLE suggestions (
    id INTEGER PRIMARY KEY,
    item_id INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    card_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('item', 'bundle', 'carryover', 'p0_alert')),
    tier TEXT NOT NULL CHECK (tier IN ('P0', 'P1', 'P2')),
    shown_at TEXT NOT NULL,
    surface TEXT NOT NULL CHECK (surface IN ('panel', 'notification', 'osascript')),
    session_id TEXT,
    seen INTEGER NOT NULL DEFAULT 0 CHECK (seen IN (0, 1)),
    response TEXT CHECK (response IS NULL OR response IN (
      'accept', 'later', 'done', 'drop', 'keep', 'ack', 'expired', 'superseded', 'closed'
    )),
    response_at TEXT,
    CHECK ((response IS NULL) = (response_at IS NULL))
  );
  CREATE UNIQUE INDEX suggestions_pending ON suggestions (item_id) WHERE seen = 1 AND response IS NULL;
  CREATE INDEX suggestions_shown ON suggestions (shown_at);
  CREATE INDEX suggestions_item ON suggestions (item_id);

  CREATE TABLE runs (
    id TEXT PRIMARY KEY NOT NULL,
    item_id INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('claimed', 'started', 'waiting_user', 'needs_attention', 'ended', 'failed')),
    worktree_identity TEXT,
    pane_key TEXT,
    playbook TEXT NOT NULL,
    created_at TEXT NOT NULL,
    ended_at TEXT,
    CHECK ((state IN ('ended', 'failed')) = (ended_at IS NOT NULL))
  );
  CREATE UNIQUE INDEX runs_active ON runs (item_id) WHERE state IN ('claimed', 'started', 'waiting_user', 'needs_attention');
  