//! Read-only queries behind the decision-data export (M7b).
//!
//! The export is a *snapshot of how decisions were actually made*: one row per match, with the scorecard that was
//! stored when it was recommended and the move that eventually ended the process. Nothing here writes, and nothing
//! here reads a profile's field values -- the point of the dataset is the decision, not the person.

use serde::Serialize;
use serde_json::Value as Json;
use sqlx::{Row, SqlitePool};

use super::repository::ORG;

/// One exported row: a match, the snapshot taken when it was tracked, and the outcome that closed it.
#[derive(Debug, Clone, Serialize)]
pub struct OutcomeRow {
    pub match_id: String,
    /// Stable identity for the pair, `profile_a|profile_b`, already in the fixed match order.
    pub pair_key: String,
    pub profile_a: String,
    pub profile_b: String,
    pub status: String,
    pub a_response: String,
    pub b_response: String,
    /// The outcome recorded when the match closed, if it has closed.
    pub outcome: Option<String>,
    /// Whether recommending or approving this pair needed a written override.
    pub was_overridden: bool,
    pub rule_set_id: String,
    pub rule_set_version: u32,
    /// `created`, `rescore` or `weights_adjusted` -- how the snapshot below came to be.
    pub snapshot_trigger: Option<String>,
    pub scorecard_at: Option<String>,
    pub overall: Option<f64>,
    pub confidence: Option<f64>,
    pub ranking_score: Option<f64>,
    /// The snapshot's own verdict on the hard constraints, not today's.
    pub eligible: Option<bool>,
    pub meets_threshold: Option<bool>,
    /// Dimension key -> score, for the dimensions that had a score at snapshot time.
    pub dimension_scores: Vec<(String, f64)>,
    pub created_at: String,
    pub updated_at: String,
}

/// Columns of `mm_matches` the export needs, plus the newest snapshot's JSON and the terminal move from the history.
const SQL: &str = "
    SELECT m.id, m.profile_a, m.profile_b, m.status, m.a_response, m.b_response, m.outcome,
           m.rule_set_id, m.rule_set_version, m.override_reason, m.created_at, m.updated_at,
           s.scorecard AS card, s.trigger AS card_trigger, s.created_at AS card_at
    FROM mm_matches m
    LEFT JOIN mm_match_snapshots s
        ON s.id = (SELECT id FROM mm_match_snapshots WHERE match_id = m.id ORDER BY id DESC LIMIT 1)
    WHERE m.org_id = ?
    ORDER BY m.updated_at DESC";

fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

/// Pull the numeric dimension scores out of a stored scorecard. Anything without a score is skipped: an
/// `unknown` dimension is absence of data, and writing 0 for it would invent a number.
fn dimension_scores(card: Option<&Json>) -> Vec<(String, f64)> {
    card.and_then(|c| c.get("dimensions"))
        .and_then(|d| d.as_array())
        .map(|dims| {
            dims.iter()
                .filter_map(|d| {
                    let key = d.get("key")?.as_str()?.to_string();
                    let score = d.get("score").and_then(|s| s.as_f64())?;
                    Some((key, score))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn f64_of(card: Option<&Json>, key: &str) -> Option<f64> {
    card.and_then(|c| c.get(key)).and_then(|v| v.as_f64())
}

fn bool_of(card: Option<&Json>, key: &str) -> Option<bool> {
    card.and_then(|c| c.get(key)).and_then(|v| v.as_bool())
}

pub struct ExportRepo;

impl ExportRepo {
    /// Every match with its newest snapshot, newest activity first.
    pub async fn outcome_rows(pool: &SqlitePool) -> Result<Vec<OutcomeRow>, sqlx::Error> {
        let rows = sqlx::query(SQL).bind(ORG).fetch_all(pool).await?;
        rows.iter()
            .map(|r| {
                let card_text: Option<String> = r.get("card");
                let card: Option<Json> = card_text
                    .map(|c| serde_json::from_str(&c).map_err(|e| bad(e.to_string())))
                    .transpose()?;
                let profile_a: String = r.get("profile_a");
                let profile_b: String = r.get("profile_b");
                let override_reason: Option<String> = r.get("override_reason");
                Ok(OutcomeRow {
                    pair_key: format!("{profile_a}|{profile_b}"),
                    match_id: r.get("id"),
                    profile_a,
                    profile_b,
                    status: r.get("status"),
                    a_response: r.get("a_response"),
                    b_response: r.get("b_response"),
                    outcome: r.get("outcome"),
                    was_overridden: override_reason.map(|s| !s.trim().is_empty()).unwrap_or(false),
                    rule_set_id: r.get("rule_set_id"),
                    rule_set_version: r.get::<i64, _>("rule_set_version") as u32,
                    snapshot_trigger: r.get("card_trigger"),
                    scorecard_at: r.get("card_at"),
                    overall: f64_of(card.as_ref(), "overall"),
                    confidence: f64_of(card.as_ref(), "confidence"),
                    ranking_score: f64_of(card.as_ref(), "ranking_score"),
                    eligible: bool_of(card.as_ref(), "eligible"),
                    meets_threshold: bool_of(card.as_ref(), "meets_threshold"),
                    dimension_scores: dimension_scores(card.as_ref()),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                })
            })
            .collect()
    }

    /// The distinct dimension keys that appear in stored scorecards, so the CSV header can cover every dimension
    /// that any exported row actually scored. Sorted, so the same data always produces the same header.
    pub async fn dimension_keys(pool: &SqlitePool) -> Result<Vec<String>, sqlx::Error> {
        let rows = sqlx::query("SELECT scorecard FROM mm_match_snapshots").fetch_all(pool).await?;
        let mut keys: Vec<String> = Vec::new();
        for r in rows {
            let card: Option<String> = r.get("scorecard");
            let Some(card) = card else { continue };
            let card: Json = match serde_json::from_str(&card) {
                Ok(c) => c,
                // A scorecard we cannot parse contributes no columns rather than failing the whole export.
                Err(_) => continue,
            };
            for (k, _) in dimension_scores(Some(&card)) {
                if !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        keys.sort();
        Ok(keys)
    }
}

