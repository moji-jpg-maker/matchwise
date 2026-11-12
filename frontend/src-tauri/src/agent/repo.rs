use super::protocol::Action;
use crate::mm::repository::ORG;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Serialize)]
pub struct Run {
    pub id: i64,
    pub profile_id: String,
    pub batch_id: Option<String>,
    pub status: String,
    pub actor: String,
    pub created_at: String,
    pub finished_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Trace {
    pub id: i64,
    pub run_id: i64,
    pub turn: i64,
    pub action: Action,
    pub status: String,
    pub observation: Value,
    pub actor: String,
    pub decided_by: Option<String>,
    pub created_at: String,
    pub scheduled_at: Option<i64>,
    pub due_at: Option<i64>,
}

fn trace(row: sqlx::sqlite::SqliteRow) -> Result<Trace, String> {
    Ok(Trace {
        id: row.get("id"),
        run_id: row.get("run_id"),
        turn: row.get("turn"),
        action: serde_json::from_str(&row.get::<String, _>("action"))
            .map_err(|error| error.to_string())?,
        status: row.get("status"),
        observation: serde_json::from_str(&row.get::<String, _>("observation"))
            .map_err(|error| error.to_string())?,
        actor: row.get("actor"),
        decided_by: row.get("decided_by"),
        created_at: row.get("created_at"),
        scheduled_at: row.get("scheduled_at"),
        due_at: row.get("due_at"),
    })
}

pub async fn start(
    pool: &SqlitePool,
    profile_id: &str,
    batch_id: Option<&str>,
) -> Result<i64, String> {
    let mut transaction = pool.begin().await.map_err(|error| error.to_string())?;
    let id = sqlx::query("INSERT INTO mm_agent_runs (org_id, profile_id, batch_id, status, actor, created_at) VALUES (?, ?, ?, 'running', 'system', ?)")
        .bind(ORG).bind(profile_id).bind(batch_id).bind(Utc::now().to_rfc3339()).execute(&mut *transaction).await.map_err(|error| error.to_string())?.last_insert_rowid();
    sqlx::query("UPDATE mm_agent_runs SET actor = ? WHERE id = ?")
        .bind(format!("agent:{id}"))
        .bind(id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| error.to_string())?;
    transaction
        .commit()
        .await
        .map_err(|error| error.to_string())?;
    Ok(id)
}

pub async fn finish(pool: &SqlitePool, id: i64, status: &str) -> Result<(), String> {
    sqlx::query("UPDATE mm_agent_runs SET status = ?, finished_at = ? WHERE id = ? AND org_id = ?")
        .bind(status)
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .bind(ORG)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn runs(pool: &SqlitePool) -> Result<Vec<Run>, String> {
    let rows =
        sqlx::query("SELECT * FROM mm_agent_runs WHERE org_id = ? ORDER BY id DESC LIMIT 100")
            .bind(ORG)
            .fetch_all(pool)
            .await
            .map_err(|error| error.to_string())?;
    Ok(rows
        .into_iter()
        .map(|row| Run {
            id: row.get("id"),
            profile_id: row.get("profile_id"),
            batch_id: row.get("batch_id"),
            status: row.get("status"),
            actor: row.get("actor"),
            created_at: row.get("created_at"),
            finished_at: row.get("finished_at"),
        })
        .collect())
}

pub async fn traces(
    pool: &SqlitePool,
    run_id: Option<i64>,
    proposals: bool,
) -> Result<Vec<Trace>, String> {
    let rows = sqlx::query("SELECT s.* FROM mm_agent_steps s JOIN mm_agent_runs r ON r.id = s.run_id WHERE r.org_id = ? AND (? IS NULL OR s.run_id = ?) AND (? = 0 OR s.status IN ('proposed', 'scheduled')) ORDER BY s.id")
        .bind(ORG).bind(run_id).bind(run_id).bind(proposals).fetch_all(pool).await.map_err(|error| error.to_string())?;
    rows.into_iter().map(trace).collect()
}

/// Select one due receipt without loading the rest of the approvals inbox.
pub async fn next_due(pool: &SqlitePool, now: i64) -> Result<Option<Trace>, String> {
    sqlx::query("SELECT s.* FROM mm_agent_steps s JOIN mm_agent_runs r ON r.id=s.run_id WHERE r.org_id=? AND s.status='scheduled' AND s.due_at<=? ORDER BY s.due_at, s.id LIMIT 1")
        .bind(ORG).bind(now).fetch_optional(pool).await.map_err(|e| e.to_string())?
        .map(trace).transpose()
}

pub async fn record(
    pool: &SqlitePool,
    run_id: i64,
    turn: usize,
    action: &Action,
    status: &str,
    observation: &Value,
) -> Result<i64, String> {
    Ok(sqlx::query("INSERT INTO mm_agent_steps (run_id, turn, action, status, observation, actor, created_at, scheduled_at, due_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(run_id).bind(turn as i64).bind(serde_json::to_string(action).map_err(|error| error.to_string())?).bind(status).bind(observation.to_string()).bind(format!("agent:{run_id}")).bind(Utc::now().to_rfc3339()).bind(observation["scheduled_at"].as_i64()).bind(observation["due_at"].as_i64()).execute(pool).await.map_err(|error| error.to_string())?.last_insert_rowid())
}

pub async fn update(
    pool: &SqlitePool,
    id: i64,
    status: &str,
    observation: &Value,
) -> Result<(), String> {
    sqlx::query("UPDATE mm_agent_steps SET status = ?, observation = ? WHERE id = ?")
        .bind(status)
        .bind(observation.to_string())
        .bind(id)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub async fn claim(
    pool: &SqlitePool,
    id: i64,
    action: &Action,
    reject: bool,
) -> Result<(), String> {
    let changed = sqlx::query("UPDATE mm_agent_steps SET status = ?, action = ?, decided_by = 'matchmaker' WHERE id = ? AND status = 'proposed' AND run_id IN (SELECT id FROM mm_agent_runs WHERE org_id = ?)")
        .bind(if reject { "rejected" } else { "executing" }).bind(serde_json::to_string(action).map_err(|error| error.to_string())?).bind(id).bind(ORG).execute(pool).await.map_err(|error| error.to_string())?.rows_affected();
    if changed != 1 {
        return Err("Proposal already decided or not found".into());
    }
    Ok(())
}

pub async fn target(pool: &SqlitePool, run_id: i64) -> Result<String, String> {
    sqlx::query_scalar("SELECT profile_id FROM mm_agent_runs WHERE id = ? AND org_id = ?")
        .bind(run_id)
        .bind(ORG)
        .fetch_one(pool)
        .await
        .map_err(|error| error.to_string())
}
