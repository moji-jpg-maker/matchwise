use chrono::Utc;
use matchmaking_core::{FieldDef, FieldRegistry, Preference, Profile};
use sqlx::{Row, SqlitePool};

pub const ORG: &str = "default";

/// Version of the default field set. v1 = first M1 release (no `children`), v2 = adds `children`, curated order.
const FIELD_SEED_VERSION: i64 = 2;

pub struct MmRepository;

pub struct StoredProfile {
    pub profile: Profile,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

impl MmRepository {
    /// Load the field registry. Default fields are seeded (insert-or-ignore, so a matchmaker's edits to an
    /// existing field are never overwritten) whenever the stored seed version is older than
    /// `FIELD_SEED_VERSION`. Bump that constant when the default set gains a field.
    pub async fn registry(pool: &SqlitePool) -> Result<FieldRegistry, sqlx::Error> {
        let seeded: i64 = sqlx::query_scalar::<_, String>("SELECT value FROM mm_meta WHERE org_id = ? AND key = 'fields_seed_version'")
            .bind(ORG)
            .fetch_optional(pool)
            .await?
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        if seeded < FIELD_SEED_VERSION {
            let defaults = matchmaking_core::default_registry();
            let mut tx = pool.begin().await?;
            for (i, def) in defaults.defs().enumerate() {
                let json = serde_json::to_string(def).map_err(|e| bad(e.to_string()))?;
                sqlx::query("INSERT OR IGNORE INTO mm_field_definitions (org_id, key, definition, sort_order) VALUES (?, ?, ?, ?)")
                    .bind(ORG)
                    .bind(&def.key)
                    .bind(json)
                    .bind(i as i64)
                    .execute(&mut *tx)
                    .await?;
                // Default fields follow the curated order (earlier versions sorted them alphabetically).
                sqlx::query("UPDATE mm_field_definitions SET sort_order = ? WHERE org_id = ? AND key = ?")
                    .bind(i as i64)
                    .bind(ORG)
                    .bind(&def.key)
                    .execute(&mut *tx)
                    .await?;
            }
            // Matchmaker-defined fields come after the defaults.
            sqlx::query("UPDATE mm_field_definitions SET sort_order = sort_order + 1000 WHERE org_id = ? AND sort_order < 1000 AND key NOT IN (SELECT value FROM json_each(?))")
                .bind(ORG)
                .bind(serde_json::to_string(&defaults.keys().collect::<Vec<_>>()).map_err(|e| bad(e.to_string()))?)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO mm_meta (org_id, key, value) VALUES (?, 'fields_seed_version', ?) ON CONFLICT(org_id, key) DO UPDATE SET value = excluded.value")
                .bind(ORG)
                .bind(FIELD_SEED_VERSION.to_string())
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        let rows = sqlx::query("SELECT definition FROM mm_field_definitions WHERE org_id = ? ORDER BY sort_order, key")
            .bind(ORG)
            .fetch_all(pool)
            .await?;
        let mut reg = FieldRegistry::new();
        for r in rows {
            let json: String = r.get("definition");
            let def: FieldDef = serde_json::from_str(&json).map_err(|e| bad(e.to_string()))?;
            reg.register(def);
        }
        Ok(reg)
    }

    pub async fn save_field(pool: &SqlitePool, def: &FieldDef, sort_order: i64) -> Result<(), sqlx::Error> {
        let json = serde_json::to_string(def).map_err(|e| bad(e.to_string()))?;
        sqlx::query(
            "INSERT INTO mm_field_definitions (org_id, key, definition, sort_order) VALUES (?, ?, ?, ?)
             ON CONFLICT(org_id, key) DO UPDATE SET definition = excluded.definition",
        )
        .bind(ORG)
        .bind(&def.key)
        .bind(json)
        .bind(sort_order)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn next_field_order(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
        let n: Option<i64> = sqlx::query_scalar("SELECT max(sort_order) FROM mm_field_definitions WHERE org_id = ?")
            .bind(ORG)
            .fetch_one(pool)
            .await?;
        Ok(n.unwrap_or(-1) + 1)
    }

    pub async fn insert_profile(pool: &SqlitePool, profile: &Profile) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let data = serde_json::to_string(&profile.fields).map_err(|e| bad(e.to_string()))?;
        sqlx::query("INSERT INTO mm_profiles (id, org_id, data, status, created_at, updated_at) VALUES (?, ?, ?, 'active', ?, ?)")
            .bind(&profile.id)
            .bind(ORG)
            .bind(data)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_profile_data(pool: &SqlitePool, profile: &Profile) -> Result<(), sqlx::Error> {
        let data = serde_json::to_string(&profile.fields).map_err(|e| bad(e.to_string()))?;
        sqlx::query("UPDATE mm_profiles SET data = ?, updated_at = ? WHERE id = ? AND org_id = ?")
            .bind(data)
            .bind(Utc::now().to_rfc3339())
            .bind(&profile.id)
            .bind(ORG)
            .execute(pool)
            .await?;
        Ok(())
    }

    fn from_row(r: &sqlx::sqlite::SqliteRow) -> Result<StoredProfile, sqlx::Error> {
        let id: String = r.get("id");
        let data: String = r.get("data");
        let fields = serde_json::from_str(&data).map_err(|e| bad(format!("profile {id}: {e}")))?;
        Ok(StoredProfile {
            profile: Profile { id, fields },
            status: r.get("status"),
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        })
    }

    pub async fn get_profile(pool: &SqlitePool, id: &str) -> Result<Option<StoredProfile>, sqlx::Error> {
        let row = sqlx::query("SELECT id, data, status, created_at, updated_at FROM mm_profiles WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(ORG)
            .fetch_optional(pool)
            .await?;
        row.as_ref().map(Self::from_row).transpose()
    }

    pub async fn list_profiles(pool: &SqlitePool, include_deactivated: bool) -> Result<Vec<StoredProfile>, sqlx::Error> {
        let sql = if include_deactivated {
            "SELECT id, data, status, created_at, updated_at FROM mm_profiles WHERE org_id = ? ORDER BY updated_at DESC"
        } else {
            "SELECT id, data, status, created_at, updated_at FROM mm_profiles WHERE org_id = ? AND status = 'active' ORDER BY updated_at DESC"
        };
        let rows = sqlx::query(sql).bind(ORG).fetch_all(pool).await?;
        rows.iter().map(Self::from_row).collect()
    }

    pub async fn set_status(pool: &SqlitePool, id: &str, status: &str) -> Result<u64, sqlx::Error> {
        let r = sqlx::query("UPDATE mm_profiles SET status = ?, updated_at = ? WHERE id = ? AND org_id = ?")
            .bind(status)
            .bind(Utc::now().to_rfc3339())
            .bind(id)
            .bind(ORG)
            .execute(pool)
            .await?;
        Ok(r.rows_affected())
    }

    pub async fn delete_profile(pool: &SqlitePool, id: &str) -> Result<u64, sqlx::Error> {
        let r = sqlx::query("DELETE FROM mm_profiles WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(ORG)
            .execute(pool)
            .await?;
        Ok(r.rows_affected())
    }

    pub async fn get_preferences(pool: &SqlitePool, profile_id: &str) -> Result<Vec<Preference>, sqlx::Error> {
        let data: Option<String> = sqlx::query_scalar("SELECT data FROM mm_preferences WHERE profile_id = ? AND org_id = ?")
            .bind(profile_id)
            .bind(ORG)
            .fetch_optional(pool)
            .await?;
        match data {
            Some(d) => serde_json::from_str(&d).map_err(|e| bad(format!("preferences of {profile_id}: {e}"))),
            None => Ok(vec![]),
        }
    }

    pub async fn save_preferences(pool: &SqlitePool, profile_id: &str, prefs: &[Preference]) -> Result<(), sqlx::Error> {
        let data = serde_json::to_string(prefs).map_err(|e| bad(e.to_string()))?;
        sqlx::query(
            "INSERT INTO mm_preferences (profile_id, org_id, data, updated_at) VALUES (?, ?, ?, ?)
             ON CONFLICT(profile_id) DO UPDATE SET data = excluded.data, updated_at = excluded.updated_at",
        )
        .bind(profile_id)
        .bind(ORG)
        .bind(data)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Append an audit entry. `detail` must never contain field values.
    pub async fn audit(pool: &SqlitePool, action: &str, entity: &str, entity_id: &str, detail: Option<&str>) {
        let res = sqlx::query("INSERT INTO mm_audit_log (org_id, actor, action, entity, entity_id, detail, at) VALUES (?, 'matchmaker', ?, ?, ?, ?, ?)")
            .bind(ORG)
            .bind(action)
            .bind(entity)
            .bind(entity_id)
            .bind(detail)
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await;
        if let Err(e) = res {
            log::warn!("audit log write failed: {e}");
        }
    }
}
