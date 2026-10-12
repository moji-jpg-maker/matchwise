use chrono::Utc;
use matchmaking_core::{Interest, MatchStatus, Outcome};
use serde_json::Value as Json;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

use super::repository::ORG;

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

#[derive(Debug, Clone)]
pub struct MatchRow {
    pub id: String,
    pub profile_a: String,
    pub profile_b: String,
    pub source_profile: Option<String>,
    pub rule_set_id: String,
    pub rule_set_version: u32,
    pub status: MatchStatus,
    pub hold_reason: Option<String>,
    pub a_response: Interest,
    pub b_response: Interest,
    pub outcome: Option<Outcome>,
    pub override_reason: Option<String>,
    pub weight_overrides: BTreeMap<String, f64>,
    pub hidden: bool,
    pub created_at: String,
    pub updated_at: String,
}

pub struct SnapshotRow {
    pub rule_set_version: u32,
    pub trigger: String,
    pub scorecard: Json,
    pub created_at: String,
}

pub struct NoteRow {
    pub id: i64,
    pub text: String,
    pub created_at: String,
}

pub struct EventRow {
    pub at: String,
    pub actor: String,
    pub kind: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub detail: Option<String>,
}

/// Store a pair in a fixed order so (x, y) and (y, x) are the same match.
pub fn pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
}

const COLS: &str = "id, profile_a, profile_b, source_profile, rule_set_id, rule_set_version, status, hold_reason, a_response, b_response, outcome, override_reason, weight_overrides, hidden, created_at, updated_at";

fn row_to_match(r: &sqlx::sqlite::SqliteRow) -> Result<MatchRow, sqlx::Error> {
    let status: String = r.get("status");
    let a: String = r.get("a_response");
    let b: String = r.get("b_response");
    let outcome: Option<String> = r.get("outcome");
    let weights: Option<String> = r.get("weight_overrides");
    Ok(MatchRow {
        id: r.get("id"),
        profile_a: r.get("profile_a"),
        profile_b: r.get("profile_b"),
        source_profile: r.get("source_profile"),
        rule_set_id: r.get("rule_set_id"),
        rule_set_version: r.get::<i64, _>("rule_set_version") as u32,
        status: status.parse().map_err(bad)?,
        hold_reason: r.get("hold_reason"),
        a_response: a.parse().map_err(bad)?,
        b_response: b.parse().map_err(bad)?,
        outcome: outcome.map(|o| o.parse().map_err(bad)).transpose()?,
        override_reason: r.get("override_reason"),
        weight_overrides: match weights {
            Some(w) => serde_json::from_str(&w).map_err(|e| bad(e.to_string()))?,
            None => BTreeMap::new(),
        },
        hidden: r.get::<i64, _>("hidden") != 0,
        created_at: r.get("created_at"),
        updated_at: r.get("updated_at"),
    })
}

pub struct MatchRepo;

impl MatchRepo {
    pub async fn get(pool: &SqlitePool, id: &str) -> Result<Option<MatchRow>, sqlx::Error> {
        let row = sqlx::query(&format!("SELECT {COLS} FROM mm_matches WHERE id = ? AND org_id = ?")).bind(id).bind(ORG).fetch_optional(pool).await?;
        row.as_ref().map(row_to_match).transpose()
    }

    pub async fn find_by_pair(pool: &SqlitePool, x: &str, y: &str) -> Result<Option<MatchRow>, sqlx::Error> {
        let (a, b) = pair_key(x, y);
        let row = sqlx::query(&format!("SELECT {COLS} FROM mm_matches WHERE org_id = ? AND profile_a = ? AND profile_b = ?"))
            .bind(ORG).bind(a).bind(b).fetch_optional(pool).await?;
        row.as_ref().map(row_to_match).transpose()
    }

    /// Every match with its latest score snapshot (JSON), newest activity first.
    pub async fn list(pool: &SqlitePool) -> Result<Vec<(MatchRow, Option<Json>)>, sqlx::Error> {
        let sql = format!(
            "SELECT {COLS}, (SELECT s.scorecard FROM mm_match_snapshots s WHERE s.match_id = mm_matches.id ORDER BY s.id DESC LIMIT 1) AS card
             FROM mm_matches WHERE org_id = ? ORDER BY updated_at DESC"
        );
        let rows = sqlx::query(&sql).bind(ORG).fetch_all(pool).await?;
        rows.iter()
            .map(|r| {
                let card: Option<String> = r.get("card");
                let card = card.map(|c| serde_json::from_str(&c).map_err(|e| bad(e.to_string()))).transpose()?;
                Ok((row_to_match(r)?, card))
            })
            .collect()
    }

