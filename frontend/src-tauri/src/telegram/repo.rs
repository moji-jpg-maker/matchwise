//! Persistence for the Telegram adapter.

use super::types::Keyboard;
use crate::mm::repository::ORG;
use chrono::{Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

pub fn now_str() -> String {
    Utc::now().to_rfc3339()
}

/// Text of the consent notice is versioned: bump this when it changes so people are asked again.
pub const CONSENT_VERSION: i64 = 1;
pub const INVITE_HOURS: i64 = 48;
pub const MAX_FAILED_CODES: i64 = 5;
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTVWXYZ23456789"; // no 0/O, 1/I/L, U
const CODE_LEN: usize = 10;

// ------------------------------------------------------------------ settings (mm_meta)

pub async fn get_meta(pool: &SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT value FROM mm_meta WHERE org_id = ? AND key = ?").bind(ORG).bind(key).fetch_optional(pool).await
}

pub async fn set_meta(pool: &SqlitePool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO mm_meta (org_id, key, value) VALUES (?, ?, ?) ON CONFLICT(org_id, key) DO UPDATE SET value = excluded.value")
        .bind(ORG).bind(key).bind(value).execute(pool).await?;
    Ok(())
}

pub async fn delete_meta(pool: &SqlitePool, key: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM mm_meta WHERE org_id = ? AND key = ?").bind(ORG).bind(key).execute(pool).await?;
    Ok(())
}

pub async fn introduction_fields(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
    match get_meta(pool, "telegram_intro_fields").await? {
        Some(j) => serde_json::from_str(&j).map_err(|e| bad(e.to_string())),
        None => Ok(matchmaking_core::DEFAULT_INTRODUCTION_FIELDS.iter().map(|s| s.to_string()).collect()),
    }
}

pub async fn show_first_name(pool: &SqlitePool) -> Result<bool, sqlx::Error> {
    Ok(get_meta(pool, "telegram_show_first_name").await?.map_or(true, |v| v == "1"))
}

pub async fn set_sharing(pool: &SqlitePool, fields: &[String], show_first_name: bool) -> Result<(), sqlx::Error> {
    set_meta(pool, "telegram_intro_fields", &serde_json::to_string(fields).map_err(|e| bad(e.to_string()))?).await?;
    set_meta(pool, "telegram_show_first_name", if show_first_name { "1" } else { "0" }).await
}

pub async fn last_update_id(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    Ok(get_meta(pool, "telegram_last_update_id").await?.and_then(|v| v.parse().ok()).unwrap_or(0))
}

pub async fn set_last_update_id(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    set_meta(pool, "telegram_last_update_id", &id.to_string()).await
}

// ------------------------------------------------------------------ links

#[derive(Debug, Clone)]
pub struct Link {
    pub profile_id: String,
    pub chat_id: i64,
    pub username: Option<String>,
    pub linked_at: String,
    /// Agreed to the current version of the notice.
    pub consented: bool,
    pub notifications_enabled: bool,
}

fn row_to_link(r: &sqlx::sqlite::SqliteRow) -> Link {
    let version: Option<i64> = r.get("consent_version");
    let at: Option<String> = r.get("consented_at");
    Link {
        profile_id: r.get("profile_id"),
        chat_id: r.get("chat_id"),
        username: r.get("username"),
        linked_at: r.get("linked_at"),
        consented: at.is_some() && version == Some(CONSENT_VERSION),
        notifications_enabled: r.get::<i64, _>("notifications_enabled") != 0,
    }
}

const LINK_COLS: &str = "profile_id, chat_id, username, linked_at, consent_version, consented_at, notifications_enabled";

pub async fn link_by_chat(pool: &SqlitePool, chat_id: i64) -> Result<Option<Link>, sqlx::Error> {
    let r = sqlx::query(&format!("SELECT {LINK_COLS} FROM mm_telegram_links WHERE chat_id = ?")).bind(chat_id).fetch_optional(pool).await?;
    Ok(r.as_ref().map(row_to_link))
}

pub async fn link_by_profile(pool: &SqlitePool, profile_id: &str) -> Result<Option<Link>, sqlx::Error> {
    let r = sqlx::query(&format!("SELECT {LINK_COLS} FROM mm_telegram_links WHERE profile_id = ?")).bind(profile_id).fetch_optional(pool).await?;
    Ok(r.as_ref().map(row_to_link))
}

#[derive(Debug, PartialEq, Eq)]
pub enum LinkError {
    ProfileHasChat,
    ChatHasProfile,
    Db(String),
}

pub async fn create_link(pool: &SqlitePool, profile_id: &str, chat_id: i64, user_id: i64, username: Option<&str>) -> Result<(), LinkError> {
    let db = |e: sqlx::Error| LinkError::Db(e.to_string());
    if link_by_profile(pool, profile_id).await.map_err(db)?.is_some() {
        return Err(LinkError::ProfileHasChat);
    }
    if link_by_chat(pool, chat_id).await.map_err(db)?.is_some() {
        return Err(LinkError::ChatHasProfile);
    }
    sqlx::query("INSERT INTO mm_telegram_links (profile_id, chat_id, telegram_user_id, username, linked_at) VALUES (?, ?, ?, ?, ?)")
        .bind(profile_id).bind(chat_id).bind(user_id).bind(username).bind(now_str())
        .execute(pool).await.map_err(db)?;
    Ok(())
}

pub async fn set_consent(pool: &SqlitePool, profile_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_telegram_links SET consent_version = ?, consented_at = ? WHERE profile_id = ?")
        .bind(CONSENT_VERSION).bind(now_str()).bind(profile_id).execute(pool).await?;
    Ok(())
}

pub async fn set_notifications(pool: &SqlitePool, profile_id: &str, enabled: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_telegram_links SET notifications_enabled = ? WHERE profile_id = ?").bind(enabled).bind(profile_id).execute(pool).await?;
    Ok(())
}

/// Remove the link, pending messages and conversation state. The profile itself is untouched.
pub async fn delete_link(pool: &SqlitePool, profile_id: &str) -> Result<(), sqlx::Error> {
    let chat: Option<i64> = sqlx::query_scalar("SELECT chat_id FROM mm_telegram_links WHERE profile_id = ?").bind(profile_id).fetch_optional(pool).await?;
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE mm_outbox SET status = 'cancelled' WHERE profile_id = ? AND status = 'pending'").bind(profile_id).execute(&mut *tx).await?;
    if let Some(c) = chat {
        sqlx::query("DELETE FROM mm_telegram_state WHERE chat_id = ?").bind(c).execute(&mut *tx).await?;
    }
    sqlx::query("DELETE FROM mm_telegram_links WHERE profile_id = ?").bind(profile_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

// ------------------------------------------------------------------ invites

fn hash_code(code: &str) -> String {
    let mut h = Sha256::new();
    h.update(code.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Upper-case, drop separators and anything that is not a letter or digit.
pub fn normalize_code(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_uppercase()
}

pub struct Invite {
    /// Shown once; only its hash is stored.
    pub code: String,
    pub expires_at: String,
}

/// Create an invite for a profile, revoking any unused earlier one.
pub async fn create_invite(pool: &SqlitePool, profile_id: &str) -> Result<Invite, sqlx::Error> {
    let mut rng = rand::rngs::OsRng;
    let mut code = String::with_capacity(CODE_LEN);
    for _ in 0..CODE_LEN {
        // rejection sampling keeps the distribution uniform
        loop {
            let mut b = [0u8; 1];
            rng.fill_bytes(&mut b);
            if (b[0] as usize) < 240 {
                code.push(CODE_ALPHABET[(b[0] as usize) % CODE_ALPHABET.len()] as char);
                break;
            }
        }
    }
    let now = Utc::now();
    let expires = (now + Duration::hours(INVITE_HOURS)).to_rfc3339();
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE mm_telegram_invites SET revoked = 1 WHERE profile_id = ? AND used_at IS NULL").bind(profile_id).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO mm_telegram_invites (profile_id, code_hash, created_at, expires_at) VALUES (?, ?, ?, ?)")
        .bind(profile_id).bind(hash_code(&code)).bind(now.to_rfc3339()).bind(&expires)
        .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Invite { code, expires_at: expires })
}

#[derive(Debug, PartialEq, Eq)]
pub enum InviteError {
    Invalid,
    Used,
    Expired,
}

/// Single use: the code is marked used in the same statement that checks it.
pub async fn redeem_invite(pool: &SqlitePool, raw_code: &str) -> Result<Result<String, InviteError>, sqlx::Error> {
    let code = normalize_code(raw_code);
    if code.len() != CODE_LEN {
        return Ok(Err(InviteError::Invalid));
    }
    let row = sqlx::query("SELECT id, profile_id, expires_at, used_at, revoked FROM mm_telegram_invites WHERE code_hash = ?")
        .bind(hash_code(&code)).fetch_optional(pool).await?;
    let Some(row) = row else { return Ok(Err(InviteError::Invalid)) };
    if row.get::<i64, _>("revoked") != 0 {
        return Ok(Err(InviteError::Invalid));
    }
    if row.get::<Option<String>, _>("used_at").is_some() {
        return Ok(Err(InviteError::Used));
    }
    let expires: String = row.get("expires_at");
    let expired = chrono::DateTime::parse_from_rfc3339(&expires).map(|e| e < Utc::now()).unwrap_or(true);
    if expired {
        return Ok(Err(InviteError::Expired));
    }
    let id: i64 = row.get("id");
    let n = sqlx::query("UPDATE mm_telegram_invites SET used_at = ? WHERE id = ? AND used_at IS NULL AND revoked = 0")
        .bind(now_str()).bind(id).execute(pool).await?.rows_affected();
    if n != 1 {
        return Ok(Err(InviteError::Used));
    }
    Ok(Ok(row.get("profile_id")))
}

pub async fn has_open_invite(pool: &SqlitePool, profile_id: &str) -> Result<bool, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_telegram_invites WHERE profile_id = ? AND used_at IS NULL AND revoked = 0 AND expires_at > ?")
        .bind(profile_id).bind(now_str()).fetch_one(pool).await?;
    Ok(n > 0)
}

pub async fn record_failed_attempt(pool: &SqlitePool, chat_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO mm_telegram_attempts (chat_id, at) VALUES (?, ?)").bind(chat_id).bind(now_str()).execute(pool).await?;
    Ok(())
}

pub async fn failed_attempts_last_hour(pool: &SqlitePool, chat_id: i64) -> Result<i64, sqlx::Error> {
    let since = (Utc::now() - Duration::hours(1)).to_rfc3339();
    sqlx::query("DELETE FROM mm_telegram_attempts WHERE at < ?").bind(&since).execute(pool).await?;
    sqlx::query_scalar("SELECT count(*) FROM mm_telegram_attempts WHERE chat_id = ? AND at >= ?").bind(chat_id).bind(since).fetch_one(pool).await
}

// ------------------------------------------------------------------ conversation state

pub async fn get_state(pool: &SqlitePool, chat_id: i64) -> Result<Option<serde_json::Value>, sqlx::Error> {
    let s: Option<String> = sqlx::query_scalar("SELECT state FROM mm_telegram_state WHERE chat_id = ?").bind(chat_id).fetch_optional(pool).await?;
    s.map(|s| serde_json::from_str(&s).map_err(|e| bad(e.to_string()))).transpose()
}

pub async fn set_state(pool: &SqlitePool, chat_id: i64, state: &serde_json::Value) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO mm_telegram_state (chat_id, state, updated_at) VALUES (?, ?, ?) ON CONFLICT(chat_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at")
        .bind(chat_id).bind(state.to_string()).bind(now_str()).execute(pool).await?;
    Ok(())
}

pub async fn clear_state(pool: &SqlitePool, chat_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM mm_telegram_state WHERE chat_id = ?").bind(chat_id).execute(pool).await?;
    Ok(())
}

// ------------------------------------------------------------------ outbox

#[derive(Debug, Clone)]
pub struct OutboxRow {
    pub id: i64,
    pub profile_id: String,
    pub kind: String,
    pub text: String,
    pub keyboard: Option<Keyboard>,
    pub match_id: Option<String>,
    pub attempts: i64,
    pub status: String,
    pub created_at: String,
    pub sent_at: Option<String>,
    pub last_error: Option<String>,
}

fn row_to_outbox(r: &sqlx::sqlite::SqliteRow) -> Result<OutboxRow, sqlx::Error> {
    let kb: Option<String> = r.get("keyboard");
    Ok(OutboxRow {
        id: r.get("id"),
        profile_id: r.get("profile_id"),
        kind: r.get("kind"),
        text: r.get("text"),
        keyboard: kb.map(|k| serde_json::from_str(&k).map_err(|e| bad(e.to_string()))).transpose()?,
        match_id: r.get("match_id"),
        attempts: r.get("attempts"),
        status: r.get("status"),
        created_at: r.get("created_at"),
        sent_at: r.get("sent_at"),
        last_error: r.get("last_error"),
    })
}

const OUTBOX_COLS: &str = "id, profile_id, kind, text, keyboard, match_id, attempts, status, created_at, sent_at, last_error";

pub async fn enqueue(pool: &SqlitePool, profile_id: &str, kind: &str, text: &str, keyboard: Option<&Keyboard>, match_id: Option<&str>) -> Result<i64, sqlx::Error> {
    let now = now_str();
    let kb = keyboard.map(|k| serde_json::to_string(k)).transpose().map_err(|e| bad(e.to_string()))?;
    let r = sqlx::query("INSERT INTO mm_outbox (profile_id, kind, text, keyboard, match_id, status, attempts, next_attempt_at, created_at) VALUES (?, ?, ?, ?, ?, 'pending', 0, ?, ?)")
        .bind(profile_id).bind(kind).bind(text).bind(kb).bind(match_id).bind(&now).bind(&now)
        .execute(pool).await?;
    Ok(r.last_insert_rowid())
}

pub async fn due(pool: &SqlitePool, limit: i64) -> Result<Vec<OutboxRow>, sqlx::Error> {
    let rows = sqlx::query(&format!("SELECT {OUTBOX_COLS} FROM mm_outbox WHERE status = 'pending' AND next_attempt_at <= ? ORDER BY id LIMIT ?"))
        .bind(now_str()).bind(limit).fetch_all(pool).await?;
    rows.iter().map(row_to_outbox).collect()
}

pub async fn mark_sent(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_outbox SET status = 'sent', sent_at = ?, attempts = attempts + 1, last_error = NULL WHERE id = ?").bind(now_str()).bind(id).execute(pool).await?;
    Ok(())
}

/// Record a failed attempt. With `retry_in_secs` the message stays pending and is retried later; without, it is given up.
pub async fn mark_failed(pool: &SqlitePool, id: i64, error: &str, retry_in_secs: Option<i64>) -> Result<(), sqlx::Error> {
    match retry_in_secs {
        Some(s) => {
            let next = (Utc::now() + Duration::seconds(s)).to_rfc3339();
            sqlx::query("UPDATE mm_outbox SET attempts = attempts + 1, last_error = ?, next_attempt_at = ? WHERE id = ?").bind(error).bind(next).bind(id).execute(pool).await?;
        }
        None => {
            sqlx::query("UPDATE mm_outbox SET status = 'failed', attempts = attempts + 1, last_error = ? WHERE id = ?").bind(error).bind(id).execute(pool).await?;
        }
    }
    Ok(())
}

pub async fn cancel(pool: &SqlitePool, id: i64, why: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_outbox SET status = 'cancelled', last_error = ? WHERE id = ?").bind(why).bind(id).execute(pool).await?;
    Ok(())
}

pub async fn outbox_for_profile(pool: &SqlitePool, profile_id: &str, limit: i64) -> Result<Vec<OutboxRow>, sqlx::Error> {
    let rows = sqlx::query(&format!("SELECT {OUTBOX_COLS} FROM mm_outbox WHERE profile_id = ? ORDER BY id DESC LIMIT ?")).bind(profile_id).bind(limit).fetch_all(pool).await?;
    rows.iter().map(row_to_outbox).collect()
}

pub async fn count_kind(pool: &SqlitePool, profile_id: &str, kind: &str, match_id: Option<&str>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM mm_outbox WHERE profile_id = ? AND kind = ? AND status != 'cancelled' AND (? IS NULL OR match_id = ?)")
        .bind(profile_id).bind(kind).bind(match_id).bind(match_id).fetch_one(pool).await
}

pub async fn last_created(pool: &SqlitePool, profile_id: &str, kinds: &[&str], match_id: Option<&str>) -> Result<Option<String>, sqlx::Error> {
    let list = kinds.iter().map(|k| format!("'{}'", k.replace('\'', ""))).collect::<Vec<_>>().join(",");
    sqlx::query_scalar(&format!("SELECT max(created_at) FROM mm_outbox WHERE profile_id = ? AND kind IN ({list}) AND status != 'cancelled' AND (? IS NULL OR match_id = ?)"))
        .bind(profile_id).bind(match_id).bind(match_id).fetch_one(pool).await
}

/// Delete delivered or abandoned messages older than `days` (the conversation log is kept separately).
pub async fn purge_outbox(pool: &SqlitePool, days: i64) -> Result<u64, sqlx::Error> {
    let cutoff = (Utc::now() - Duration::days(days)).to_rfc3339();
    Ok(sqlx::query("DELETE FROM mm_outbox WHERE status != 'pending' AND created_at < ?").bind(cutoff).execute(pool).await?.rows_affected())
}

// ------------------------------------------------------------------ conversation log and requests

#[derive(Debug, Clone)]
pub struct MessageRow {
    pub id: i64,
    pub direction: String,
    pub text: String,
    pub match_id: Option<String>,
    pub created_at: String,
    pub is_read: bool,
}

pub async fn add_message(pool: &SqlitePool, profile_id: &str, direction: &str, text: &str, match_id: Option<&str>) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO mm_telegram_messages (profile_id, direction, text, match_id, created_at, is_read) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(profile_id).bind(direction).bind(text).bind(match_id).bind(now_str()).bind(direction == "out")
        .execute(pool).await?;
    Ok(())
}

pub async fn messages(pool: &SqlitePool, profile_id: &str, limit: i64) -> Result<Vec<MessageRow>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, direction, text, match_id, created_at, is_read FROM mm_telegram_messages WHERE profile_id = ? ORDER BY id DESC LIMIT ?")
        .bind(profile_id).bind(limit).fetch_all(pool).await?;
    Ok(rows.iter().map(|r| MessageRow { id: r.get("id"), direction: r.get("direction"), text: r.get("text"), match_id: r.get("match_id"), created_at: r.get("created_at"), is_read: r.get::<i64, _>("is_read") != 0 }).collect())
}

