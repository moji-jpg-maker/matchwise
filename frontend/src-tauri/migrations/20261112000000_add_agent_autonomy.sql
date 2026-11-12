-- Matchwise M12 phase 1: the autonomy policy and the record of what the agent changed.
--
-- `mm_agent_policy` holds one row per organisation: the policy as JSON plus the kill switch as its own column so
-- it can be read without parsing. No row means the L0 Manual preset, so an upgrade changes nothing.
--
-- `mm_agent_actions` is one row per field the agent applied. `prior` is the value the field had before as JSON
-- (`{"value": ..., "source": ...}`), NULL when the field was absent, so an undo can restore or remove it.
-- `actor` is `agent:<run_id>`; phase 1 writes `agent:0` for its deterministic step, and `run_id` stays NULL until
-- phase 2 adds real runs.

CREATE TABLE IF NOT EXISTS mm_agent_policy (
    org_id TEXT PRIMARY KEY,
    version INTEGER NOT NULL,
    policy TEXT NOT NULL,
    paused INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mm_agent_actions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    field TEXT NOT NULL,
    kind TEXT NOT NULL,
    prior TEXT,
    applied TEXT NOT NULL,
    suggestion_id INTEGER,
    actor TEXT NOT NULL,
    run_id INTEGER,
    undone_at TEXT,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mm_agent_actions_profile ON mm_agent_actions (profile_id, field, id);