    /// Create the match, its first snapshot and a "created" event in one transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        pool: &SqlitePool,
        x: &str,
        y: &str,
        source_profile: Option<&str>,
        rule_set_id: &str,
        rule_set_version: u32,
        status: MatchStatus,
        scorecard: &Json,
    ) -> Result<String, sqlx::Error> {
        let (a, b) = pair_key(x, y);
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("INSERT INTO mm_matches (id, org_id, profile_a, profile_b, source_profile, rule_set_id, rule_set_version, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&id).bind(ORG).bind(&a).bind(&b).bind(source_profile).bind(rule_set_id).bind(rule_set_version as i64).bind(status.as_str()).bind(&now).bind(&now)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_match_snapshots (match_id, rule_set_id, rule_set_version, trigger, scorecard, created_at) VALUES (?, ?, ?, 'created', ?, ?)")
            .bind(&id).bind(rule_set_id).bind(rule_set_version as i64).bind(scorecard.to_string()).bind(&now)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', 'created', NULL, ?, NULL)")
            .bind(&id).bind(&now).bind(status.as_str())
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn latest_snapshot(pool: &SqlitePool, id: &str) -> Result<Option<SnapshotRow>, sqlx::Error> {
        let row = sqlx::query("SELECT rule_set_version, trigger, scorecard, created_at FROM mm_match_snapshots WHERE match_id = ? ORDER BY id DESC LIMIT 1")
            .bind(id).fetch_optional(pool).await?;
        row.map(|r| {
            let card: String = r.get("scorecard");
            Ok(SnapshotRow {
                rule_set_version: r.get::<i64, _>("rule_set_version") as u32,
                trigger: r.get("trigger"),
                scorecard: serde_json::from_str(&card).map_err(|e| bad(e.to_string()))?,
                created_at: r.get("created_at"),
            })
        })
        .transpose()
    }

    pub async fn snapshot_count(pool: &SqlitePool, id: &str) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT count(*) FROM mm_match_snapshots WHERE match_id = ?").bind(id).fetch_one(pool).await
    }

    /// Store a new snapshot and, if requested, switch the match to a different rule-set version, with an event.
    pub async fn add_snapshot(
        pool: &SqlitePool,
        id: &str,
        rule_set_id: &str,
        rule_set_version: u32,
        trigger: &str,
        scorecard: &Json,
        weights: Option<&BTreeMap<String, f64>>,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("INSERT INTO mm_match_snapshots (match_id, rule_set_id, rule_set_version, trigger, scorecard, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(id).bind(rule_set_id).bind(rule_set_version as i64).bind(trigger).bind(scorecard.to_string()).bind(&now)
            .execute(&mut *tx).await?;
        sqlx::query("UPDATE mm_matches SET rule_set_version = ?, updated_at = ? WHERE id = ?").bind(rule_set_version as i64).bind(&now).bind(id).execute(&mut *tx).await?;
        if let Some(w) = weights {
            let json = if w.is_empty() { None } else { Some(serde_json::to_string(w).map_err(|e| bad(e.to_string()))?) };
            sqlx::query("UPDATE mm_matches SET weight_overrides = ? WHERE id = ?").bind(json).bind(id).execute(&mut *tx).await?;
        }
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', ?, NULL, NULL, ?)")
            .bind(id).bind(&now).bind(trigger).bind(format!("rule set v{rule_set_version}"))
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Move to a new status, optionally recording an outcome and an override reason, plus the event: one transaction.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_transition(
        pool: &SqlitePool,
        id: &str,
        from: MatchStatus,
        to: MatchStatus,
        outcome: Option<Outcome>,
        override_reason: Option<&str>,
        detail: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE mm_matches SET status = ?, outcome = COALESCE(?, outcome), override_reason = COALESCE(?, override_reason), updated_at = ? WHERE id = ?")
            .bind(to.as_str()).bind(outcome.map(|o| o.as_str())).bind(override_reason).bind(&now).bind(id)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', 'status_changed', ?, ?, ?)")
            .bind(id).bind(&now).bind(from.as_str()).bind(to.as_str()).bind(detail)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Record what the two people said and any status change that follows, in one transaction.
    pub async fn record_responses(
        pool: &SqlitePool,
        id: &str,
        from: MatchStatus,
        a: Interest,
        b: Interest,
        new_status: Option<MatchStatus>,
        who: &str,
    ) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE mm_matches SET a_response = ?, b_response = ?, status = COALESCE(?, status), updated_at = ? WHERE id = ?")
            .bind(a.as_str()).bind(b.as_str()).bind(new_status.map(|s| s.as_str())).bind(&now).bind(id)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', 'response_recorded', NULL, NULL, ?)")
            .bind(id).bind(&now).bind(format!("{who}: a={}, b={}", a.as_str(), b.as_str()))
            .execute(&mut *tx).await?;
        if let Some(to) = new_status {
            sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'system', 'status_changed', ?, ?, 'follows from the responses')")
                .bind(id).bind(&now).bind(from.as_str()).bind(to.as_str())
                .execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_outcome(pool: &SqlitePool, id: &str, outcome: Outcome) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE mm_matches SET outcome = ?, updated_at = ? WHERE id = ?").bind(outcome.as_str()).bind(&now).bind(id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', 'outcome_recorded', NULL, NULL, ?)")
            .bind(id).bind(&now).bind(outcome.as_str()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Set or clear the "waiting for more information" hold.
    pub async fn set_hold(pool: &SqlitePool, id: &str, reason: Option<&str>) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE mm_matches SET hold_reason = ?, updated_at = ? WHERE id = ?").bind(reason).bind(&now).bind(id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', ?, NULL, NULL, ?)")
            .bind(id).bind(&now).bind(if reason.is_some() { "info_requested" } else { "hold_cleared" }).bind(reason)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn set_hidden(pool: &SqlitePool, id: &str, hidden: bool) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("UPDATE mm_matches SET hidden = ?, updated_at = ? WHERE id = ?").bind(hidden).bind(&now).bind(id).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', ?, NULL, NULL, NULL)")
            .bind(id).bind(&now).bind(if hidden { "hidden" } else { "unhidden" }).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn add_note(pool: &SqlitePool, id: &str, text: &str) -> Result<(), sqlx::Error> {
        let now = Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        sqlx::query("INSERT INTO mm_match_notes (match_id, text, created_at) VALUES (?, ?, ?)").bind(id).bind(text).bind(&now).execute(&mut *tx).await?;
        sqlx::query("UPDATE mm_matches SET updated_at = ? WHERE id = ?").bind(&now).bind(id).execute(&mut *tx).await?;
        // the event deliberately carries no text
        sqlx::query("INSERT INTO mm_match_events (match_id, at, actor, kind, from_status, to_status, detail) VALUES (?, ?, 'matchmaker', 'note_added', NULL, NULL, NULL)")
            .bind(id).bind(&now).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn notes(pool: &SqlitePool, id: &str) -> Result<Vec<NoteRow>, sqlx::Error> {
        let rows = sqlx::query("SELECT id, text, created_at FROM mm_match_notes WHERE match_id = ? ORDER BY id DESC").bind(id).fetch_all(pool).await?;
        Ok(rows.iter().map(|r| NoteRow { id: r.get("id"), text: r.get("text"), created_at: r.get("created_at") }).collect())
    }

    pub async fn events(pool: &SqlitePool, id: &str) -> Result<Vec<EventRow>, sqlx::Error> {
        let rows = sqlx::query("SELECT at, actor, kind, from_status, to_status, detail FROM mm_match_events WHERE match_id = ? ORDER BY id DESC").bind(id).fetch_all(pool).await?;
        Ok(rows
            .iter()
            .map(|r| EventRow { at: r.get("at"), actor: r.get("actor"), kind: r.get("kind"), from_status: r.get("from_status"), to_status: r.get("to_status"), detail: r.get("detail") })
            .collect())
    }

    pub async fn counts(pool: &SqlitePool) -> Result<BTreeMap<String, i64>, sqlx::Error> {
        let rows = sqlx::query("SELECT status, count(*) AS n FROM mm_matches WHERE org_id = ? AND hidden = 0 GROUP BY status").bind(ORG).fetch_all(pool).await?;
        Ok(rows.iter().map(|r| (r.get::<String, _>("status"), r.get::<i64, _>("n"))).collect())
    }
}
