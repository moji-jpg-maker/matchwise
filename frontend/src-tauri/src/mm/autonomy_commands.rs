//! Commands for the autonomy policy and the agent's undo (M12 phase 1).

use super::autonomy::undo_action;
use super::autonomy_repo::{AgentAction, AutonomyRepo};
use super::repository::MmRepository;
use crate::state::AppState;
use matchmaking_core::autonomy::{Actor, AutonomyPolicy};
use matchmaking_core::Locale;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The stored policy, the fields the agent applied that can still be undone, and whether a preset still matches.
#[derive(Debug, Serialize)]
pub struct PolicyView {
    pub policy: AutonomyPolicy,
    pub custom: bool,
    pub actions: Vec<AgentAction>,
}

pub async fn get_policy(pool: &SqlitePool, profile_id: Option<&str>) -> Result<PolicyView, String> {
    let policy = AutonomyRepo::load_policy(pool).await.map_err(err)?;
    let actions = match profile_id {
        Some(id) => AutonomyRepo::active_actions(pool, id).await.map_err(err)?,
        None => vec![],
    };
    Ok(PolicyView { custom: !policy.matches_preset(), policy, actions })
}

/// Save a policy. Only the matchmaker can: the agent has no tool for it (`HumanOnly::AiSettings`).
pub async fn save_policy(pool: &SqlitePool, policy: &AutonomyPolicy) -> Result<PolicyView, String> {
    let saved = AutonomyRepo::save_policy(pool, policy).await?;
    MmRepository::audit(pool, "agent_policy_saved", "agent_policy", &saved.version.to_string(), Some(&format!("{:?}", saved.level))).await;
    Ok(PolicyView { custom: !saved.matches_preset(), policy: saved, actions: vec![] })
}

/// The kill switch. Takes effect on the next agent action; a stored policy keeps everything else as it was.
pub async fn set_paused(pool: &SqlitePool, paused: bool) -> Result<PolicyView, String> {
    let mut policy = AutonomyRepo::load_policy(pool).await.map_err(err)?;
    policy.paused = paused;
    let saved = AutonomyRepo::save_policy(pool, &policy).await?;
    MmRepository::audit(pool, if paused { "agent_paused" } else { "agent_resumed" }, "agent_policy", &saved.version.to_string(), None).await;
    Ok(PolicyView { custom: !saved.matches_preset(), policy: saved, actions: vec![] })
}

#[tauri::command]
pub async fn agent_get_policy(state: State<'_, AppState>, profile_id: Option<String>) -> Result<PolicyView, String> {
    get_policy(state.db_manager.pool(), profile_id.as_deref()).await
}

#[tauri::command]
pub async fn agent_save_policy(state: State<'_, AppState>, policy: AutonomyPolicy) -> Result<PolicyView, String> {
    save_policy(state.db_manager.pool(), &policy).await
}

#[tauri::command]
pub async fn agent_set_paused(state: State<'_, AppState>, paused: bool) -> Result<PolicyView, String> {
    set_paused(state.db_manager.pool(), paused).await
}

#[tauri::command]
pub async fn agent_undo(state: State<'_, AppState>, action_id: i64, locale: Option<String>) -> Result<AgentAction, String> {
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    undo_action(state.db_manager.pool(), action_id, Actor::Matchmaker, locale).await
}
