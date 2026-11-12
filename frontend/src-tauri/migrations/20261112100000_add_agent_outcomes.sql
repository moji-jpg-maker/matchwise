-- Outcomes recorded by the agent must remain distinguishable from a person's label, so the export and
-- ranking code can filter or weight them. Historical outcomes were human-only.
ALTER TABLE mm_matches ADD COLUMN outcome_by TEXT;
UPDATE mm_matches SET outcome_by = 'matchmaker' WHERE outcome IS NOT NULL;
