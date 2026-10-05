-- Matchwise M1b: partner preferences and a small key/value table for seeding state.
CREATE TABLE IF NOT EXISTS mm_meta (
    org_id  TEXT NOT NULL DEFAULT 'default',
    key     TEXT NOT NULL,
    value   TEXT NOT NULL,
    PRIMARY KEY (org_id, key)
);

-- One row per profile holding that person's partner preferences as a JSON list.
-- Deleting a profile deletes its preferences (data deletion must be complete).
CREATE TABLE IF NOT EXISTS mm_preferences (
    profile_id  TEXT PRIMARY KEY REFERENCES mm_profiles(id) ON DELETE CASCADE,
    org_id      TEXT NOT NULL DEFAULT 'default',
    data        TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
