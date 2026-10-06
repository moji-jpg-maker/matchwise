-- Matchwise M4: match records, their score snapshots, notes and decision history.
-- A match is one unordered pair of profiles (profile_a < profile_b). Deleting either profile deletes the match
-- and everything below it, so data deletion is complete.
CREATE TABLE IF NOT EXISTS mm_matches (
    id                TEXT PRIMARY KEY,
    org_id            TEXT NOT NULL DEFAULT 'default',
    profile_a         TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    profile_b         TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    source_profile    TEXT,                       -- the person the search was run for (informational)
    rule_set_id       TEXT NOT NULL REFERENCES mm_rule_sets(id),
    rule_set_version  INTEGER NOT NULL,
    status            TEXT NOT NULL,
    hold_reason       TEXT,                       -- set while the matchmaker waits for more information
    a_response        TEXT NOT NULL DEFAULT 'unknown',
    b_response        TEXT NOT NULL DEFAULT 'unknown',
    outcome           TEXT,
    override_reason   TEXT,                       -- why an excluded pair was advanced anyway
    weight_overrides  TEXT,                       -- JSON {dimension: weight} applied only to this match
    hidden            INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    CHECK (profile_a < profile_b),
    UNIQUE (org_id, profile_a, profile_b)
);
CREATE INDEX IF NOT EXISTS idx_mm_matches_status ON mm_matches (org_id, status);
CREATE INDEX IF NOT EXISTS idx_mm_matches_a ON mm_matches (profile_a);
CREATE INDEX IF NOT EXISTS idx_mm_matches_b ON mm_matches (profile_b);

-- Immutable score snapshots: why a pair was recommended, which rule-set version produced the score, and every re-score.
CREATE TABLE IF NOT EXISTS mm_match_snapshots (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    match_id          TEXT NOT NULL REFERENCES mm_matches(id) ON DELETE CASCADE,
    rule_set_id       TEXT NOT NULL,
    rule_set_version  INTEGER NOT NULL,
    trigger           TEXT NOT NULL,              -- created | rescore | weights_adjusted
    scorecard         TEXT NOT NULL,              -- JSON ScoreCard
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mm_snapshots_match ON mm_match_snapshots (match_id, id);

CREATE TABLE IF NOT EXISTS mm_match_notes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    match_id    TEXT NOT NULL REFERENCES mm_matches(id) ON DELETE CASCADE,
    text        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

-- Decision history shown on the match. Reasons are matchmaker-written text, and the global audit log never copies them.
CREATE TABLE IF NOT EXISTS mm_match_events (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    match_id     TEXT NOT NULL REFERENCES mm_matches(id) ON DELETE CASCADE,
    at           TEXT NOT NULL,
    actor        TEXT NOT NULL,
    kind         TEXT NOT NULL,
    from_status  TEXT,
    to_status    TEXT,
    detail       TEXT
);
CREATE INDEX IF NOT EXISTS idx_mm_events_match ON mm_match_events (match_id, id);
