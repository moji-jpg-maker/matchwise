//! Exposure logging, and the two read-only views that sit on top of it.
//!
//! Recording which candidates a search actually *showed* is the part of a future ranking that cannot be
//! reconstructed later: the outcome of a tracked pair says what was decided, but nothing says what else was on
//! screen when the decision was made. Without that, a model trained on decisions alone would learn the matchmaker's
//! browsing order as if it were preference. Rows are therefore written whenever a search returns candidates, not
//! only when something is decided.
//!
//! What lives here is collection and reporting only. Nothing computes a ranking, fits a model or re-orders a list.

use matchmaking_core::RANKING_MINIMUM;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;

use super::repository::ORG;

/// One candidate that a search returned, ready to be written as an exposure row.
#[derive(Debug, Clone)]
pub struct ExposureRow {
    pub candidate_id: String,
    /// 0-based position in the returned list.
    pub rank: i64,
    /// Whether the row was actually shown to the matchmaker (kept as a flag so a silent-candidate change is a
    /// recording decision, not a schema one).
    pub shown: bool,
    pub score: Option<f64>,
    pub confidence: Option<f64>,
    pub eligible: Option<bool>,
}

/// What the exposure log contains, for the matchmaker and for the ranking gate.
#[derive(Debug, Clone, Serialize)]
pub struct ExposureSummary {
    pub exposures: i64,
    /// Distinct (profile, candidate) pairs that have appeared in a result list.
    pub pairs_shown: i64,
    /// How many of those shown pairs later produced a recorded outcome.
    pub pairs_with_outcome: i64,
    pub searches: i64,
    pub first_at: Option<String>,
    pub last_at: Option<String>,
}

/// Whether there is enough recorded history to compare a learned ranking, and the counts that decide it.
///
/// Read-only and deliberately dumb: it reports the gate and the numbers behind it. It holds no coefficients and
/// proposes no ordering.
#[derive(Debug, Clone, Serialize)]
pub struct RankingReadiness {
    /// Finished matches needed before a learned ranking could be compared (the gate).
    pub minimum_finished: i64,
    /// Finished matches recorded today.
    pub finished: i64,
    /// Whether the gate is reached. While false, `state` says why there is nothing to compare.
    pub ready: bool,
    /// One plain sentence describing the current position, shown to the matchmaker as-is.
    pub state: String,
    pub with_outcome: i64,
    /// Matches carrying a stored scorecard: without one there is nothing to compare a result against.
    pub with_snapshot: i64,
    /// Outcome -> count, for matches that have closed.
    pub outcome_counts: BTreeMap<String, i64>,
    /// Matches still in progress, by status.
    pub open_by_status: BTreeMap<String, i64>,
    /// Candidates shown by searches so far (the exposure log).
    pub exposures: i64,
    pub pairs_shown: i64,
    pub pairs_with_outcome: i64,
}

pub struct ExposureRepo;

