ALTER TABLE mm_ai_suggestions ADD COLUMN source TEXT NOT NULL DEFAULT 'ai';

CREATE TABLE IF NOT EXISTS mm_extraction_presets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    fields TEXT NOT NULL,
    with_preferences INTEGER NOT NULL DEFAULT 0,
    is_default INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
