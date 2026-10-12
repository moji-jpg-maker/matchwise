//! Tauri commands that hand the recorded decisions to the matchmaker as a file (M7b).
//!
//! The export is a snapshot for learning and for inspection. It is never sent anywhere by the app: the command returns
//! the text and the UI writes it wherever the matchmaker chooses.

use super::export_repo::{ExportRepo, OutcomeRow};
use crate::mm::repository::{MmRepository, ORG};
use crate::state::AppState;
use matchmaking_core::{export_header, rows_to_csv, ExportRow};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeMap;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The exported document, plus what it contains, so the UI can describe it before writing anything.
#[derive(Serialize)]
pub struct ExportDoc {
    pub generated_at: String,
    pub columns: Vec<String>,
    pub row_count: usize,
    /// How many exported rows have a recorded outcome (the rest are still in progress).
    pub with_outcome: usize,
    pub csv: String,
}

/// How much decision data exists, and whether it is yet enough to be worth learning from. Read-only; the ranking gate
/// in the plan needs a number to compare against, and this is where it comes from.
#[derive(Serialize)]
pub struct DatasetSummary {
    pub matches: i64,
    pub finished: i64,
    pub with_outcome: i64,
    /// Outcome -> count, for matches that have one.
    pub outcome_counts: BTreeMap<String, i64>,
    /// Status -> count, for matches still in progress.
    pub open_by_status: BTreeMap<String, i64>,
    /// How many matches have a stored scorecard (without one, there is nothing to compare a result against).
    pub with_snapshot: i64,
    pub overridden: i64,
    /// Distinct rule-set versions the decisions were made under. More than one means the rules changed mid-history.
    pub rule_set_versions: Vec<String>,
    pub earliest_created_at: Option<String>,
    pub latest_updated_at: Option<String>,
}

fn to_core_row(r: &OutcomeRow) -> ExportRow {
    ExportRow {
        match_id: r.match_id.clone(),
        pair_key: r.pair_key.clone(),
        profile_a: r.profile_a.clone(),
        profile_b: r.profile_b.clone(),
        status: r.status.clone(),
        a_response: r.a_response.clone(),
        b_response: r.b_response.clone(),
        outcome: r.outcome.clone(),
        was_overridden: r.was_overridden,
        rule_set_id: r.rule_set_id.clone(),
        rule_set_version: r.rule_set_version,
        scoring_config_id: r.scoring_config_id.clone(),
        scoring_config_version: r.scoring_config_version,
        snapshot_trigger: r.snapshot_trigger.clone(),
        scorecard_at: r.scorecard_at.clone(),
        overall: r.overall,
        confidence: r.confidence,
        ranking_score: r.ranking_score,
        eligible: r.eligible,
        meets_threshold: r.meets_threshold,
        dimension_scores: r.dimension_scores.clone(),
        created_at: r.created_at.clone(),
        updated_at: r.updated_at.clone(),
    }
}

/// The decision dataset as CSV. One row per tracked match, newest activity first, dimension scores as extra columns.
#[tauri::command]
pub async fn mm_export_outcomes(state: State<'_, AppState>) -> Result<ExportDoc, String> {
    let pool = state.db_manager.pool();
    build_export(pool).await
}

pub async fn build_export(pool: &SqlitePool) -> Result<ExportDoc, String> {
    let rows = ExportRepo::outcome_rows(pool).await.map_err(err)?;
    let dimension_keys = ExportRepo::dimension_keys(pool).await.map_err(err)?;
    let core_rows: Vec<ExportRow> = rows.iter().map(to_core_row).collect();
    let csv = rows_to_csv(&core_rows, &dimension_keys);
    let columns = export_header(&dimension_keys);
    let with_outcome = rows.iter().filter(|r| r.outcome.is_some()).count();
    // The export is itself a use of the matchmaking data, so it is audited -- the counts, never the content.
    MmRepository::audit(
        pool,
        "matchmaking_export",
        "dataset",
        "outcomes",
        Some(&format!("{} rows, {} with an outcome", rows.len(), with_outcome)),
    )
    .await;
    Ok(ExportDoc { generated_at: chrono::Utc::now().to_rfc3339(), columns, row_count: rows.len(), with_outcome, csv })
}

/// Counts and coverage behind the export, for the matchmaker and for the ranking gate.
#[tauri::command]
pub async fn mm_outcome_dataset_summary(state: State<'_, AppState>) -> Result<DatasetSummary, String> {
    let pool = state.db_manager.pool();
    summary(pool).await
}

pub async fn summary(pool: &SqlitePool) -> Result<DatasetSummary, String> {
    let rows = ExportRepo::outcome_rows(pool).await.map_err(err)?;
    let mut outcome_counts: BTreeMap<String, i64> = BTreeMap::new();
    let mut open_by_status: BTreeMap<String, i64> = BTreeMap::new();
    let mut versions: Vec<String> = Vec::new();
    let mut with_snapshot = 0i64;
    let mut overridden = 0i64;
    let mut with_outcome = 0i64;
    let mut earliest: Option<String> = None;
    let mut latest: Option<String> = None;

    for r in &rows {
        match &r.outcome {
            Some(o) => {
                with_outcome += 1;
                *outcome_counts.entry(o.clone()).or_insert(0) += 1;
            }
            // No outcome yet: this match is still moving, so it is counted by where it currently stands.
            None => {
                *open_by_status.entry(r.status.clone()).or_insert(0) += 1;
            }
        }
        if r.scorecard_at.is_some() {
            with_snapshot += 1;
        }
        if r.was_overridden {
            overridden += 1;
        }
        let v = format!("{}@{}", r.rule_set_id, r.rule_set_version);
        if !versions.contains(&v) {
            versions.push(v);
        }
        if earliest.as_ref().map(|e| &r.created_at < e).unwrap_or(true) {
            earliest = Some(r.created_at.clone());
        }
        if latest.as_ref().map(|l| &r.updated_at > l).unwrap_or(true) {
            latest = Some(r.updated_at.clone());
        }
    }

    // Finished = has an outcome. A match closed without one would be a hole the lifecycle is meant to prevent, and the
    // count of finished-without-outcome is deliberately not invented here.
    let finished = with_outcome;
    let matches = rows.len() as i64;
    Ok(DatasetSummary {
        matches,
        finished,
        with_outcome,
        outcome_counts,
        open_by_status,
        with_snapshot,
        overridden,
        rule_set_versions: versions,
        earliest_created_at: earliest,
        latest_updated_at: latest,
    })
}

/// Whether the audit log records an export as having happened (used by the end-to-end test).
pub async fn export_audited(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM mm_audit_log WHERE org_id = ? AND action = 'matchmaking_export'")
        .bind(ORG)
        .fetch_one(pool)
        .await?;
    Ok(row.get::<i64, _>("n"))
}
