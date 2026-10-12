//! Commands for versioned scoring configs.
//!
//! A config is validated before it is stored, so a saved version can always be used to read a scorecard. Versions
//! are append-only: saving never rewrites an old version, because stored snapshots name the version that judged
//! them.

use super::repository::{MmRepository, VersionRow};
use super::scoring_config_repo::{ScoringConfigRepo, ScoringConfigSummary};
use crate::state::AppState;
use matchmaking_core::{validate_scoring_config, ScoringConfig};
use serde::Serialize;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
pub struct VersionInfo {
    pub version: u32,
    pub created_at: String,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct ScoringConfigView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub archived: bool,
    pub current_version: u32,
    /// The version shown (may be older than current).
    pub version: u32,
    pub definition: ScoringConfig,
    pub versions: Vec<VersionInfo>,
}

fn view(meta: super::scoring_config_repo::ScoringConfigRow, def: ScoringConfig, versions: Vec<VersionRow>) -> ScoringConfigView {
    ScoringConfigView {
        id: meta.id,
        name: meta.name,
        description: meta.description,
        archived: meta.archived,
        current_version: meta.current_version,
        version: def_version(&versions, meta.current_version),
        definition: def,
        versions: versions.into_iter().map(|v| VersionInfo { version: v.version, created_at: v.created_at, note: v.note }).collect(),
    }
}

/// The version shown is the newest one at or below the requested number; the caller asked for a specific version,
/// so when it found a definition there is always a matching row.
fn def_version(versions: &[VersionRow], fallback: u32) -> u32 {
    versions.first().map(|v| v.version).unwrap_or(fallback)
}

#[tauri::command]
pub async fn mm_list_scoring_configs(
    state: State<'_, AppState>,
    include_archived: Option<bool>,
) -> Result<Vec<ScoringConfigSummary>, String> {
    let pool = state.db_manager.pool();
    MmRepository::ensure_default_rule_set(pool).await.map_err(err)?;
    ScoringConfigRepo::ensure_scoring_configs(pool).await.map_err(err)?;
    ScoringConfigRepo::list(pool, include_archived.unwrap_or(false)).await.map_err(err)
}

#[tauri::command]
pub async fn mm_get_scoring_config(
    state: State<'_, AppState>,
    id: String,
    version: Option<u32>,
) -> Result<ScoringConfigView, String> {
    let (meta, def, versions) = ScoringConfigRepo::get(state.db_manager.pool(), &id, version)
        .await
        .map_err(err)?
        .ok_or("Scoring config or version not found")?;
    Ok(view(meta, def, versions))
}

/// Live validation for the editor: nothing is saved.
#[tauri::command]
pub async fn mm_validate_scoring_config(definition: ScoringConfig) -> Result<Vec<String>, String> {
    Ok(validate_scoring_config(&definition))
}

/// Create a config (`id` = None) or save a new version of an existing one. Refuses anything invalid, so a stored
/// config can always be used. The audit record names the version number only -- never the numbers themselves,
/// which live in the config tables where they belong.
#[tauri::command]
pub async fn mm_save_scoring_config(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    description: Option<String>,
    definition: ScoringConfig,
    note: Option<String>,
) -> Result<ScoringConfigView, String> {
    let pool = state.db_manager.pool();
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("A scoring config needs a name".into());
    }
    let issues = validate_scoring_config(&definition);
    if !issues.is_empty() {
        return Err(format!("Not saved. {}", issues.join("; ")));
    }
    if ScoringConfigRepo::name_taken(pool, &name, id.as_deref()).await.map_err(err)? {
        return Err(format!("A scoring config named '{name}' already exists"));
    }
    let description = description.unwrap_or_default();
    let note = note.filter(|n| !n.trim().is_empty());
    let (saved_id, version) = match id {
        Some(existing) => {
            let v = ScoringConfigRepo::save_version(pool, &existing, &name, &description, &definition, note.as_deref())
                .await
                .map_err(err)?;
            (existing, v)
        }
        None => {
            let new_id = ScoringConfigRepo::create(pool, &name, &description, &definition, note.as_deref()).await.map_err(err)?;
            (new_id, 1)
        }
    };
    MmRepository::audit(pool, "scoring_config_save", "scoring_config", &saved_id, Some(&format!("v{version}"))).await;
    let (meta, def, versions) = ScoringConfigRepo::get(pool, &saved_id, Some(version))
        .await
        .map_err(err)?
        .ok_or("Scoring config not found after saving")?;
    Ok(view(meta, def, versions))
}

/// Archive or restore a config. Archiving is a label, not a delete: rule sets already using it keep working, and a
/// stored snapshot still resolves its version.
#[tauri::command]
pub async fn mm_archive_scoring_config(state: State<'_, AppState>, id: String, archived: bool) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let affected = ScoringConfigRepo::set_archived(pool, &id, archived).await.map_err(err)?;
    if affected == 0 {
        return Err("Scoring config not found".into());
    }
    MmRepository::audit(pool, if archived { "scoring_config_archive" } else { "scoring_config_restore" }, "scoring_config", &id, None).await;
    Ok(())
}
