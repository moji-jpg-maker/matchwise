//! AI configuration and the gate that decides whether a model may be used at all.
//!
//! Default stance: AI is off, and when switched on it is meant to run on this computer. A provider that is not
//! on a loopback address counts as "cloud": data leaves the machine, so it needs a recorded consent, an API key
//! kept in the credential store, HTTPS, and sensitive fields stay out unless the consent says otherwise.

use super::backend::{is_local_url, Kind};
use crate::telegram::repo::{delete_meta, get_meta, set_meta};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

pub const CLOUD_CONSENT_VERSION: i64 = 1;

pub const CLOUD_CONSENT_TEXT: &str = "Using a cloud AI provider sends text about your candidates to that provider's servers, outside this computer and outside Matchwise's control.\n\n\
Matchwise removes names, phone numbers, e-mail addresses, @handles, links and long numbers before sending, and never sends contact details, full names, dates of birth or (unless you tick the box below) sensitive fields such as health, finances or religion. \
This cleaning is automatic and not perfect: free text can still identify someone.\n\n\
Only agree if you have a lawful basis and your candidates' consent to this kind of processing, and if you accept the provider's own terms. You can withdraw this consent at any time.";

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiConfig {
    /// "none", "ollama", "openai_compatible" or "anthropic"
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// For a model on this computer: allow sensitive fields (the data does not leave the machine).
    #[serde(default = "yes")]
    pub local_include_sensitive: bool,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig { provider: "none".into(), model: String::new(), base_url: String::new(), local_include_sensitive: true }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CloudConsent {
    pub version: i64,
    pub at: String,
    pub include_sensitive: bool,
}

pub fn default_base_url(provider: &str) -> &'static str {
    match provider {
        "ollama" => "http://localhost:11434",
        "openai_compatible" => "https://api.openai.com/v1",
        "anthropic" => "https://api.anthropic.com",
        _ => "",
    }
}

pub fn kind_of(provider: &str) -> Option<Kind> {
    match provider {
        "ollama" => Some(Kind::Ollama),
        "openai_compatible" => Some(Kind::OpenAiCompatible),
        "anthropic" => Some(Kind::Anthropic),
        _ => None,
    }
}

pub fn validate_config(c: &AiConfig) -> Result<(), String> {
    if c.provider == "none" {
        return Ok(());
    }
    if kind_of(&c.provider).is_none() {
        return Err(format!("Unknown AI provider '{}'", c.provider));
    }
    if c.model.trim().is_empty() || c.model.len() > 100 {
        return Err("Enter the model name (for example the one shown by `ollama list`)".into());
    }
    let url = c.base_url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) || url.contains(char::is_whitespace) || url.len() > 300 {
        return Err("The address must start with http:// or https:// and contain no spaces".into());
    }
    if !is_local_url(url) && url.starts_with("http://") {
        return Err("A provider outside this computer must use https:// so data and keys are encrypted in transit".into());
    }
    Ok(())
}

pub async fn load_config(pool: &SqlitePool) -> Result<AiConfig, String> {
    match get_meta(pool, "ai_config").await.map_err(|e| e.to_string())? {
        Some(j) => serde_json::from_str(&j).map_err(|e| e.to_string()),
        None => Ok(AiConfig::default()),
    }
}

pub async fn save_config(pool: &SqlitePool, c: &AiConfig) -> Result<(), String> {
    set_meta(pool, "ai_config", &serde_json::to_string(c).map_err(|e| e.to_string())?).await.map_err(|e| e.to_string())
}

pub async fn load_consent(pool: &SqlitePool) -> Result<Option<CloudConsent>, String> {
    match get_meta(pool, "ai_cloud_consent").await.map_err(|e| e.to_string())? {
        Some(j) => Ok(serde_json::from_str(&j).ok()),
        None => Ok(None),
    }
}

pub async fn save_consent(pool: &SqlitePool, include_sensitive: bool) -> Result<CloudConsent, String> {
    let c = CloudConsent { version: CLOUD_CONSENT_VERSION, at: chrono::Utc::now().to_rfc3339(), include_sensitive };
    set_meta(pool, "ai_cloud_consent", &serde_json::to_string(&c).map_err(|e| e.to_string())?).await.map_err(|e| e.to_string())?;
    Ok(c)
}

pub async fn clear_consent(pool: &SqlitePool) -> Result<(), String> {
    delete_meta(pool, "ai_cloud_consent").await.map_err(|e| e.to_string())
}

// ------------------------------------------------------------------ API key (never in the database)

const KEYRING_SERVICE: &str = "com.matchwise.desktop";
const KEYRING_ACCOUNT: &str = "ai-api-key";
const KEY_ENV: &str = "MATCHWISE_AI_API_KEY";

pub fn load_key() -> Result<Option<(String, &'static str)>, String> {
    if let Ok(k) = std::env::var(KEY_ENV) {
        if !k.trim().is_empty() {
            return Ok(Some((k.trim().to_string(), "environment")));
        }
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| format!("OS credential store unavailable: {e}"))?;
    match entry.get_password() {
        Ok(k) => Ok(Some((k, "credential store"))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("OS credential store unavailable: {e}")),
    }
}

pub fn store_key(key: &str) -> Result<(), String> {
    if std::env::var(KEY_ENV).map_or(false, |v| !v.trim().is_empty()) {
        return Err(format!("{KEY_ENV} is set and takes precedence; unset it to store a key in the credential store."));
    }
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .and_then(|e| e.set_password(key))
        .map_err(|e| format!("Could not store the key in the OS credential store: {e}. On WSL without a Secret Service, set {KEY_ENV} instead."))
}

pub fn delete_key() {
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        let _ = e.delete_credential();
    }
}

// ------------------------------------------------------------------ the gate

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Mode {
    /// Data leaves this computer.
    pub is_cloud: bool,
    pub include_sensitive: bool,
}

/// May a model be used right now, and under which privacy mode? Checked before every call.
pub fn check_gate(config: &AiConfig, consent: Option<&CloudConsent>, has_key: bool) -> Result<Mode, String> {
    if config.provider == "none" {
        return Err("AI is turned off. Choose a provider on the AI page.".into());
    }
    validate_config(config)?;
    if is_local_url(&config.base_url) {
        return Ok(Mode { is_cloud: false, include_sensitive: config.local_include_sensitive });
    }
    let consent = consent
        .filter(|c| c.version == CLOUD_CONSENT_VERSION)
        .ok_or("This provider is outside your computer. Read and accept the cloud notice on the AI page first.")?;
    if config.provider == "anthropic" && !has_key {
        return Err("Add the provider's API key on the AI page first.".into());
    }
    Ok(Mode { is_cloud: true, include_sensitive: consent.include_sensitive })
}
