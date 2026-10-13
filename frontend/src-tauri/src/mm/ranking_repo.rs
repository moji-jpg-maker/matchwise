//! Reading the recorded decisions as a comparison set (M7b, ranking v1).
//!
//! This is the only app-layer step in the comparison: turn stored matches into `ComparisonRow`s. Everything after it
//! -- the concordance, the ties, the R-precision -- is pure and lives in `matchmaking-core`, so it is tested without a
//! database. Nothing here writes, and nothing here reads a profile's field values.

use super::export_repo::{ExportRepo, OutcomeRow};
use matchmaking_core::{classify_outcome, ComparisonRow, Progress};
use serde::Serialize;
use sqlx::SqlitePool;

/// What was left out, and why. The comparison is only as good as its set, so the exclusions are reported rather than
/// dropped silently: a matchmaker should be able to see that most of the history is still in flight.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ExclusionCounts {
    /// Still moving: no outcome recorded yet, so there is no label to compare against.
    pub no_outcome: usize,
    /// The outcome is `unknown`: recorded, but it will never say which way the pair went.
    pub unknown_outcome: usize,
    /// No stored snapshot, so there are no numbers to put in an order.
    pub no_snapshot: usize,
    /// A snapshot exists but produced no `overall`, which means no `ranking_score` either -- the adjusted score is
    /// derived from the raw one. Nothing can be placed, by either ordering.
    pub no_score: usize,
}

impl ExclusionCounts {
    pub fn total(&self) -> usize {
        self.no_outcome + self.unknown_outcome + self.no_snapshot + self.no_score
    }
}

pub struct RankingRepo;

impl RankingRepo {
    /// The matches that can be compared, plus the counts of those that could not.
    ///
    /// A row is kept when it has a label that says which way it went *and* a stored scorecard carrying numbers. It is
    /// dropped otherwise, in this order: no outcome; a non-progress outcome; no snapshot; no score in the snapshot.
    /// Dropping is preferred to filling a gap, on the same principle as the export's blank-versus-zero.
    pub async fn comparison_rows(pool: &SqlitePool) -> Result<(Vec<ComparisonRow>, ExclusionCounts), sqlx::Error> {
        let rows = ExportRepo::outcome_rows(pool).await?;
        let mut counts = ExclusionCounts::default();
        let mut kept = Vec::new();
        for row in rows {
            let Some(outcome) = row.outcome.as_deref() else {
                counts.no_outcome += 1;
                continue;
            };
            // `unknown` (and any outcome this version does not know) carries no direction. It is not a failure to
            // progress, so it cannot be counted as one.
            if classify_outcome(outcome) == Progress::Excluded {
                counts.unknown_outcome += 1;
                continue;
            }
            if row.scorecard_at.is_none() {
                counts.no_snapshot += 1;
                continue;
            }
            let Some(overall) = row.overall else {
                counts.no_score += 1;
                continue;
            };
            kept.push(ComparisonRow {
                overall: Some(overall),
                ranking_score: row.ranking_score,
                progressed: classify_outcome(outcome) == Progress::Progressed,
            });
        }
        Ok((kept, counts))
    }

}
