use crate::mm::repository::ORG;
use chrono::Utc;
use matchmaking_core::{builtin_mini_ipip, Instrument};
use serde_json::Value as Json;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Debug, Clone)]
pub struct InstrumentRow {
    pub instrument: Instrument,
    pub archived: bool,
    pub created_at: String,
}

fn row_to_instrument(r: &sqlx::sqlite::SqliteRow) -> Result<InstrumentRow, sqlx::Error> {
    let def: String = r.get("definition");
    Ok(InstrumentRow {
        instrument: serde_json::from_str(&def).map_err(|e| bad(e.to_string()))?,
        archived: r.get::<i64, _>("archived") != 0,
        created_at: r.get("created_at"),
    })
}

/// Make sure the instrument shipped with the app is present (once; archiving it later is respected).
pub async fn ensure_builtin(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let have: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_instruments WHERE org_id = ? AND key = 'mini_ipip'").bind(ORG).fetch_one(pool).await?;
    if have == 0 {
        insert_instrument(pool, &builtin_mini_ipip()).await?;
    }
    Ok(())
}

pub async fn insert_instrument(pool: &SqlitePool, i: &Instrument) -> Result<(), sqlx::Error> {
    let level = serde_json::to_value(i.evidence.level).map_err(|e| bad(e.to_string()))?.as_str().unwrap_or("").to_string();
    sqlx::query("INSERT INTO mm_instruments (org_id, key, version, name, kind, dimension, evidence_level, definition, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(ORG).bind(&i.key).bind(i.version as i64).bind(&i.name).bind(&i.kind).bind(&i.dimension).bind(level)
        .bind(serde_json::to_string(i).map_err(|e| bad(e.to_string()))?).bind(now())
        .execute(pool).await?;
    Ok(())
}

/// Every stored version, newest first within a key.
pub async fn list_instruments(pool: &SqlitePool, include_archived: bool) -> Result<Vec<InstrumentRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT definition, archived, created_at FROM mm_instruments WHERE org_id = ? AND (? OR archived = 0) ORDER BY key, version DESC")
        .bind(ORG).bind(include_archived).fetch_all(pool).await?;
    rows.iter().map(row_to_instrument).collect()
}

pub async fn get_instrument(pool: &SqlitePool, key: &str, version: Option<u32>) -> Result<Option<InstrumentRow>, sqlx::Error> {
    let row = match version {
        Some(v) => sqlx::query("SELECT definition, archived, created_at FROM mm_instruments WHERE org_id = ? AND key = ? AND version = ?").bind(ORG).bind(key).bind(v as i64).fetch_optional(pool).await?,
        None => sqlx::query("SELECT definition, archived, created_at FROM mm_instruments WHERE org_id = ? AND key = ? ORDER BY version DESC LIMIT 1").bind(ORG).bind(key).fetch_optional(pool).await?,
    };
    row.as_ref().map(row_to_instrument).transpose()
}

pub async fn set_archived(pool: &SqlitePool, key: &str, archived: bool) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("UPDATE mm_instruments SET archived = ? WHERE org_id = ? AND key = ?").bind(archived).bind(ORG).bind(key).execute(pool).await?.rows_affected())
}

#[derive(Debug, Clone)]
pub struct ResponseRow {
    pub id: i64,
    pub profile_id: String,
    pub instrument_key: String,
    pub instrument_version: u32,
    pub status: String,
    pub answers: BTreeMap<String, i32>,
    pub scores: Option<BTreeMap<String, f64>>,
    pub assigned_at: String,
    pub completed_at: Option<String>,
}

fn row_to_response(r: &sqlx::sqlite::SqliteRow) -> Result<ResponseRow, sqlx::Error> {
    let answers: String = r.get("answers");
    let scores: Option<String> = r.get("scores");
    Ok(ResponseRow {
        id: r.get("id"),
        profile_id: r.get("profile_id"),
        instrument_key: r.get("instrument_key"),
        instrument_version: r.get::<i64, _>("instrument_version") as u32,
        status: r.get("status"),
        answers: serde_json::from_str(&answers).map_err(|e| bad(e.to_string()))?,
        scores: scores.map(|s| serde_json::from_str(&s).map_err(|e| bad(e.to_string()))).transpose()?,
        assigned_at: r.get("assigned_at"),
        completed_at: r.get("completed_at"),
    })
}

const RESP_COLS: &str = "id, profile_id, instrument_key, instrument_version, status, answers, scores, assigned_at, completed_at";

pub async fn get_response(pool: &SqlitePool, id: i64) -> Result<Option<ResponseRow>, sqlx::Error> {
    let r = sqlx::query(&format!("SELECT {RESP_COLS} FROM mm_questionnaire_responses WHERE id = ?")).bind(id).fetch_optional(pool).await?;
    r.as_ref().map(row_to_response).transpose()
}

pub async fn responses_for_profile(pool: &SqlitePool, profile_id: &str) -> Result<Vec<ResponseRow>, sqlx::Error> {
    let rows = sqlx::query(&format!("SELECT {RESP_COLS} FROM mm_questionnaire_responses WHERE profile_id = ? ORDER BY id DESC")).bind(profile_id).fetch_all(pool).await?;
    rows.iter().map(row_to_response).collect()
}

pub async fn open_responses(pool: &SqlitePool, profile_id: &str) -> Result<Vec<ResponseRow>, sqlx::Error> {
    let rows = sqlx::query(&format!("SELECT {RESP_COLS} FROM mm_questionnaire_responses WHERE profile_id = ? AND status != 'complete' ORDER BY id")).bind(profile_id).fetch_all(pool).await?;
    rows.iter().map(row_to_response).collect()
}

/// One open response per instrument and person: assigning again while one is open is refused by the caller.
pub async fn assign(pool: &SqlitePool, profile_id: &str, key: &str, version: u32) -> Result<i64, sqlx::Error> {
    let r = sqlx::query("INSERT INTO mm_questionnaire_responses (profile_id, instrument_key, instrument_version, assigned_at) VALUES (?, ?, ?, ?)")
        .bind(profile_id).bind(key).bind(version as i64).bind(now()).execute(pool).await?;
    Ok(r.last_insert_rowid())
}

pub async fn save_answers(pool: &SqlitePool, id: i64, answers: &BTreeMap<String, i32>) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_questionnaire_responses SET answers = ?, status = 'in_progress', started_at = COALESCE(started_at, ?) WHERE id = ? AND status != 'complete'")
        .bind(serde_json::to_string(answers).map_err(|e| bad(e.to_string()))?).bind(now()).bind(id).execute(pool).await?;
    Ok(())
}

pub async fn complete(pool: &SqlitePool, id: i64, answers: &BTreeMap<String, i32>, scores: &BTreeMap<String, f64>) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_questionnaire_responses SET answers = ?, scores = ?, status = 'complete', started_at = COALESCE(started_at, ?), completed_at = ? WHERE id = ?")
        .bind(serde_json::to_string(answers).map_err(|e| bad(e.to_string()))?).bind(serde_json::to_string(scores).map_err(|e| bad(e.to_string()))?).bind(now()).bind(now()).bind(id)
        .execute(pool).await?;
    Ok(())
}

pub async fn delete_response(pool: &SqlitePool, id: i64) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM mm_questionnaire_responses WHERE id = ?").bind(id).execute(pool).await?.rows_affected())
}

pub fn _json(_: &Json) {}
