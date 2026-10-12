//! Persistence for versioned scoring configs, and the backfill that gives every existing rule set one.
//!
//! Why a separate table rather than a field on the rule set: the numbers that read a scorecard are values a
//! matchmaker revisits independently of the rules, and a stored snapshot has to be able to name the exact version
//! that judged it. Rule sets and scoring configs are therefore versioned the same way, and a rule set points at a
//! config.

use chrono::Utc;
use matchmaking_core::ScoringConfig;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

use super::repository::{VersionRow, ORG};

/// The config created once, on first use, so the app is usable without a matchmaker defining anything.
pub const DEFAULT_CONFIG_NAME: &str = "Default scoring";

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

#[derive(Debug, Clone)]
pub struct ScoringConfigRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub current_version: u32,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A config as it applies to one rule set: which config, which version of it is in force, and the numbers themselves.
///
/// A snapshot stores the identity alongside the numbers so a stored scorecard can say exactly which reading judged it
/// -- and, if the config is re-tuned later, which reading it is *not* being judged by any more.
#[derive(Debug, Clone)]
pub struct EffectiveConfig {
    pub id: Option<String>,
    pub version: Option<u32>,
    pub definition: ScoringConfig,
}

impl EffectiveConfig {
    /// The app defaults, used when a rule set names no config (or names one that is gone).
    fn fallback() -> Self {
        EffectiveConfig { id: None, version: None, definition: ScoringConfig::default() }
    }
}

#[derive(Serialize)]
pub struct ScoringConfigSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub current_version: u32,
    pub archived: bool,
    /// The numbers in force, so a list can be read without opening each config.
    pub definition: ScoringConfig,
    /// How many rule sets currently score with this config.
    pub used_by: i64,
    pub updated_at: String,
}

pub struct ScoringConfigRepo;

impl ScoringConfigRepo {
    fn row(r: &sqlx::sqlite::SqliteRow) -> ScoringConfigRow {
        ScoringConfigRow {
            id: r.get("id"),
            name: r.get("name"),
            description: r.get("description"),
            current_version: r.get::<i64, _>("current_version") as u32,
            archived: r.get::<i64, _>("archived") != 0,
            created_at: r.get("created_at"),
            updated_at: r.get("updated_at"),
        }
    }

    async fn definition(pool: &SqlitePool, id: &str, version: u32) -> Result<Option<ScoringConfig>, sqlx::Error> {
        let json: Option<String> = sqlx::query_scalar("SELECT definition FROM mm_scoring_config_versions WHERE config_id = ? AND version = ?")
            .bind(id)
            .bind(version as i64)
            .fetch_optional(pool)
            .await?;
        json.map(|j| serde_json::from_str(&j).map_err(|e| bad(e.to_string()))).transpose()
    }

    /// The config in force for a rule set, or the app default when the rule set has none (or names a missing one).
    /// Scoring must never fail because of configuration bookkeeping.
    pub async fn config_for_rule_set(pool: &SqlitePool, rule_set_id: &str) -> Result<ScoringConfig, sqlx::Error> {
        Ok(Self::effective_for_rule_set(pool, rule_set_id).await?.definition)
    }

    /// The same lookup, but keeping the config's identity: used when writing a snapshot, which records not just the
    /// numbers that scored a pair but the exact config version they came from.
    pub async fn effective_for_rule_set(pool: &SqlitePool, rule_set_id: &str) -> Result<EffectiveConfig, sqlx::Error> {
        let id: Option<String> = sqlx::query_scalar("SELECT scoring_config_id FROM mm_rule_sets WHERE id = ? AND org_id = ?")
            .bind(rule_set_id)
            .bind(ORG)
            .fetch_optional(pool)
            .await?
            .flatten();
        let Some(id) = id else {
            return Ok(EffectiveConfig::fallback());
        };
        let current: Option<i64> = sqlx::query_scalar("SELECT current_version FROM mm_scoring_configs WHERE id = ?")
            .bind(&id)
            .fetch_optional(pool)
            .await?;
        let Some(current) = current else {
            return Ok(EffectiveConfig::fallback());
        };
        let verified = sqlx::query_scalar::<_, i64>("SELECT version FROM mm_scoring_config_versions WHERE config_id = ? AND version = ?")
            .bind(&id)
            .bind(current)
            .fetch_optional(pool)
            .await?;
        if verified.is_none() {
            return Ok(EffectiveConfig::fallback());
        }
        Ok(EffectiveConfig { definition: Self::definition(pool, &id, current as u32).await?.unwrap_or_default(), id: Some(id), version: Some(current as u32) })
    }

    pub async fn name(pool: &SqlitePool, id: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT name FROM mm_scoring_configs WHERE id = ? AND org_id = ?").bind(id).bind(ORG).fetch_optional(pool).await
    }

