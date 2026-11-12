-- Durable, coalesced work. Generation prevents an event arriving during a run
-- from being removed when that run acknowledges its original work item.
CREATE TABLE mm_agent_work (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id TEXT NOT NULL,
    profile_id TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    batch_id TEXT,
    generation INTEGER NOT NULL DEFAULT 1,
    UNIQUE(org_id, profile_id)
);

-- All validated lifecycle paths (including Telegram responses and undo) update
-- this column inside their transaction. Rollbacks also roll back the work item.
CREATE TRIGGER mm_agent_match_status_work
AFTER UPDATE OF status ON mm_matches
WHEN OLD.status <> NEW.status
BEGIN
    INSERT INTO mm_agent_work(org_id, profile_id)
    VALUES (NEW.org_id, NEW.profile_a)
    ON CONFLICT(org_id, profile_id) DO UPDATE SET generation = generation + 1;
    INSERT INTO mm_agent_work(org_id, profile_id)
    VALUES (NEW.org_id, NEW.profile_b)
    ON CONFLICT(org_id, profile_id) DO UPDATE SET generation = generation + 1;
END;
