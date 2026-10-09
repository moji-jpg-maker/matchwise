//! Tauri commands for the AI layer. Every command that calls a model goes through [`prepare`], which applies the
//! privacy gate first: no provider, no cloud consent, or a missing key means the model is never contacted.

use super::backend::{is_local_url, HttpBackend};
use super::repo::{self, LogRow, SuggestionRow};
use super::service::{self, Decision, ExtractionView, PairView, Session};
use super::settings::{self, AiConfig, CloudConsent, Mode};
use crate::mm::repository::MmRepository;
use crate::state::AppState;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Gate, key and backend for one request.
async fn prepare(pool: &SqlitePool) -> Result<(AiConfig, Mode, HttpBackend), String> {
    let config = settings::load_config(pool).await?;
    let consent = settings::load_consent(pool).await?;
    let key = settings::load_key().ok().flatten().map(|(k, _)| k);
    let mode = settings::check_gate(&config, consent.as_ref(), key.is_some())?;
    let kind = settings::kind_of(&config.provider).ok_or("Unknown AI provider")?;
    // a key is only ever sent to the provider it was stored for, and never to a local server that does not need one
    let key = if config.provider == "ollama" { None } else { key };
    Ok((config.clone(), mode, HttpBackend::new(kind, &config.base_url, &config.model, key)))
}

#[derive(Serialize)]
pub struct AiSettingsView {
    pub config: AiConfig,
    pub is_cloud: bool,
    pub has_key: bool,
    pub key_source: Option<String>,
    pub consent: Option<CloudConsent>,
    pub consent_text: String,
    pub consent_version: i64,
    pub default_urls: Vec<(String, String)>,
    pub ready: bool,
    pub not_ready_reason: Option<String>,
    pub mode: Option<Mode>,
}

async fn settings_view(pool: &SqlitePool) -> Result<AiSettingsView, String> {
    let config = settings::load_config(pool).await?;
    let consent = settings::load_consent(pool).await?;
    let key = settings::load_key().ok().flatten();
    let gate = settings::check_gate(&config, consent.as_ref(), key.is_some());
    Ok(AiSettingsView {
        is_cloud: config.provider != "none" && !is_local_url(&config.base_url),
        has_key: key.is_some(),
        key_source: key.as_ref().map(|(_, s)| s.to_string()),
        consent,
        consent_text: settings::CLOUD_CONSENT_TEXT.to_string(),
        consent_version: settings::CLOUD_CONSENT_VERSION,
        default_urls: ["ollama", "openai_compatible", "anthropic"].iter().map(|p| (p.to_string(), settings::default_base_url(p).to_string())).collect(),
        ready: gate.is_ok(),
        not_ready_reason: gate.as_ref().err().cloned(),
        mode: gate.ok(),
        config,
    })
}

#[tauri::command]
pub async fn ai_get_settings(state: State<'_, AppState>) -> Result<AiSettingsView, String> {
    settings_view(state.db_manager.pool()).await
}

#[tauri::command]
pub async fn ai_save_settings(
    state: State<'_, AppState>,
    provider: String,
    model: String,
    base_url: Option<String>,
    local_include_sensitive: bool,
) -> Result<AiSettingsView, String> {
    let pool = state.db_manager.pool();
    let base = base_url.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).unwrap_or_else(|| settings::default_base_url(&provider).to_string());
    let config = AiConfig { provider, model: model.trim().to_string(), base_url: base, local_include_sensitive };
    settings::validate_config(&config)?;
    settings::save_config(pool, &config).await?;
    MmRepository::audit(pool, "ai_settings_changed", "settings", "ai", Some(&config.provider)).await;
    settings_view(pool).await
}

#[tauri::command]
pub async fn ai_save_key(state: State<'_, AppState>, key: String) -> Result<AiSettingsView, String> {
    let key = key.trim().to_string();
    if key.is_empty() || key.len() > 400 || key.contains(char::is_whitespace) {
        return Err("That does not look like an API key".into());
    }
    settings::store_key(&key)?;
    MmRepository::audit(state.db_manager.pool(), "ai_key_saved", "settings", "ai", None).await;
    settings_view(state.db_manager.pool()).await
}

#[tauri::command]
pub async fn ai_clear_key(state: State<'_, AppState>) -> Result<AiSettingsView, String> {
    settings::delete_key();
    MmRepository::audit(state.db_manager.pool(), "ai_key_removed", "settings", "ai", None).await;
    settings_view(state.db_manager.pool()).await
}