    /// Point a rule set at a config (or clear it). Stored on the rule set rather than on a version: it says how the
    /// rule set reads results from now on, and every snapshot records the version it was actually judged by.
    pub async fn set_rule_set_config(pool: &SqlitePool, rule_set_id: &str, config_id: Option<&str>) -> Result<u64, sqlx::Error> {
        let r = sqlx::query("UPDATE mm_rule_sets SET scoring_config_id = ?, updated_at = ? WHERE id = ? AND org_id = ?")
            .bind(config_id)
            .bind(Utc::now().to_rfc3339())
            .bind(rule_set_id)
            .bind(ORG)
            .execute(pool)
            .await?;
        Ok(r.rows_affected())
    }

    pub async fn name_taken(pool: &SqlitePool, name: &str, except_id: Option<&str>) -> Result<bool, sqlx::Error> {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_scoring_configs WHERE org_id = ? AND name = ? AND id != ?")
            .bind(ORG)
            .bind(name)
            .bind(except_id.unwrap_or(""))
            .fetch_one(pool)
            .await?;
        Ok(n > 0)
    }

    pub async fn list(pool: &SqlitePool, include_archived: bool) -> Result<Vec<ScoringConfigSummary>, sqlx::Error> {
        let sql = "SELECT s.id, s.name, s.description, s.current_version, s.archived, s.created_at, s.updated_at, v.definition,
                          (SELECT count(*) FROM mm_rule_sets r WHERE r.scoring_config_id = s.id) AS used_by
                   FROM mm_scoring_configs s JOIN mm_scoring_config_versions v ON v.config_id = s.id AND v.version = s.current_version
                   WHERE s.org_id = ? AND (? OR s.archived = 0) ORDER BY s.name";
        let rows = sqlx::query(sql).bind(ORG).bind(include_archived).fetch_all(pool).await?;
        rows.iter()
            .map(|r| {
                let meta = Self::row(r);
                let definition: ScoringConfig = serde_json::from_str(&r.get::<String, _>("definition")).map_err(|e| bad(e.to_string()))?;
                Ok(ScoringConfigSummary {
                    used_by: r.get::<i64, _>("used_by"),
                    id: meta.id,
                    name: meta.name,
                    description: meta.description,
                    current_version: meta.current_version,
                    archived: meta.archived,
                    definition,
                    updated_at: meta.updated_at,
                })
            })
            .collect()
    }

    /// One config at `version` (current when None), with its version history. Old versions stay readable.
    pub async fn get(
        pool: &SqlitePool,
        id: &str,
        version: Option<u32>,
    ) -> Result<Option<(ScoringConfigRow, ScoringConfig, Vec<VersionRow>)>, sqlx::Error> {
        let Some(row) = sqlx::query("SELECT id, name, description, current_version, archived, created_at, updated_at FROM mm_scoring_configs WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(ORG)
            .fetch_optional(pool)
            .await?
        else {
            return Ok(None);
        };
        let meta = Self::row(&row);
        let v = version.unwrap_or(meta.current_version);
        let Some(def) = Self::definition(pool, id, v).await? else {
            return Ok(None);
        };
        let versions = sqlx::query("SELECT version, created_at, note FROM mm_scoring_config_versions WHERE config_id = ? ORDER BY version DESC")
            .bind(id)
            .fetch_all(pool)
            .await?
            .iter()
            .map(|r| VersionRow { version: r.get::<i64, _>("version") as u32, created_at: r.get("created_at"), note: r.get("note") })
            .collect();
        Ok(Some((meta, def, versions)))
    }

    /// Create a config at v1. Returns its id.
    pub async fn create(pool: &SqlitePool, name: &str, description: &str, def: &ScoringConfig, note: Option<&str>) -> Result<String, sqlx::Error> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let json = serde_json::to_string(def).map_err(|e| bad(e.to_string()))?;
        let mut tx = pool.begin().await?;
        sqlx::query("INSERT INTO mm_scoring_configs (id, org_id, name, description, current_version, archived, created_at, updated_at) VALUES (?, ?, ?, ?, 1, 0, ?, ?)")
            .bind(&id).bind(ORG).bind(name).bind(description).bind(&now).bind(&now)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_scoring_config_versions (config_id, version, definition, note, created_at) VALUES (?, 1, ?, ?, ?)")
            .bind(&id).bind(json).bind(note).bind(&now)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// Append `def` as the next version. Versions are immutable: an existing version is never rewritten, so a
    /// stored snapshot that names "config@3" keeps meaning what it meant when it was written. Returns the new number.
    pub async fn save_version(
        pool: &SqlitePool,
        id: &str,
        name: &str,
        description: &str,
        def: &ScoringConfig,
        note: Option<&str>,
    ) -> Result<u32, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let current: Option<i64> = sqlx::query_scalar("SELECT current_version FROM mm_scoring_configs WHERE id = ? AND org_id = ?")
            .bind(id).bind(ORG).fetch_optional(&mut *tx).await?;
        let next = current.ok_or_else(|| bad("Scoring config not found"))? as u32 + 1;
        let json = serde_json::to_string(def).map_err(|e| bad(e.to_string()))?;
        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO mm_scoring_config_versions (config_id, version, definition, note, created_at) VALUES (?, ?, ?, ?, ?)")
            .bind(id).bind(next as i64).bind(json).bind(note).bind(&now)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE mm_scoring_configs SET name = ?, description = ?, current_version = ?, updated_at = ? WHERE id = ? AND org_id = ?")
            .bind(name).bind(description).bind(next as i64).bind(&now).bind(id).bind(ORG)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(next)
    }

