CREATE TABLE mm_agent_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    batch_id TEXT,
    status TEXT NOT NULL,
    actor TEXT NOT NULL,
    created_at TEXT NOT NULL,
    finished_at TEXT
);

CREATE TABLE mm_agent_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER NOT NULL REFERENCES mm_agent_runs(id),
    turn INTEGER NOT NULL,
    action TEXT NOT NULL,
    status TEXT NOT NULL,
    observation TEXT NOT NULL,
    actor TEXT NOT NULL,
    decided_by TEXT,
    created_at TEXT NOT NULL,
    UNIQUE(run_id, turn)
);

CREATE INDEX idx_mm_agent_runs_org ON mm_agent_runs(org_id, id);
CREATE INDEX idx_mm_agent_steps_proposals ON mm_agent_steps(status, run_id);
