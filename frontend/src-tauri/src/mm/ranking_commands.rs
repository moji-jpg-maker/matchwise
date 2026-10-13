//! The ranking comparison, as a command (M7b, ranking v1).
//!
//! Read-only, and separate from `mm_ranking_readiness` on purpose: the gate says whether there is enough history to be
//! worth comparing, this says what the comparison found. Neither one orders a search, and neither one is the gate --
//! the comparison is shown below the gate too, marked as for inspection only.

use super::ranking_repo::{ExclusionCounts, RankingRepo};
use crate::state::AppState;
use matchmaking_core::{OrderingResult, RANKING_MINIMUM};
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The comparison, plus the wording the page shows. Every sentence the interface needs to explain this number comes
/// from here, so the rule behind the figure cannot drift between the report and the explanation of it.
#[derive(Serialize)]
pub struct RankingComparison {
    /// How the label is decided, and why the other two outcomes are left out. One plain sentence, shown as-is.
    pub classification: String,
    /// Which ordering is used to order a search today. The page labels the two columns from this, so a change of
    /// policy is a change of one string rather than a change of layout.
    pub enforced: String,
    /// Whether the history has reached the gate. False does not hide the comparison; it marks it as not enough to
    /// draw a conclusion from.
    pub ready: bool,
    pub minimum_finished: i64,
    /// One plain sentence saying whether this is a finding or a below-the-gate inspection, written for a reader who
    /// will not check the counts behind it.
    pub gate_note: String,
    /// How many matches were examined, before and after the drop rules.
    pub matches_total: usize,
    pub matches_compared: usize,
    pub excluded: ExclusionCounts,
    /// The label breakdown over the compared rows.
    pub progressed: usize,
    pub not_progressed: usize,
    /// The confidence-adjusted `ranking_score`, which is in force today.
    pub baseline: OrderingResult,
    /// The raw coverage-weighted `overall`, before the confidence adjustment.
    pub candidate: OrderingResult,
}

/// The label rule, in one sentence. Kept next to the command rather than in the UI so the page cannot describe a
/// classification the backend does not apply.
pub const CLASSIFICATION: &str = "A pair counted as progressed when its recorded outcome is interested, first conversation, first meeting, continued, relationship formed or married; as not progressed when it is not interested or stopped. A match with no outcome yet, or with an unknown outcome, is left out: an unrecorded move is not a move that went badly.";

/// One plain sentence for whether the history behind the comparison has reached the gate. Phrased so that below it the
/// rates discourage being read as a finding without being hidden -- a matchmaker who asks should still see what there
/// is, with the reason to be cautious attached to it.
pub fn gate_note(finished: i64) -> String {
    if finished >= RANKING_MINIMUM {
        format!(
            "{finished} finished matches, at or above the {RANKING_MINIMUM} the gate asks for, so this comparison covers enough history to be read as a finding rather than as an impression."
        )
    } else {
        format!(
            "Below the gate: {finished} of {RANKING_MINIMUM} finished matches. Shown for inspection only -- with this little history the rates move for reasons other than which ordering is better, and it is still not used to order anything."
        )
    }
}

/// Compare the two orderings against the recorded outcomes. Reports an ordering; it does not produce one.
#[tauri::command]
pub async fn mm_ranking_comparison(state: State<'_, AppState>) -> Result<RankingComparison, String> {
    build_comparison(state.db_manager.pool()).await
}

/// The body of the command, without Tauri, so it can be driven directly by a test.
pub async fn build_comparison(pool: &SqlitePool) -> Result<RankingComparison, String> {
    let (rows, excluded) = RankingRepo::comparison_rows(pool).await.map_err(err)?;
    let totals = matchmaking_core::compare(&rows);
    // Read from the readiness view so the gate is one number, quoted by the page that reports it and by the page that
    // explains the comparison, rather than a fourth literal.
    let readiness = super::exposure_repo::ExposureRepo::ranking_readiness(pool).await.map_err(err)?;
    Ok(RankingComparison {
        classification: CLASSIFICATION.to_string(),
        enforced: "ranking_score".to_string(),
        ready: readiness.ready,
        minimum_finished: readiness.minimum_finished,
        gate_note: gate_note(readiness.finished),
        matches_total: rows.len() + excluded.total(),
        matches_compared: rows.len(),
        excluded,
        progressed: totals.progressed,
        not_progressed: totals.not_progressed,
        baseline: totals.baseline,
        candidate: totals.candidate,
    })
}