impl ExposureRepo {
    /// Write one row per candidate returned by one search, sharing `search_id` so the result list can be
    /// reconstructed as a list rather than a bag of pairs.
    ///
    /// Only candidates the search *returned* are passed in: an evaluated-but-filtered candidate, or one cut by the
    /// request's `limit`, was never on screen, and logging it would invent an exposure that did not happen.
    #[allow(clippy::too_many_arguments)]
    pub async fn record(
        pool: &SqlitePool,
        search_id: &str,
        profile_id: &str,
        rule_set_id: &str,
        rule_set_version: u32,
        scoring_config_id: Option<&str>,
        scoring_config_version: Option<u32>,
        rows: &[ExposureRow],
    ) -> Result<(), sqlx::Error> {
        if rows.is_empty() {
            return Ok(());
        }
        // One reading for the whole batch: the rows are the single list that was on screen at that moment.
        // Successive searches can share an instant; `id` is the tiebreaker for their order.
        let now = chrono::Utc::now().to_rfc3339();
        let mut tx = pool.begin().await?;
        for r in rows {
            sqlx::query(
                "INSERT INTO mm_exposure (search_id, profile_id, candidate_id, rule_set_id, rule_set_version,
                                          scoring_config_id, scoring_config_version, rank, shown, score, confidence, eligible, at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(search_id)
            .bind(profile_id)
            .bind(&r.candidate_id)
            .bind(rule_set_id)
            .bind(rule_set_version as i64)
            .bind(scoring_config_id)
            .bind(scoring_config_version.map(|v| v as i64))
            .bind(r.rank)
            .bind(if r.shown { 1i64 } else { 0 })
            .bind(r.score)
            .bind(r.confidence)
            .bind(r.eligible.map(|e| if e { 1i64 } else { 0 }))
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn summary(pool: &SqlitePool) -> Result<ExposureSummary, sqlx::Error> {
        let row = sqlx::query(
            "SELECT count(*) AS exposures,
                    count(DISTINCT search_id) AS searches,
                    min(at) AS first_at,
                    max(at) AS last_at
             FROM mm_exposure WHERE shown = 1",
        )
        .fetch_one(pool)
        .await?;
        let (pairs_shown, pairs_with_outcome) = Self::pair_counts(pool).await?;
        Ok(ExposureSummary {
            exposures: row.get("exposures"),
            searches: row.get("searches"),
            first_at: row.get("first_at"),
            last_at: row.get("last_at"),
            pairs_shown,
            pairs_with_outcome,
        })
    }

    /// How many distinct shown pairs exist, and how many of them later produced an outcome. A pair counts as having
    /// an outcome when a tracked match for the same two people recorded one *after* the moment the candidate was
    /// shown -- a pair that closed before it appeared on screen was not decided from that screenful, and a decision
    /// sharing the exposure's own instant cannot be shown to have followed it.
    ///
    /// The exposure's moment is the earliest one recorded for the pair: a pair that was decided and later shown
    /// again still produced its outcome after it had been seen.
    ///
    /// The pair is unordered, the same way matches are stored, so (x, y) and (y, x) are one pair here too. The
    /// outcome's own moment comes from the `outcome_recorded` event, never from `updated_at`: a later note or
    /// status change touches that column and would otherwise date the decision to the edit.
    async fn pair_counts(pool: &SqlitePool) -> Result<(i64, i64), sqlx::Error> {
        let row = sqlx::query(
            "WITH shown_pairs AS (
                 SELECT min(profile_id, candidate_id) AS profile_a,
                        max(profile_id, candidate_id) AS profile_b,
                        min(at) AS shown_at
                 FROM mm_exposure
                 WHERE shown = 1
                 GROUP BY min(profile_id, candidate_id), max(profile_id, candidate_id)
             )
             SELECT count(*) AS pairs_shown,
                    sum(CASE WHEN EXISTS (
                        SELECT 1 FROM mm_matches m
                        WHERE m.org_id = ?
                          AND m.profile_a = p.profile_a
                          AND m.profile_b = p.profile_b
                          AND m.outcome IS NOT NULL
                          AND (SELECT e.at FROM mm_match_events e
                               WHERE e.match_id = m.id AND e.kind = 'outcome_recorded'
                               ORDER BY e.at DESC LIMIT 1) > p.shown_at
                    ) THEN 1 ELSE 0 END) AS pairs_with_outcome
             FROM shown_pairs p",
        )
        .bind(ORG)
        .fetch_one(pool)
        .await?;
        Ok((row.get("pairs_shown"), row.get::<Option<i64>, _>("pairs_with_outcome").unwrap_or(0)))
    }

    /// The gate and the counts behind it. Reports only: below the gate it says plainly that there is not enough
    /// outcome data to compare a learned ranking, and above it, that the comparison has become possible.
    pub async fn ranking_readiness(pool: &SqlitePool) -> Result<RankingReadiness, sqlx::Error> {
        let mut outcome_counts: BTreeMap<String, i64> = BTreeMap::new();
        let mut open_by_status: BTreeMap<String, i64> = BTreeMap::new();
        let rows = sqlx::query(
            // One row per match, so `snaps` is that match's own snapshot count. Grouping the matches instead left
            // `mm_matches.id` resolved from an arbitrary row of the group, counting every group as having none.
            "SELECT m.status, m.outcome,
                    (SELECT count(*) FROM mm_match_snapshots s WHERE s.match_id = m.id) AS snaps
             FROM mm_matches m WHERE m.org_id = ?",
        )
        .bind(ORG)
        .fetch_all(pool)
        .await?;
        let mut finished = 0i64;
        let mut with_snapshot = 0i64;
        for r in &rows {
            if r.get::<i64, _>("snaps") > 0 {
                with_snapshot += 1;
            }
            match r.get::<Option<String>, _>("outcome") {
                Some(o) => {
                    finished += 1;
                    *outcome_counts.entry(o).or_default() += 1;
                }
                None => {
                    *open_by_status.entry(r.get::<String, _>("status")).or_default() += 1;
                }
            }
        }
        let with_outcome = finished;
        let exposure = Self::summary(pool).await?;
        let ready = finished >= RANKING_MINIMUM;
        let state = if ready {
            format!(
                "{} finished matches, at or above the {} needed to compare a learned ranking.",
                finished, RANKING_MINIMUM
            )
        } else {
            format!(
                "Not enough outcome data to compare a learned ranking: {finished} of {RANKING_MINIMUM} finished matches. Exposures are being recorded meanwhile, so the history keeps building."
            )
        };
        Ok(RankingReadiness {
            minimum_finished: RANKING_MINIMUM,
            finished,
            ready,
            state,
            with_outcome,
            with_snapshot,
            outcome_counts,
            open_by_status,
            exposures: exposure.exposures,
            pairs_shown: exposure.pairs_shown,
            pairs_with_outcome: exposure.pairs_with_outcome,
        })
    }
}
