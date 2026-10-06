-- Matchwise M2: versioned rule sets ("matchmaking programs"). Every save creates a new immutable version so a
-- recommendation can record exactly which rules produced it.
CREATE TABLE IF NOT EXISTS mm_rule_sets (
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

CREATE TABLE IF NOT EXISTS mm_rule_set_versions (
    rule_set_id  TEXT NOT NULL REFERENCES mm_rule_sets(id) ON DELETE CASCADE,
    version      INTEGER NOT NULL,
    definition   TEXT NOT NULL,       -- JSON RuleSet
    note         TEXT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (rule_set_id, version)
);
