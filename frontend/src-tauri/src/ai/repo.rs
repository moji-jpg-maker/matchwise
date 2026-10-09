use super::settings::Mode;
use crate::mm::repository::ORG;
use chrono::{Duration, Utc};
use serde::Serialize;
use serde_json::Value as Json;
use sqlx::{Row, SqlitePool};

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

#[derive(Debug, Clone, Serialize)]
pub struct SuggestionRow {
    pub id: i64,
    pub profile_id: String,
    pub kind: String,
    pub field: String,
    pub payload: Json,
    pub evidence: String,
    pub confidence: f64,
    pub conflict: bool,
    pub status: String,
    pub model: String,
    pub created_at: String,
}

fn row_to_suggestion(r: &sqlx::sqlite::SqliteRow) -> Result<SuggestionRow, sqlx::Error> {
    let payload: String = r.get("payload");
    Ok(SuggestionRow {
        id: r.get("id"),
        profile_id: r.get("profile_id"),
        kind: r.get("kind"),
        field: r.get("field"),
        payload: serde_json::from_str(&payload).map_err(|e| bad(e.to_string()))?,
        evidence: r.get("evidence"),
        confidence: r.get("confidence"),
        conflict: r.get::<i64, _>("conflict") != 0,
        status: r.get("status"),
        model: r.get("model"),
        created_at: r.get("created_at"),
    })
}

const SUGG_COLS: &str = "id, profile_id, kind, field, payload, evidence, confidence, conflict, status, model, created_at";

pub struct RunRow {
    pub id: i64,
    pub provider: String,
    pub model: String,
    pub is_cloud: bool,
    pub input_hash: String,
    pub rule_set_version: Option<i64>,
    pub result: Json,
    pub created_at: String,
}

fn row_to_run(r: &sqlx::sqlite::SqliteRow) -> Result<RunRow, sqlx::Error> {
    let result: String = r.get("result");
    Ok(RunRow {
        id: r.get("id"),
        provider: r.get("provider"),
        model: r.get("model"),
        is_cloud: r.get::<i64, _>("is_cloud") != 0,
        input_hash: r.get("input_hash"),
        rule_set_version: r.get("rule_set_version"),
        result: serde_json::from_str(&result).map_err(|e| bad(e.to_string()))?,
        created_at: r.get("created_at"),
    })
}

