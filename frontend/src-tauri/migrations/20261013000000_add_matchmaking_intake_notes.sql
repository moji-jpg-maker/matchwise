-- Matchwise M8: bulk profile intake. The raw text a profile was created from is kept here so the source is never
-- lost. It is deliberately NOT a profile field: it is never exported as field data, never scored, and a model may
-- only ever read it through the same redacted `extra` path the per-profile notes box uses.

-- One row per created profile. `profile_id` is unique because a profile has at most one intake note.
CREATE TABLE IF NOT EXISTS mm_intake_notes (
    profile_id  TEXT PRIMARY KEY REFERENCES mm_profiles(id) ON DELETE CASCADE,
    org_id      TEXT NOT NULL DEFAULT 'default',
    batch_id    TEXT NOT NULL,                  -- client-generated id grouping one bulk run
    note_text   TEXT NOT NULL,                  -- the raw entry, exactly as the matchmaker supplied it
    label       TEXT NOT NULL,                  -- "Entry N" or a name the matchmaker gave the entry
    created_at  TEXT NOT NULL
);

-- The report for a batch reads every entry it created in one go.
CREATE INDEX IF NOT EXISTS idx_mm_intake_notes_batch ON mm_intake_notes (org_id, batch_id);
