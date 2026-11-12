-- Profile reminders have no match. Preserve existing durable budget receipts.
ALTER TABLE mm_agent_messages RENAME TO mm_agent_messages_before_follow_ups;
CREATE TABLE mm_agent_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    match_id TEXT,
    actor TEXT NOT NULL,
    reserved_at INTEGER NOT NULL
);
INSERT INTO mm_agent_messages SELECT * FROM mm_agent_messages_before_follow_ups;
DROP TABLE mm_agent_messages_before_follow_ups;
CREATE INDEX idx_mm_agent_messages_budget ON mm_agent_messages(profile_id, reserved_at);

-- Lifetime reminder caps and spacing survive outbox retention. Count admitted
-- attempts conservatively even if delivery later fails or is cancelled.
CREATE TABLE mm_reminder_history (
    outbox_id INTEGER PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    match_id TEXT,
    kind TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_mm_reminder_history_subject ON mm_reminder_history(profile_id, match_id, kind);
INSERT INTO mm_reminder_history
SELECT id, profile_id, match_id, kind, created_at FROM mm_outbox
WHERE kind IN ('introduction','interest_buttons','intro_reminder','profile_reminder') AND status <> 'cancelled';
CREATE TRIGGER mm_reminder_history_admission AFTER INSERT ON mm_outbox
WHEN NEW.kind IN ('introduction','interest_buttons','intro_reminder','profile_reminder')
BEGIN
    INSERT INTO mm_reminder_history VALUES (NEW.id, NEW.profile_id, NEW.match_id, NEW.kind, NEW.created_at);
END;
