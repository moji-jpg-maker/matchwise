use super::{decide_proposal, repo, run, run_batch};
use crate::{
    ai::{commands::prepare_for, service::Session},
    state::AppState,
};
use matchmaking_core::Locale;
use serde_json::Value;
use tauri::State;

#[tauri::command]
pub async fn agent_run_now(
    state: State<'_, AppState>,
    profile_id: Option<String>,
    batch_id: Option<String>,
    locale: Option<String>,
) -> Result<Vec<i64>, String> {
    let pool = state.db_manager.pool();
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    let (config, mode, backend) = prepare_for(pool, locale).await?;
    let session = Session {
        backend: backend.as_ref(),
        provider: config.provider.clone(),
        model: config.model.clone(),
        mode,
        locale,
        agent_config: Some(config),
    };
    match (profile_id, batch_id) {
        (Some(target), None) => Ok(vec![run(pool, &session, &target, None).await?]),
        (None, Some(batch)) => run_batch(pool, &session, &batch).await,
        _ => Err(locale
            .pick(
                "Select one profile or intake batch",
                "یک پروفایل یا دسته ورودی انتخاب کنید",
            )
            .into()),
    }
}

#[tauri::command]
pub async fn agent_runs(state: State<'_, AppState>) -> Result<Vec<repo::Run>, String> {
    repo::runs(state.db_manager.pool()).await
}

#[tauri::command]
pub async fn agent_run_detail(
    state: State<'_, AppState>,
    run_id: i64,
) -> Result<Vec<repo::Trace>, String> {
    repo::traces(state.db_manager.pool(), Some(run_id), false).await
}

#[tauri::command]
pub async fn agent_proposals(state: State<'_, AppState>) -> Result<Vec<repo::Trace>, String> {
    repo::traces(state.db_manager.pool(), None, true).await
}

#[tauri::command]
pub async fn agent_decide_proposal(
    state: State<'_, AppState>,
    proposal_id: i64,
    approve: bool,
    args: Option<Value>,
    locale: Option<String>,
) -> Result<(), String> {
    let pool = state.db_manager.pool();
    if !approve {
        return decide_proposal(pool, None, proposal_id, false, None).await;
    }
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    let (config, mode, backend) = prepare_for(pool, locale).await?;
    let session = Session {
        backend: backend.as_ref(),
        provider: config.provider.clone(),
        model: config.model.clone(),
        mode,
        locale,
        agent_config: Some(config),
    };
    decide_proposal(pool, Some(&session), proposal_id, true, args).await
}

#[tauri::command]
pub async fn agent_veto(state: State<'_, AppState>, proposal_id: i64) -> Result<(), String> {
    super::veto(state.db_manager.pool(), proposal_id).await
}
