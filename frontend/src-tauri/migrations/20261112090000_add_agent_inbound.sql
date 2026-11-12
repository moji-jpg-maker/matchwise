-- Candidate free-text replies waiting to be interpreted by the agent. A row is written only when the
-- reply step is on and the AI gate is open, and it is deleted as soon as a run has shown the text to the
-- model. The text is never written to mm_audit_log; the everyday conversation stays in mm_telegram_messages.
CREATE TABLE IF NOT EXISTS mm_agent_inbound (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id     TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    text       TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mm_agent_inbound_profile ON mm_agent_inbound (profile_id, id);
