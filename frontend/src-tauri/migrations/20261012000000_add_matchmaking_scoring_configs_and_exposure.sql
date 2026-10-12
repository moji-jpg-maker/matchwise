-- Matchwise M7b step 2: versioned scoring configs and exposure logging.
--
-- A rule set says what to measure; a scoring config says how to read the result (the strong/mixed cutoffs and the
-- neutral prior). Configs are versioned exactly like rule sets so every snapshot can name the numbers it was judged
-- by. Exposure rows record which candidates a search actually showed, so a future ranking can be checked against what
-- was on screen rather than only against what was decided.

CREATE TABLE IF NOT EXISTS mm_scoring_configs (
    id               TEXT PRIMARY KEY,
    org_id           TEXT NOT NULL DEFAULT 'default',
    name             TEXT NOT NULL,
    description      TEXT NOT NULL DEFAULT '',
    current_version  INTEGER NOT NULL,
    archived         INTEGER NOT NULL DEFAULT 0,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL,
    UNIQUE (org_id, name)
);

CREATE TABLE IF NOT EXISTS mm_scoring_config_versions (
    config_id    TEXT NOT NULL REFERENCES mm_scoring_configs(id) ON DELETE CASCADE,
    version      INTEGER NOT NULL,
    definition   TEXT NOT NULL,       -- JSON ScoringConfig
    note         TEXT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (config_id, version)
);

-- Which numbers judged this pair. Nullable on purpose: matches created before scoring configs existed were judged by
-- the literals baked into that build, and those numbers were never stored, so they are left blank instead of guessed.
ALTER TABLE mm_matches ADD COLUMN scoring_config_id TEXT REFERENCES mm_scoring_configs(id);
ALTER TABLE mm_matches ADD COLUMN scoring_config_version INTEGER;

ALTER TABLE mm_match_snapshots ADD COLUMN scoring_config_id TEXT;
ALTER TABLE mm_match_snapshots ADD COLUMN scoring_config_version INTEGER;

-- The config a rule set scores with from now on. Nullable while the backfill runs.
ALTER TABLE mm_rule_sets ADD COLUMN scoring_config_id TEXT REFERENCES mm_scoring_configs(id);

-- One row per candidate returned by a search, so what the matchmaker was shown is recorded even when nothing was
-- decided. Deleting either profile removes the exposures that involved them.
CREATE TABLE IF NOT EXISTS mm_exposure (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    search_id             TEXT NOT NULL,             -- groups the rows of one search
    profile_id            TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    candidate_id          TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    rule_set_id           TEXT,
    rule_set_version      INTEGER,
    scoring_config_id     TEXT,
    scoring_config_version INTEGER,
    rank                  INTEGER NOT NULL,          -- 0-based position in the returned list
    shown                 INTEGER NOT NULL DEFAULT 1,
    score                 REAL,
    confidence            REAL,
    eligible              INTEGER,
    at                    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mm_exposure_search ON mm_exposure (search_id, rank);
CREATE INDEX IF NOT EXISTS idx_mm_exposure_profile ON mm_exposure (profile_id, at);
CREATE INDEX IF NOT EXISTS idx_mm_exposure_pair ON mm_exposure (profile_id, candidate_id);
