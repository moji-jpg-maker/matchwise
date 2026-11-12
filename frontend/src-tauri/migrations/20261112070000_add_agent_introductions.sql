-- Attribution survives delivery; durable budget receipts survive outbox cleanup.
ALTER TABLE mm_outbox ADD COLUMN actor TEXT NOT NULL DEFAULT 'system';
CREATE TABLE mm_agent_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    match_id TEXT NOT NULL,
    actor TEXT NOT NULL,
    reserved_at INTEGER NOT NULL
);
CREATE INDEX idx_mm_agent_messages_budget ON mm_agent_messages(profile_id, reserved_at);
