-- Reserve before sending a request: failures, cancellation and retries consume a call.
-- Independent of prompt retention and survives application restarts.
CREATE TABLE mm_agent_model_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id TEXT NOT NULL,
    run_id INTEGER NOT NULL REFERENCES mm_agent_runs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    started_at INTEGER NOT NULL
);
CREATE INDEX idx_agent_model_calls_window ON mm_agent_model_calls(org_id, started_at);
