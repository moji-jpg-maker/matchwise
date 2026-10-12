-- Matchwise M7: the questionnaire and evidence registry. Instruments are versioned and never edited in place, so
-- every response can always be read against exactly the questions and scoring that produced it.
CREATE TABLE IF NOT EXISTS mm_instruments (
    org_id          TEXT NOT NULL DEFAULT 'default',
    key             TEXT NOT NULL,
    version         INTEGER NOT NULL,
    name            TEXT NOT NULL,
    kind            TEXT NOT NULL,            -- standard | custom
    dimension       TEXT NOT NULL,
    evidence_level  TEXT NOT NULL,            -- validated_instrument | adapted_instrument | matchmaker_defined
    definition      TEXT NOT NULL,            -- JSON Instrument (items, scale, subscales, evidence record)
    archived        INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,
    PRIMARY KEY (org_id, key, version)
);

CREATE TABLE IF NOT EXISTS mm_questionnaire_responses (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id          TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    instrument_key      TEXT NOT NULL,
    instrument_version  INTEGER NOT NULL,
    status              TEXT NOT NULL DEFAULT 'assigned',   -- assigned | in_progress | complete
    answers             TEXT NOT NULL DEFAULT '{}',          -- JSON {item id: value}
    scores              TEXT,                                -- JSON {subscale: score}, set when complete
    assigned_at         TEXT NOT NULL,
    started_at          TEXT,
    completed_at        TEXT
);
CREATE INDEX IF NOT EXISTS idx_mm_qresp_profile ON mm_questionnaire_responses (profile_id, status);