pub async fn insert_run(
    pool: &SqlitePool,
    profile_id: Option<&str>,
    match_id: Option<&str>,
    kind: &str,
    provider: &str,
    model: &str,
    is_cloud: bool,
    input_hash: &str,
    rule_set_version: Option<u32>,
    result: &Json,
) -> Result<i64, sqlx::Error> {
    let r = sqlx::query("INSERT INTO mm_ai_runs (profile_id, match_id, kind, provider, model, is_cloud, input_hash, rule_set_version, result, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(profile_id).bind(match_id).bind(kind).bind(provider).bind(model).bind(is_cloud).bind(input_hash).bind(rule_set_version.map(|v| v as i64)).bind(result.to_string()).bind(now())
        .execute(pool).await?;
    Ok(r.last_insert_rowid())
}

pub async fn latest_profile_run(pool: &SqlitePool, profile_id: &str) -> Result<Option<RunRow>, sqlx::Error> {
    let r = sqlx::query("SELECT id, provider, model, is_cloud, input_hash, rule_set_version, result, created_at FROM mm_ai_runs WHERE profile_id = ? AND kind = 'extract' ORDER BY id DESC LIMIT 1")
        .bind(profile_id).fetch_optional(pool).await?;
    r.as_ref().map(row_to_run).transpose()
}

pub async fn latest_match_run(pool: &SqlitePool, match_id: &str) -> Result<Option<RunRow>, sqlx::Error> {
    let r = sqlx::query("SELECT id, provider, model, is_cloud, input_hash, rule_set_version, result, created_at FROM mm_ai_runs WHERE match_id = ? AND kind = 'pair' ORDER BY id DESC LIMIT 1")
        .bind(match_id).fetch_optional(pool).await?;
    r.as_ref().map(row_to_run).transpose()
}

/// A new extraction replaces the previous pending suggestions (accepted and rejected ones are history and stay).
pub async fn supersede_pending(pool: &SqlitePool, profile_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_ai_suggestions SET status = 'superseded', decided_at = ? WHERE profile_id = ? AND status = 'pending'").bind(now()).bind(profile_id).execute(pool).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_suggestion(
    pool: &SqlitePool,
    profile_id: &str,
    run_id: i64,
    kind: &str,
    field: &str,
    payload: &Json,
    evidence: &str,
    confidence: f64,
    conflict: bool,
    model: &str,
) -> Result<i64, sqlx::Error> {
    let r = sqlx::query("INSERT INTO mm_ai_suggestions (profile_id, run_id, kind, field, payload, evidence, confidence, conflict, model, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(profile_id).bind(run_id).bind(kind).bind(field).bind(payload.to_string()).bind(evidence).bind(confidence).bind(conflict).bind(model).bind(now())
        .execute(pool).await?;
    Ok(r.last_insert_rowid())
}

pub async fn suggestions_for_run(pool: &SqlitePool, run_id: i64) -> Result<Vec<SuggestionRow>, sqlx::Error> {
    let rows = sqlx::query(&format!("SELECT {SUGG_COLS} FROM mm_ai_suggestions WHERE run_id = ? ORDER BY id")).bind(run_id).fetch_all(pool).await?;
    rows.iter().map(row_to_suggestion).collect()
}

pub async fn pending_suggestions(pool: &SqlitePool, profile_id: &str) -> Result<Vec<SuggestionRow>, sqlx::Error> {
    let rows = sqlx::query(&format!("SELECT {SUGG_COLS} FROM mm_ai_suggestions WHERE profile_id = ? AND status = 'pending' ORDER BY id")).bind(profile_id).fetch_all(pool).await?;
    rows.iter().map(row_to_suggestion).collect()
}

pub async fn get_suggestion(pool: &SqlitePool, id: i64) -> Result<Option<SuggestionRow>, sqlx::Error> {
    let r = sqlx::query(&format!("SELECT {SUGG_COLS} FROM mm_ai_suggestions WHERE id = ?")).bind(id).fetch_optional(pool).await?;
    r.as_ref().map(row_to_suggestion).transpose()
}

pub async fn set_suggestion_status(pool: &SqlitePool, id: i64, status: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_ai_suggestions SET status = ?, decided_at = ? WHERE id = ?").bind(status).bind(now()).bind(id).execute(pool).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct LogRow {
    pub at: String,
    pub kind: String,
    pub provider: String,
    pub model: String,
    pub is_cloud: bool,
    pub include_sensitive: bool,
    pub input_chars: i64,
    pub output_chars: i64,
    pub ok: bool,
    pub error: Option<String>,
    /// The exact (redacted) text that was sent: kept for cloud calls only, for 30 days.
    pub prompt: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn log_call(
    pool: &SqlitePool,
    kind: &str,
    provider: &str,
    model: &str,
    mode: Mode,
    profile_a: Option<&str>,
    profile_b: Option<&str>,
    input_chars: usize,
    output_chars: usize,
    ok: bool,
    error: Option<&str>,
    prompt: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO mm_ai_log (at, kind, provider, model, is_cloud, include_sensitive, profile_a, profile_b, input_chars, output_chars, ok, error, prompt) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(now()).bind(kind).bind(provider).bind(model).bind(mode.is_cloud).bind(mode.include_sensitive).bind(profile_a).bind(profile_b)
        .bind(input_chars as i64).bind(output_chars as i64).bind(ok).bind(error).bind(if mode.is_cloud { prompt } else { None })
        .execute(pool).await?;
    Ok(())
}

pub async fn log_rows(pool: &SqlitePool, limit: i64) -> Result<Vec<LogRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT at, kind, provider, model, is_cloud, include_sensitive, input_chars, output_chars, ok, error, prompt FROM mm_ai_log ORDER BY id DESC LIMIT ?").bind(limit).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .map(|r| LogRow {
            at: r.get("at"),
            kind: r.get("kind"),
            provider: r.get("provider"),
            model: r.get("model"),
            is_cloud: r.get::<i64, _>("is_cloud") != 0,
            include_sensitive: r.get::<i64, _>("include_sensitive") != 0,
            input_chars: r.get("input_chars"),
            output_chars: r.get("output_chars"),
            ok: r.get::<i64, _>("ok") != 0,
            error: r.get("error"),
            prompt: r.get("prompt"),
        })
        .collect())
}

/// Blank the stored prompt text of old cloud calls (the fact that a call happened stays in the log).
pub async fn purge_old_prompts(pool: &SqlitePool, days: i64) -> Result<(), sqlx::Error> {
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
    sqlx::query("UPDATE mm_ai_log SET prompt = NULL WHERE prompt IS NOT NULL AND at < ?").bind(cutoff).execute(pool).await?;
    Ok(())
}

pub fn _org() -> &'static str {
    ORG
}
