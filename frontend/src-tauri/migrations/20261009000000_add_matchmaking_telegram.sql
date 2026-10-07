-- Matchwise M5: Telegram adapter. Everything here belongs to a profile and is deleted with it.

-- One Telegram chat per profile (and one profile per chat).
CREATE TABLE IF NOT EXISTS mm_telegram_links (
    profile_id             TEXT PRIMARY KEY REFERENCES mm_profiles(id) ON DELETE CASCADE,
    chat_id                INTEGER NOT NULL UNIQUE,
    telegram_user_id       INTEGER NOT NULL,
    username               TEXT,
    linked_at              TEXT NOT NULL,
    consent_version        INTEGER,
    consented_at           TEXT,
    notifications_enabled  INTEGER NOT NULL DEFAULT 1
);

-- One-time invite codes. Only a hash is stored: the code is shown to the matchmaker once.
CREATE TABLE IF NOT EXISTS mm_telegram_invites (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    code_hash   TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL,
    expires_at  TEXT NOT NULL,
    used_at     TEXT,
    revoked     INTEGER NOT NULL DEFAULT 0
);

-- Failed code attempts per chat (guessing protection).
CREATE TABLE IF NOT EXISTS mm_telegram_attempts (
    chat_id  INTEGER NOT NULL,
    at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mm_telegram_attempts ON mm_telegram_attempts (chat_id, at);

-- Conversation state of a guided flow (editing the profile, adding a preference).
CREATE TABLE IF NOT EXISTS mm_telegram_state (
    chat_id     INTEGER PRIMARY KEY,
    state       TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

-- Messages waiting to be sent. Rendered text is kept so the matchmaker can see exactly what was sent.
CREATE TABLE IF NOT EXISTS mm_outbox (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id       TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL,
    text             TEXT NOT NULL,
    keyboard         TEXT,
    match_id         TEXT,
    status           TEXT NOT NULL DEFAULT 'pending',   -- pending | sent | failed | cancelled
    attempts         INTEGER NOT NULL DEFAULT 0,
    next_attempt_at  TEXT NOT NULL,
    created_at       TEXT NOT NULL,
    sent_at          TEXT,
    last_error       TEXT
);
CREATE INDEX IF NOT EXISTS idx_mm_outbox_due ON mm_outbox (status, next_attempt_at);
CREATE INDEX IF NOT EXISTS idx_mm_outbox_profile ON mm_outbox (profile_id, kind);

-- The conversation between a candidate and the matchmaker.
CREATE TABLE IF NOT EXISTS mm_telegram_messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    direction   TEXT NOT NULL,                          -- in | out
    text        TEXT NOT NULL,
    match_id    TEXT,
    created_at  TEXT NOT NULL,
    is_read     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_mm_telegram_messages ON mm_telegram_messages (profile_id, id);

-- Requests a candidate made through the bot (for example "delete my data"); the matchmaker resolves them in the app.
CREATE TABLE IF NOT EXISTS mm_data_requests (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id  TEXT NOT NULL REFERENCES mm_profiles(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,                          -- deletion
    status      TEXT NOT NULL DEFAULT 'open',           -- open | done
    created_at  TEXT NOT NULL,
    resolved_at TEXT
);
