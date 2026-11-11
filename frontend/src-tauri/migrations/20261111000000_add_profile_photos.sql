-- Matchwise M11.1: profile photos. Bytes live on disk under `mm_photos/<profile_id>/`; this table only records
-- what is there and who owns it. Photos are stored exactly as received (no resizing, no EXIF removal).

CREATE TABLE IF NOT EXISTS mm_profile_photos (
    id                TEXT PRIMARY KEY,
    profile_id        TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    org_id            TEXT NOT NULL DEFAULT 'default',
    stored_name       TEXT,                     -- file name under mm_photos/<profile_id>/; NULL while pending
    original_name     TEXT,                     -- the name the file arrived with, for display only
    mime              TEXT NOT NULL,            -- image/jpeg | image/png | image/webp
    size_bytes        INTEGER NOT NULL,
    sha256            TEXT,                     -- content hash; NULL while pending (Telegram download not done)
    position          INTEGER NOT NULL,         -- display order within the profile, starting at 0
    source            TEXT NOT NULL DEFAULT 'upload',   -- upload | telegram
    status            TEXT NOT NULL DEFAULT 'ready',    -- ready | pending | failed
    telegram_file_id  TEXT,                     -- set for photos that arrived through Telegram
    created_at        TEXT NOT NULL,
    -- The same picture is kept once per person, however it arrives.
    UNIQUE (profile_id, sha256),
    -- Two photos cannot share a slot; ordering stays stable without relying on created_at.
    UNIQUE (profile_id, position)
);

CREATE INDEX IF NOT EXISTS idx_mm_profile_photos_profile ON mm_profile_photos (org_id, profile_id, position);
