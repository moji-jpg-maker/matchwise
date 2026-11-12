-- Durable review receipts. The anchor is fixed when scheduled; current policy
-- may lengthen a window, but cannot shorten the promised veto period.
ALTER TABLE mm_agent_steps ADD COLUMN scheduled_at INTEGER;
ALTER TABLE mm_agent_steps ADD COLUMN due_at INTEGER;
CREATE INDEX idx_mm_agent_steps_due ON mm_agent_steps(status, due_at, id);
