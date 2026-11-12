-- Historical overrides were human-only. Keep their attribution on upgrade.
ALTER TABLE mm_matches ADD COLUMN override_by TEXT;
UPDATE mm_matches SET override_by = 'matchmaker' WHERE override_reason IS NOT NULL;

-- Undo restores the override state from before the recommendation/review.
ALTER TABLE mm_agent_transition_undo ADD COLUMN prior_override_reason TEXT;
ALTER TABLE mm_agent_transition_undo ADD COLUMN prior_override_by TEXT;
UPDATE mm_agent_transition_undo SET
    prior_override_reason = (SELECT m.override_reason FROM mm_match_events e JOIN mm_matches m ON m.id = e.match_id WHERE e.id = event_id),
    prior_override_by = (SELECT m.override_by FROM mm_match_events e JOIN mm_matches m ON m.id = e.match_id WHERE e.id = event_id);