pub async fn mark_read(pool: &SqlitePool, profile_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_telegram_messages SET is_read = 1 WHERE profile_id = ? AND is_read = 0").bind(profile_id).execute(pool).await?;
    Ok(())
}

pub async fn unread_counts(pool: &SqlitePool) -> Result<BTreeMap<String, i64>, sqlx::Error> {
    let rows = sqlx::query("SELECT profile_id, count(*) AS n FROM mm_telegram_messages WHERE direction = 'in' AND is_read = 0 GROUP BY profile_id").fetch_all(pool).await?;
    Ok(rows.iter().map(|r| (r.get::<String, _>("profile_id"), r.get::<i64, _>("n"))).collect())
}

/// Record a request unless an identical one is already open. Returns whether a new one was created.
pub async fn add_request(pool: &SqlitePool, profile_id: &str, kind: &str) -> Result<bool, sqlx::Error> {
    let open: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_data_requests WHERE profile_id = ? AND kind = ? AND status = 'open'").bind(profile_id).bind(kind).fetch_one(pool).await?;
    if open > 0 {
        return Ok(false);
    }
    sqlx::query("INSERT INTO mm_data_requests (profile_id, kind, created_at) VALUES (?, ?, ?)").bind(profile_id).bind(kind).bind(now_str()).execute(pool).await?;
    Ok(true)
}

pub async fn open_requests(pool: &SqlitePool) -> Result<Vec<(i64, String, String, String)>, sqlx::Error> {
    let rows = sqlx::query("SELECT id, profile_id, kind, created_at FROM mm_data_requests WHERE status = 'open' ORDER BY id").fetch_all(pool).await?;
    Ok(rows.iter().map(|r| (r.get("id"), r.get("profile_id"), r.get("kind"), r.get("created_at"))).collect())
}

pub async fn resolve_request(pool: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE mm_data_requests SET status = 'done', resolved_at = ? WHERE id = ?").bind(now_str()).bind(id).execute(pool).await?;
    Ok(())
}
