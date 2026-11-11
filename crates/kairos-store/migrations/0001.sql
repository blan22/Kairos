
  CREATE TABLE source_run (
    id INTEGER PRIMARY KEY,
    source TEXT NOT NULL,
    query TEXT NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN (
      'ok', 'partial', 'tool_missing', 'auth', 'rate_limited',
      'parse_error', 'timeout', 'contract_break', 'not_connected'
    )),
    pages INTEGER NOT NULL DEFAULT 0,
    expected_count INTEGER,
    parsed_count INTEGER NOT NULL DEFAULT 0,
    identity TEXT,
    window_from TEXT,
    window_to TEXT,
    error_code TEXT
  );
  CREATE INDEX source_run_latest ON source_run (source, query, id);

  CREATE TABLE cursor (
    source TEXT NOT NULL,
    query TEXT NOT NULL,
    watermark TEXT,
    overlap_s INTEGER NOT NULL DEFAULT 0,
    t0 TEXT,
    PRIMARY KEY (source, query)
  );

  CREATE TABLE objects (
    object_key TEXT PRIMARY KEY NOT NULL,
    source TEXT NOT NULL,
    query TEXT NOT NULL,
    kind TEXT NOT NULL,
    external_id TEXT NOT NULL,
    snapshot_hash TEXT NOT NULL,
    permalink TEXT,
    actor_id TEXT,
    channel_id TEXT,
    ts TEXT,
    text_head TEXT CHECK (text_head IS NULL OR length(text_head) <= 120),
    trust_tier TEXT CHECK (trust_tier IS NULL OR trust_tier IN ('T0', 'T1', 'T2', 'T3')),
    all_day INTEGER,
    start_date TEXT,
    end_date TEXT,
    start_at TEXT,
    end_at TEXT,
    series_id TEXT,
    attendee_count INTEGER,
    my_response TEXT,
    busy INTEGER,
    state TEXT,
    milestone_title TEXT,
    assigned_to_me INTEGER,
    authored_by_me INTEGER,
    review_requested_me INTEGER,
    is_draft INTEGER,
    review_decision TEXT,
    checks_state TEXT,
    head_ref_oid TEXT,
    baseline INTEGER NOT NULL DEFAULT 0 CHECK (baseline IN (0, 1)),
    first_seen_at TEXT NOT NULL,
    last_seen_run INTEGER NOT NULL,
    missing_runs INTEGER NOT NULL DEFAULT 0
  );
  CREATE INDEX objects_scope ON objects (source, query, last_seen_run);
  CREATE INDEX objects_kind ON objects (kind);

  CREATE TABLE events (
    id INTEGER PRIMARY KEY,
    dedupe_key TEXT NOT NULL UNIQUE,
    source TEXT NOT NULL,
    object_key TEXT,
    thread_key TEXT NOT NULL,
    type TEXT NOT NULL,
    actor_id TEXT,
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('human', 'bot', 'self')),
    mentions_me INTEGER NOT NULL DEFAULT 0 CHECK (mentions_me IN (0, 1)),
    mention_count INTEGER NOT NULL DEFAULT 0,
    occurred_at TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    run_id INTEGER NOT NULL,
    catchup INTEGER NOT NULL DEFAULT 0 CHECK (catchup IN (0, 1)),
    baseline INTEGER NOT NULL DEFAULT 0 CHECK (baseline IN (0, 1)),
    channel_id TEXT,
    permalink TEXT,
    text_head TEXT CHECK (text_head IS NULL OR length(text_head) <= 120),
    trust_tier TEXT CHECK (trust_tier IS NULL OR trust_tier IN ('T0', 'T1', 'T2', 'T3')),
    disposition TEXT NOT NULL CHECK (disposition IN ('opened', 'folded', 'noise', 'unclassified')),
    rule_id TEXT NOT NULL,
    close_reason TEXT CHECK (close_reason IS NULL OR close_reason IN ('done_evidence', 'withdrawn', 'expired')),
    close_evidence_event TEXT,
    closed_at TEXT,
    CHECK (close_reason IS NULL OR disposition = 'opened'),
    CHECK ((close_reason IS NULL) = (closed_at IS NULL)),
    CHECK (close_reason NOT IN ('done_evidence', 'withdrawn') OR close_evidence_event IS NOT NULL)
  );
  CREATE INDEX events_object ON events (object_key, occurred_at);
  CREATE INDEX events_thread ON events (thread_key, occurred_at);
  CREATE INDEX events_open ON events (source, disposition, close_reason);
  