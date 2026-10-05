use chrono::Utc;
use matchmaking_core::{FieldDef, FieldRegistry, Profile};
use sqlx::{Row, SqlitePool};

pub const ORG: &str = "default";

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
    /// Load the field registry, seeding the default field set the first time.
    pub async fn registry(pool: &SqlitePool) -> Result<FieldRegistry, sqlx::Error> {
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_field_definitions WHERE org_id = ?")
            .bind(ORG)
            .fetch_one(pool)
            .await?;
        if count == 0 {
            for (i, def) in matchmaking_core::default_registry().defs().enumerate() {
                Self::save_field(pool, def, i as i64).await?;
            }
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