    pub async fn set_archived(pool: &SqlitePool, id: &str, archived: bool) -> Result<u64, sqlx::Error> {
        let r = sqlx::query("UPDATE mm_scoring_configs SET archived = ?, updated_at = ? WHERE id = ? AND org_id = ?")
            .bind(archived).bind(Utc::now().to_rfc3339()).bind(id).bind(ORG)
            .execute(pool).await?;
        Ok(r.rows_affected())
    }

    /// Create the default scoring config once, then give every rule set one. Idempotent, and safe to call on every
    /// list: it does nothing once every rule set has a config.
    ///
    /// The prior a rule set used to carry in its own definition is preserved rather than discarded. For each rule
    /// set whose stored definition holds a `prior_score` different from the default, a config version holding
    /// exactly that value is created and the rule set points at it, so a future scorecard reproduces the same
    /// ranking score it would have produced before this change.
    pub async fn ensure_scoring_configs(pool: &SqlitePool) -> Result<(), sqlx::Error> {
        let default_id = match sqlx::query_scalar::<_, String>("SELECT id FROM mm_scoring_configs WHERE org_id = ? AND name = ?")
            .bind(ORG)
            .bind(DEFAULT_CONFIG_NAME)
            .fetch_optional(pool)
            .await?
        {
            Some(id) => id,
            None => {
                Self::create(pool, DEFAULT_CONFIG_NAME, "The cutoffs and the neutral baseline every rule set starts with.", &ScoringConfig::default(), Some("Created with the first scoring config")).await?
            }
        };

        // Every rule set's current definition, plus whatever prior it recorded in its own JSON.
        let rows = sqlx::query(
            "SELECT s.id, v.definition FROM mm_rule_sets s
             JOIN mm_rule_set_versions v ON v.rule_set_id = s.id AND v.version = s.current_version",
        )
        .fetch_all(pool)
        .await?;
        for r in rows {
            let rule_set_id: String = r.get("id");
            let raw: String = r.get("definition");
            let json: serde_json::Value = serde_json::from_str(&raw).map_err(|e| bad(e.to_string()))?;
            let legacy_prior = json.get("prior_score").and_then(|p| p.as_f64());
            let config_id = match legacy_prior {
                Some(p) if p != ScoringConfig::default().prior_score => {
                    let def = ScoringConfig { prior_score: p, ..ScoringConfig::default() };
                    // One config per rule set, named after it, so the value stays visible and editable.
                    let name = match sqlx::query_scalar::<_, String>("SELECT name FROM mm_rule_sets WHERE id = ?").bind(&rule_set_id).fetch_optional(pool).await? {
                        Some(n) => format!("{n} (prior {})", trim_num(p)),
                        None => continue,
                    };
                    if let Some(existing) =
                        sqlx::query_scalar::<_, String>("SELECT id FROM mm_scoring_configs WHERE org_id = ? AND name = ?").bind(ORG).bind(&name).fetch_optional(pool).await?
                    {
                        existing
                    } else {
                        Self::create(pool, &name, "Carried over from the rule set, which used to hold this baseline itself.", &def, Some("Backfilled from the rule set")).await?
                    }
                }
                _ => default_id.clone(),
            };
            sqlx::query("UPDATE mm_rule_sets SET scoring_config_id = ? WHERE id = ? AND org_id = ?")
                .bind(&config_id)
                .bind(&rule_set_id)
                .bind(ORG)
                .execute(pool)
                .await?;
        }
        Ok(())
    }
}

/// 20 shows as "20", 12.5 as "12.5": names are for people to read, not for round-tripping.
fn trim_num(v: f64) -> String {
    if (v - v.round()).abs() < f64::EPSILON {
        format!("{}", v.round() as i64)
    } else {
        format!("{v}")
    }
}