/// Record (or withdraw) the organization's consent to send redacted text to a provider outside this computer.
#[tauri::command]
pub async fn ai_set_cloud_consent(state: State<'_, AppState>, accept: bool, include_sensitive: bool) -> Result<AiSettingsView, String> {
    let pool = state.db_manager.pool();
    if accept {
        settings::save_consent(pool, include_sensitive).await?;
        MmRepository::audit(pool, "ai_cloud_consent_given", "settings", "ai", Some(if include_sensitive { "with sensitive fields" } else { "without sensitive fields" })).await;
    } else {
        settings::clear_consent(pool).await?;
        MmRepository::audit(pool, "ai_cloud_consent_withdrawn", "settings", "ai", None).await;
    }
    settings_view(pool).await
}

#[derive(Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub is_cloud: bool,
}

/// A harmless fixed request to check the address, key and model name.
#[tauri::command]
pub async fn ai_test_connection(state: State<'_, AppState>) -> Result<TestResult, String> {
    let pool = state.db_manager.pool();
    let (config, mode, backend) = prepare(pool).await?;
    let (system, user) = ("Reply with one JSON object.".to_string(), "Return {\"ok\": true}".to_string());
    let res = super::backend::LlmBackend::complete(&backend, &system, &user, true).await;
    let _ = repo::log_call(pool, "test", &config.provider, &config.model, mode, None, None, system.len() + user.len(), res.as_ref().map(|r| r.len()).unwrap_or(0), res.is_ok(), res.as_ref().err().map(|e| e.to_string()).as_deref(), None).await;
    res.map_err(err)?;
    Ok(TestResult { ok: true, is_cloud: mode.is_cloud })
}

#[derive(Serialize)]
pub struct PromptPreview {
    pub system: String,
    pub user: String,
    pub is_cloud: bool,
    pub include_sensitive: bool,
}

#[tauri::command]
pub async fn ai_preview_extraction(state: State<'_, AppState>, profile_id: String, extra_text: Option<String>) -> Result<PromptPreview, String> {
    let pool = state.db_manager.pool();
    let (_, mode, _) = prepare(pool).await?;
    let (system, user) = service::preview_extraction(pool, &profile_id, extra_text.as_deref(), mode).await?;
    Ok(PromptPreview { system, user, is_cloud: mode.is_cloud, include_sensitive: mode.include_sensitive })
}

#[tauri::command]
pub async fn ai_extract_profile(state: State<'_, AppState>, profile_id: String, extra_text: Option<String>) -> Result<ExtractionView, String> {
    let pool = state.db_manager.pool();
    let (config, mode, backend) = prepare(pool).await?;
    let s = Session { backend: &backend, provider: config.provider, model: config.model, mode };
    service::extract_profile(pool, &s, &profile_id, extra_text.as_deref()).await
}

#[tauri::command]
pub async fn ai_pending_suggestions(state: State<'_, AppState>, profile_id: String) -> Result<Vec<SuggestionRow>, String> {
    repo::pending_suggestions(state.db_manager.pool(), &profile_id).await.map_err(err)
}

/// `decision` is "accept", "accept_verified" or "reject".
#[tauri::command]
pub async fn ai_decide_suggestion(state: State<'_, AppState>, id: i64, decision: String) -> Result<(), String> {
    let d = match decision.as_str() {
        "accept" => Decision::Accept,
        "accept_verified" => Decision::AcceptVerified,
        "reject" => Decision::Reject,
        _ => return Err("decision must be accept, accept_verified or reject".into()),
    };
    service::decide_suggestion(state.db_manager.pool(), id, d).await
}

#[tauri::command]
pub async fn ai_preview_match(state: State<'_, AppState>, match_id: String) -> Result<PromptPreview, String> {
    let pool = state.db_manager.pool();
    let (_, mode, _) = prepare(pool).await?;
    let (system, user) = service::preview_pair(pool, &match_id, mode).await?;
    Ok(PromptPreview { system, user, is_cloud: mode.is_cloud, include_sensitive: mode.include_sensitive })
}

#[tauri::command]
pub async fn ai_analyse_match(state: State<'_, AppState>, match_id: String) -> Result<PairView, String> {
    let pool = state.db_manager.pool();
    let (config, mode, backend) = prepare(pool).await?;
    let s = Session { backend: &backend, provider: config.provider, model: config.model, mode };
    service::analyse_pair(pool, &s, &match_id).await
}

/// The latest stored analysis for a match (works even when AI is switched off).
#[tauri::command]
pub async fn ai_get_match_analysis(state: State<'_, AppState>, match_id: String) -> Result<Option<PairView>, String> {
    service::latest_pair_analysis(state.db_manager.pool(), &match_id).await
}

/// What has been sent to a model, newest first.
#[tauri::command]
pub async fn ai_log(state: State<'_, AppState>, limit: Option<i64>) -> Result<Vec<LogRow>, String> {
    repo::log_rows(state.db_manager.pool(), limit.unwrap_or(50).clamp(1, 200)).await.map_err(err)
}
