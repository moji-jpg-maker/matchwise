//! Storage for the autonomy policy and for the fields the agent applied (M12 phase 1).

use chrono::Utc;
use matchmaking_core::autonomy::AutonomyPolicy;
use serde::Serialize;
use serde_json::Value as Json;
use sqlx::{Row, SqlitePool};

use super::repository::ORG;

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

/// One field the agent applied. `prior` is the entry the field had before (`None`: it was absent) and `applied` the
/// entry the agent stored, both as `{"value": ..., "source": ...}`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentAction {
    pub id: i64,
    pub profile_id: String,
    pub field: String,
    pub kind: String,
    pub prior: Option<Json>,
    pub applied: Json,
    pub suggestion_id: Option<i64>,
    pub actor: String,
    pub run_id: Option<i64>,
    pub undone_at: Option<String>,
    pub created_at: String,
}

const ACTION_COLS: &str = "id, profile_id, field, kind, prior, applied, suggestion_id, actor, run_id, undone_at, created_at";

fn parse_json(text: &str) -> Result<Json, sqlx::Error> {
    serde_json::from_str(text).map_err(|e| bad(e.to_string()))
}

fn row_to_action(row: &sqlx::sqlite::SqliteRow) -> Result<AgentAction, sqlx::Error> {
    let prior: Option<String> = row.get("prior");
    let applied: String = row.get("applied");
    Ok(AgentAction {
        id: row.get("id"),
        profile_id: row.get("profile_id"),
        field: row.get("field"),
        kind: row.get("kind"),
        prior: prior.as_deref().map(parse_json).transpose()?,
        applied: parse_json(&applied)?,
        suggestion_id: row.get("suggestion_id"),
        actor: row.get("actor"),
        run_id: row.get("run_id"),
        undone_at: row.get("undone_at"),
        created_at: row.get("created_at"),
    })
}

pub struct AutonomyRepo;

impl AutonomyRepo {
    /// The stored policy, or the L0 Manual preset when none was ever saved.
    pub async fn load_policy(pool: &SqlitePool) -> Result<AutonomyPolicy, sqlx::Error> {
        let row = sqlx::query("SELECT policy, version, paused FROM mm_agent_policy WHERE org_id = ?").bind(ORG).fetch_optional(pool).await?;
        let Some(row) = row else { return Ok(AutonomyPolicy::default()) };
        let text: String = row.get("policy");
        let mut policy: AutonomyPolicy = serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        policy.version = row.get::<i64, _>("version") as u32;
        policy.paused = row.get("paused");
        Ok(policy)
    }

    /// Validate and store a policy. The version is assigned here: the stored version plus one.
    pub async fn save_policy(pool: &SqlitePool, policy: &AutonomyPolicy) -> Result<AutonomyPolicy, String> {
        policy.validate()?;
        let current = Self::load_policy(pool).await.map_err(|e| e.to_string())?;
        let stored = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mm_agent_policy WHERE org_id = ?").bind(ORG).fetch_one(pool).await.map_err(|e| e.to_string())?;
        let mut next = policy.clone();
        next.version = if stored == 0 { 1 } else { current.version + 1 };
        let text = serde_json::to_string(&next).map_err(|e| e.to_string())?;
        sqlx::query(
            "INSERT INTO mm_agent_policy (org_id, version, policy, paused, updated_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(org_id) DO UPDATE SET version = excluded.version, policy = excluded.policy, paused = excluded.paused, updated_at = excluded.updated_at",
        )
        .bind(ORG)
        .bind(next.version as i64)
        .bind(text)
        .bind(next.paused)
        .bind(Utc::now().to_rfc3339())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(next)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_action(
        pool: &SqlitePool,
        profile_id: &str,
        field: &str,
        kind: &str,
        prior: Option<&Json>,
        applied: &Json,
        suggestion_id: Option<i64>,
        actor: &str,
        run_id: Option<i64>,
    ) -> Result<i64, sqlx::Error> {
        let result = sqlx::query("INSERT INTO mm_agent_actions (org_id, profile_id, field, kind, prior, applied, suggestion_id, actor, run_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(ORG)
            .bind(profile_id)
            .bind(field)
            .bind(kind)
            .bind(prior.map(|value| value.to_string()))
            .bind(applied.to_string())
            .bind(suggestion_id)
            .bind(actor)
            .bind(run_id)
            .bind(Utc::now().to_rfc3339())
            .execute(pool)
            .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn get_action(pool: &SqlitePool, id: i64) -> Result<Option<AgentAction>, sqlx::Error> {
        let row = sqlx::query(&format!("SELECT {ACTION_COLS} FROM mm_agent_actions WHERE id = ? AND org_id = ?")).bind(id).bind(ORG).fetch_optional(pool).await?;
        row.as_ref().map(row_to_action).transpose()
    }

    /// The actions on a profile that have not been undone, newest first.
    pub async fn active_actions(pool: &SqlitePool, profile_id: &str) -> Result<Vec<AgentAction>, sqlx::Error> {
        let rows = sqlx::query(&format!("SELECT {ACTION_COLS} FROM mm_agent_actions WHERE profile_id = ? AND org_id = ? AND undone_at IS NULL ORDER BY id DESC"))
            .bind(profile_id)
            .bind(ORG)
            .fetch_all(pool)
            .await?;
        rows.iter().map(row_to_action).collect()
    }

    pub async fn mark_undone(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE mm_agent_actions SET undone_at = ? WHERE id = ?").bind(Utc::now().to_rfc3339()).bind(id).execute(pool).await?;
        Ok(())
    }
}
