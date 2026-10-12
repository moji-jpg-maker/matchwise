//! Commands over the exposure log and the ranking gate.
//!
//! Both are read-only. The gate reports whether a learned ranking could be compared yet; it does not compare one,
//! and nothing here returns an ordering.

use super::exposure_repo::{ExposureRepo, ExposureSummary, RankingReadiness};
use crate::state::AppState;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// What the exposure log holds: how many candidates have been shown, and how many shown pairs were later decided.
#[tauri::command]
pub async fn mm_exposure_summary(state: State<'_, AppState>) -> Result<ExposureSummary, String> {
    ExposureRepo::summary(state.db_manager.pool()).await.map_err(err)
}

/// The gate for building a learned ranking, and the counts behind it. Reports only.
#[tauri::command]
pub async fn mm_ranking_readiness(state: State<'_, AppState>) -> Result<RankingReadiness, String> {
    ExposureRepo::ranking_readiness(state.db_manager.pool()).await.map_err(err)
}
