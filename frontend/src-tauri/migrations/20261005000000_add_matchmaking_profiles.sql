-- Matchwise M1: schema-driven profiles. Field definitions and profile values are stored as JSON so
-- matchmakers can add fields without a migration. org_id is on every table for later multi-tenant use.
CREATE TABLE IF NOT EXISTS mm_field_definitions (
    org_id      TEXT NOT NULL DEFAULT 'default',
    key         TEXT NOT NULL,
    definition  TEXT NOT NULL,              -- JSON FieldDef
    sort_order  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (org_id, key)
);

CREATE TABLE IF NOT EXISTS mm_profiles (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL DEFAULT 'default',
    data        TEXT NOT NULL,              -- JSON map: field key -> {value, source}
    status      TEXT NOT NULL DEFAULT 'active',   -- active | deactivated
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mm_profiles_org_status ON mm_profiles (org_id, status);

-- Audit trail. Records WHICH fields changed, never their values (profiles hold sensitive data).
CREATE TABLE IF NOT EXISTS mm_audit_log (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    org_id      TEXT NOT NULL DEFAULT 'default',
    actor       TEXT NOT NULL,
    action      TEXT NOT NULL,
    entity      TEXT NOT NULL,
    entity_id   TEXT NOT NULL,
    detail      TEXT,
    at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mm_audit_entity ON mm_audit_log (entity, entity_id);
