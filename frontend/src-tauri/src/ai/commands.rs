//! Tauri commands for the AI layer. Every command that calls a model goes through [`prepare`], which applies the
//! privacy gate first: no provider, no cloud consent, or a missing key means the model is never contacted.

use super::backend::{is_local_url, BuiltinBackend, HttpBackend, LlmBackend};
use super::models::{self, ModelFile, ProviderInfo, Readiness, ReadinessInput};
use super::repo::{self, LogRow, SuggestionRow};
use super::service::{self, Decision, ExtractionView, PairView, Session};
use super::settings::{self, AiConfig, CloudConsent, Mode};
use crate::mm::repository::MmRepository;
use crate::state::AppState;
use matchmaking_core::Locale;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

fn locale_of(raw: Option<String>) -> Locale {
    Locale::parse_or_default(raw.as_deref().unwrap_or_default())
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Gate, key and backend for one request. Shared with the bulk-intake commands, which read the gate once per run.
pub(crate) async fn prepare(pool: &SqlitePool) -> Result<(AiConfig, Mode, Box<dyn LlmBackend>), String> {
    prepare_for(pool, Locale::En).await
}

pub(crate) async fn prepare_for(pool: &SqlitePool, locale: Locale) -> Result<(AiConfig, Mode, Box<dyn LlmBackend>), String> {
    let config = settings::load_config(pool).await?;
    let consent = settings::load_consent(pool).await?;
    let key = settings::load_key().ok().flatten().map(|(k, _)| k);
    let mode = settings::check_gate_for(&config, consent.as_ref(), key.is_some(), locale)?;
    if config.provider == settings::BUILTIN {
        let backend: Box<dyn LlmBackend> = Box::new(BuiltinBackend::new(config.model.trim()));
        return Ok((config, mode, backend));
    }
    let kind = settings::kind_of(&config.provider).ok_or_else(|| locale.pick("Unknown AI provider", "ارائه‌دهندهٔ هوش مصنوعی ناشناخته است").to_string())?;
    // a key is only ever sent to the provider it was stored for, and never to a local server that does not need one
    let key = if config.provider == "ollama" { None } else { key };
    let backend: Box<dyn LlmBackend> = Box::new(HttpBackend::new(kind, &config.base_url, &config.model, key));
    Ok((config, mode, backend))
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
    pub catalog: Vec<ProviderInfo>,
    pub builtin_models: Vec<BuiltinModelView>,
    pub readiness: Readiness,
    pub ready: bool,
    pub not_ready_reason: Option<String>,
    pub mode: Option<Mode>,
}

#[derive(Serialize)]
pub struct BuiltinModelView {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub recommended: bool,
    pub file: ModelFile,
    pub size_mb: u64,
}

fn builtin_models_view(locale: Locale) -> Vec<BuiltinModelView> {
    models::models()
        .iter()
        .map(|m| BuiltinModelView {
            id: m.id,
            label: m.label,
            note: m.note_for(locale),
            recommended: m.recommended,
            file: super::backend::app_data_dir().map_or(ModelFile::NotDownloaded, |d| models::model_file_state(d, m)),
            size_mb: crate::summary::summary_engine::get_model_by_name(m.summary_model).map_or(0, |x| x.size_mb as u64),
        })
        .collect()
}

async fn settings_view(pool: &SqlitePool, locale: Locale) -> Result<AiSettingsView, String> {
    let config = settings::load_config(pool).await?;
    let consent = settings::load_consent(pool).await?;
    let key = settings::load_key().ok().flatten();
    let gate = settings::check_gate_for(&config, consent.as_ref(), key.is_some(), locale);
    let model_file = models::find_model(&config.model).and_then(|m| super::backend::app_data_dir().map(|d| models::model_file_state(d, m)));
    let (readiness, reason) = models::readiness_for(&ReadinessInput { config: &config, consent: consent.as_ref(), has_key: key.is_some(), model_file }, locale);
    Ok(AiSettingsView {
        is_cloud: config.provider != "none" && config.provider != settings::BUILTIN && !is_local_url(&config.base_url),
        has_key: key.is_some(),
        key_source: key.as_ref().map(|(_, s)| s.to_string()),
        consent,
        consent_text: settings::consent_text_for(locale).to_string(),
        consent_version: settings::CLOUD_CONSENT_VERSION,
        default_urls: models::provider_catalog_for(locale).iter().map(|p| (p.id.to_string(), p.default_url.to_string())).collect(),
        catalog: models::provider_catalog_for(locale),
        builtin_models: builtin_models_view(locale),
        ready: readiness == Readiness::Ready,
        not_ready_reason: reason,
        readiness,
        mode: gate.ok(),
        config,
    })
}

#[tauri::command]
pub async fn ai_get_settings(state: State<'_, AppState>, locale: Option<String>) -> Result<AiSettingsView, String> {
    settings_view(state.db_manager.pool(), locale_of(locale)).await
}

#[tauri::command]
pub async fn ai_save_settings(
    state: State<'_, AppState>,
    provider: String,
    model: String,
    base_url: Option<String>,
    local_include_sensitive: bool,
    locale: Option<String>,
) -> Result<AiSettingsView, String> {
    let locale = locale_of(locale);
    let pool = state.db_manager.pool();
    let provider = models::normalize_provider(&provider);
    let base = base_url.map(|b| b.trim().to_string()).filter(|b| !b.is_empty()).unwrap_or_else(|| settings::default_base_url(&provider).to_string());
    let config = AiConfig { provider, model: model.trim().to_string(), base_url: base, local_include_sensitive };
    settings::validate_config_for(&config, locale)?;
    settings::save_config(pool, &config).await?;
    MmRepository::audit(pool, "ai_settings_changed", "settings", "ai", Some(&config.provider)).await;
    settings_view(pool, locale).await
}

#[tauri::command]
pub async fn ai_save_key(state: State<'_, AppState>, key: String, locale: Option<String>) -> Result<AiSettingsView, String> {
    let locale = locale_of(locale);
    let key = key.trim().to_string();
    if key.is_empty() || key.len() > 400 || key.contains(char::is_whitespace) {
        return Err(locale.pick("That does not look like an API key", "این شبیه یک کلید API نیست").to_string());
    }
    settings::store_key(&key)?;
    MmRepository::audit(state.db_manager.pool(), "ai_key_saved", "settings", "ai", None).await;
    settings_view(state.db_manager.pool(), locale).await
}

#[tauri::command]
pub async fn ai_clear_key(state: State<'_, AppState>, locale: Option<String>) -> Result<AiSettingsView, String> {
    settings::delete_key();
    MmRepository::audit(state.db_manager.pool(), "ai_key_removed", "settings", "ai", None).await;
    settings_view(state.db_manager.pool(), locale_of(locale)).await
}

/// Record (or withdraw) the organization's consent to send redacted text to a provider outside this computer.
#[tauri::command]
pub async fn ai_set_cloud_consent(state: State<'_, AppState>, accept: bool, include_sensitive: bool, locale: Option<String>) -> Result<AiSettingsView, String> {
    let locale = locale_of(locale);
    let pool = state.db_manager.pool();
    if accept {
        settings::save_consent(pool, include_sensitive).await?;
        MmRepository::audit(pool, "ai_cloud_consent_given", "settings", "ai", Some(if include_sensitive { "with sensitive fields" } else { "without sensitive fields" })).await;
    } else {
        settings::clear_consent(pool).await?;
        MmRepository::audit(pool, "ai_cloud_consent_withdrawn", "settings", "ai", None).await;
    }
    settings_view(pool, locale).await
}

#[derive(Serialize)]
pub struct TestResult {
    pub ok: bool,
    pub is_cloud: bool,
}

/// A harmless fixed request to check the address, key and model name.
#[tauri::command]
pub async fn ai_test_connection(state: State<'_, AppState>, locale: Option<String>) -> Result<TestResult, String> {
    let pool = state.db_manager.pool();
    let (config, mode, backend) = prepare_for(pool, locale_of(locale)).await?;
    let (system, user) = ("Reply with one JSON object.".to_string(), "Return {\"ok\": true}".to_string());
    let res = backend.complete(&system, &user, true).await;
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
    /// False when the rules already fill everything the selection asks for: no model call would be made.
    pub will_send: bool,
}

/// The preview when no model call would be made: the explanation goes where a prompt would, and `will_send`
/// says so, so the panel never shows an empty or misleading prompt.
pub(crate) fn nothing_to_send(explanation: String, mode: Mode) -> PromptPreview {
    PromptPreview { system: String::new(), user: explanation, is_cloud: mode.is_cloud, include_sensitive: mode.include_sensitive, will_send: false }
}

/// The slim prompt that would really be sent. `will_send` is false when nothing would be — the preview then
/// carries an explanation instead of a prompt.
#[tauri::command]
pub async fn ai_preview_extraction(
    state: State<'_, AppState>,
    profile_id: String,
    extra_text: Option<String>,
    locale: Option<String>,
    fields: Option<Vec<String>>,
    with_preferences: Option<bool>,
) -> Result<PromptPreview, String> {
    let pool = state.db_manager.pool();
    let locale = locale_of(locale);
    let (mode, unavailable) = match prepare_for(pool, locale).await {
        Ok((_, mode, _)) => (mode, None),
        Err(reason) => (Mode { is_cloud: false, include_sensitive: false }, Some(reason)),
    };
    if let Some(reason) = unavailable {
        return Ok(nothing_to_send(reason, Mode { is_cloud: false, include_sensitive: false }));
    }
    let prompt = service::preview_extraction(pool, &profile_id, extra_text.as_deref(), mode, locale, fields, with_preferences.unwrap_or(false)).await?;
    Ok(match prompt {
        Some((system, user)) => PromptPreview { system, user, is_cloud: mode.is_cloud, include_sensitive: mode.include_sensitive, will_send: true },
        None => nothing_to_send(
            locale.pick("The rules already fill every selected field, so nothing would be sent to the model.", "قاعده‌ها همهٔ فیلدهای انتخاب‌شده را پر می‌کنند، بنابراین چیزی برای مدل فرستاده نمی‌شود.").to_string(),
            mode,
        ),
    })
}

/// Rules always run. When the gate is closed the rule suggestions are still stored and returned, with
/// `llm_status` "unavailable" and the gate's reason: a run never comes back empty-handed because AI is off.
#[tauri::command]
pub async fn ai_extract_profile(
    state: State<'_, AppState>,
    profile_id: String,
    extra_text: Option<String>,
    locale: Option<String>,
    fields: Option<Vec<String>>,
    with_preferences: Option<bool>,
) -> Result<ExtractionView, String> {
    let pool = state.db_manager.pool();
    let locale = locale_of(locale);
    let with_preferences = with_preferences.unwrap_or(false);
    let gate = prepare_for(pool, locale).await;
    let (view, reason) = match gate {
        Ok((config, mode, backend)) => {
            let s = Session { backend: backend.as_ref(), provider: config.provider, model: config.model, mode, locale };
            (service::extract_profile(pool, Some(&s), &profile_id, extra_text.as_deref(), fields, with_preferences).await?, None)
        }
        Err(reason) => (service::extract_profile(pool, None, &profile_id, extra_text.as_deref(), fields, with_preferences).await?, Some(reason)),
    };
    Ok(match reason {
        Some(reason) => ExtractionView { llm_status: "unavailable".into(), llm_error: Some(reason), ..view },
        None => view,
    })
}

#[derive(Serialize)]
pub struct ExtractionPresetsView {
    pub presets: Vec<repo::PresetRow>,
    pub fields: Vec<PresetFieldView>,
}

/// One selectable field in the presets editor: the key and its label, never a value.
#[derive(Serialize)]
pub struct PresetFieldView {
    pub key: String,
    pub label: String,
    pub sensitive: bool,
}

async fn presets_view(pool: &SqlitePool, locale: Locale) -> Result<ExtractionPresetsView, String> {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let known: Vec<String> = matchmaking_core::FieldRegistry::defs(&reg).map(|def| def.key.clone()).collect();
    let fields = matchmaking_core::FieldRegistry::defs(&reg)
        .filter(|def| !matchmaking_core::NEVER_SHARED.contains(&def.key.as_str()) && !matches!(def.kind, matchmaking_core::FieldKind::Records(_)))
        .map(|def| PresetFieldView { key: def.key.clone(), label: if locale == Locale::Fa { def.label_fa.clone().unwrap_or_else(|| def.label.clone()) } else { def.label.clone() }, sensitive: def.sensitive })
        .collect();
    let presets = repo::extraction_presets(pool, &known).await.map_err(err)?;
    Ok(ExtractionPresetsView { presets, fields })
}

#[tauri::command]
pub async fn mm_extraction_presets(state: State<'_, AppState>, locale: Option<String>) -> Result<ExtractionPresetsView, String> {
    presets_view(state.db_manager.pool(), locale_of(locale)).await
}

/// `id` empty or absent creates a preset. Audit records the name and the number of fields, never the fields.
#[tauri::command]
pub async fn mm_save_extraction_preset(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    fields: Vec<String>,
    with_preferences: bool,
    is_default: bool,
    locale: Option<String>,
) -> Result<ExtractionPresetsView, String> {
    let pool = state.db_manager.pool();
    let locale = locale_of(locale);
    if name.trim().is_empty() {
        return Err(locale.pick("A preset needs a name", "یک پیش‌تنظیم به نام نیاز دارد").to_string());
    }
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let known: Vec<String> = matchmaking_core::FieldRegistry::defs(&reg).map(|def| def.key.clone()).collect();
    let fields: Vec<String> = fields.into_iter().filter(|key| known.contains(key)).collect();
    if fields.is_empty() {
        return Err(locale.pick("Choose at least one field", "دست‌کم یک فیلد را انتخاب کنید").to_string());
    }
    let id = id.as_deref().filter(|id| !id.trim().is_empty());
    repo::save_extraction_preset(pool, id, &name, &fields, with_preferences, is_default).await.map_err(err)?;
    MmRepository::audit(pool, "extraction_preset_saved", "settings", "extraction", Some(&format!("{} ({} fields)", name.trim(), fields.len()))).await;
    presets_view(pool, locale).await
}

#[tauri::command]
pub async fn mm_delete_extraction_preset(state: State<'_, AppState>, id: String, locale: Option<String>) -> Result<ExtractionPresetsView, String> {
    let pool = state.db_manager.pool();
    let locale = locale_of(locale);
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let known: Vec<String> = matchmaking_core::FieldRegistry::defs(&reg).map(|def| def.key.clone()).collect();
    let before = repo::extraction_presets(pool, &known).await.map_err(err)?;
    repo::delete_extraction_preset(pool, &id).await.map_err(err)?;
    let name = before.iter().find(|preset| preset.id == id).map_or(String::from("?"), |preset| preset.name.clone());
    MmRepository::audit(pool, "extraction_preset_deleted", "settings", "extraction", Some(&name)).await;
    presets_view(pool, locale).await
}

#[tauri::command]
pub async fn ai_pending_suggestions(state: State<'_, AppState>, profile_id: String) -> Result<Vec<SuggestionRow>, String> {
    repo::pending_suggestions(state.db_manager.pool(), &profile_id).await.map_err(err)
}

/// `decision` is "accept", "accept_verified" or "reject".
#[tauri::command]
pub async fn ai_decide_suggestion(state: State<'_, AppState>, id: i64, decision: String, locale: Option<String>) -> Result<(), String> {
    let locale = locale_of(locale);
    let d = match decision.as_str() {
        "accept" => Decision::Accept,
        "accept_verified" => Decision::AcceptVerified,
        "reject" => Decision::Reject,
        _ => return Err(locale.pick("decision must be accept, accept_verified or reject", "تصمیم باید accept، accept_verified یا reject باشد").to_string()),
    };
    service::decide_suggestion(state.db_manager.pool(), id, d, locale).await
}

#[tauri::command]
pub async fn ai_preview_match(state: State<'_, AppState>, match_id: String, locale: Option<String>) -> Result<PromptPreview, String> {
    let pool = state.db_manager.pool();
    let locale = locale_of(locale);
    let (_, mode, _) = prepare_for(pool, locale).await?;
    let (system, user) = service::preview_pair(pool, &match_id, mode, locale).await?;
    Ok(PromptPreview { system, user, is_cloud: mode.is_cloud, include_sensitive: mode.include_sensitive, will_send: true })
}

#[tauri::command]
pub async fn ai_analyse_match(state: State<'_, AppState>, match_id: String, locale: Option<String>) -> Result<PairView, String> {
    let pool = state.db_manager.pool();
    let locale = locale_of(locale);
    let (config, mode, backend) = prepare_for(pool, locale).await?;
    let s = Session { backend: backend.as_ref(), provider: config.provider, model: config.model, mode, locale };
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
