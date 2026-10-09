-- Matchwise M6: AI suggestions and analyses. A model only ever suggests: nothing here changes a profile, a score or a status
-- until the matchmaker accepts it, and every accepted value carries "AI-inferred" provenance.

-- One row per extraction or pair analysis. Results are validated before they are stored.
CREATE TABLE IF NOT EXISTS mm_ai_runs (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id        TEXT REFERENCES mm_profiles(id) ON DELETE CASCADE,
    match_id          TEXT REFERENCES mm_matches(id) ON DELETE CASCADE,
    kind              TEXT NOT NULL,                  -- extract | pair
    provider          TEXT NOT NULL,
    model             TEXT NOT NULL,
    is_cloud          INTEGER NOT NULL,
    input_hash        TEXT NOT NULL,                  -- of the prompt, to tell when an analysis is out of date
    rule_set_version  INTEGER,
    result            TEXT NOT NULL,                  -- JSON
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mm_ai_runs_profile ON mm_ai_runs (profile_id, kind, id);
CREATE INDEX IF NOT EXISTS idx_mm_ai_runs_match ON mm_ai_runs (match_id, id);

CREATE TABLE IF NOT EXISTS mm_ai_suggestions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    run_id      INTEGER REFERENCES mm_ai_runs(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                        -- field | preference
    field       TEXT NOT NULL,
    payload     TEXT NOT NULL,                        -- JSON: the value, or the preference
    evidence    TEXT NOT NULL,
    confidence  REAL NOT NULL,
    conflict    INTEGER NOT NULL DEFAULT 0,
    status      TEXT NOT NULL DEFAULT 'pending',      -- pending | accepted | rejected | superseded
    model       TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    decided_at  TEXT
);
CREATE INDEX IF NOT EXISTS idx_mm_ai_suggestions ON mm_ai_suggestions (profile_id, status);

-- What left this computer. Sizes always. The (already redacted) prompt text only for cloud calls, and it is blanked after 30 days.
CREATE TABLE IF NOT EXISTS mm_ai_log (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    at                 TEXT NOT NULL,
    kind               TEXT NOT NULL,
    provider           TEXT NOT NULL,
    model              TEXT NOT NULL,
    is_cloud           INTEGER NOT NULL,
    include_sensitive  INTEGER NOT NULL,
    profile_a          TEXT,                          -- no foreign key: removed explicitly when a profile is deleted
    profile_b          TEXT,
    input_chars        INTEGER NOT NULL,
    output_chars       INTEGER NOT NULL DEFAULT 0,
    ok                 INTEGER NOT NULL DEFAULT 0,
    error              TEXT,
    prompt             TEXT
);
CREATE INDEX IF NOT EXISTS idx_mm_ai_log_profiles ON mm_ai_log (profile_a, profile_b);
