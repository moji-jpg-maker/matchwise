//! Talking to language models. The rest of the app depends on [`LlmBackend`] only, so models are interchangeable
//! and tests never need one. Three kinds: Ollama (local), any OpenAI-compatible server (local or cloud), Anthropic.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use super::models::TaskProfile;

#[derive(Debug, Clone, PartialEq)]
pub enum AiError {
    /// Wrong or missing API key (401/403).
    Unauthorized,
    RateLimited,
    /// Cannot reach the server (not running, wrong address, timeout).
    Unreachable(String),
    /// The server answered with an error.
    Server(String),
    /// The answer was not in the expected shape.
    BadResponse(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::Unauthorized => write!(f, "The AI provider rejected the API key"),
            AiError::RateLimited => write!(f, "The AI provider is rate limiting requests; try again shortly"),
            AiError::Unreachable(m) => write!(f, "Could not reach the AI provider: {m}"),
            AiError::Server(m) => write!(f, "The AI provider returned an error: {m}"),
            AiError::BadResponse(m) => write!(f, "The AI provider's answer could not be read: {m}"),
        }
    }
}

#[async_trait]
pub trait LlmBackend: Send + Sync {
    /// One question, one answer. `json` asks the server to produce JSON where it supports that.
    async fn complete(&self, system: &str, user: &str, json: bool) -> Result<String, AiError>;

    /// Like [`complete`](Self::complete), run with the settings of one matchmaking task. Backends that have no
    /// per-task settings just answer the plain request.
    async fn complete_for(&self, _task: TaskProfile, system: &str, user: &str, json: bool) -> Result<String, AiError> {
        self.complete(system, user, json).await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ollama,
    OpenAiCompatible,
    Anthropic,
}

static APP_DATA_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Where the built-in models live (the app data directory). Set once at startup.
pub fn set_app_data_dir(dir: std::path::PathBuf) {
    let _ = APP_DATA_DIR.set(dir);
}

pub fn app_data_dir() -> Option<&'static std::path::PathBuf> {
    APP_DATA_DIR.get()
}

/// The model that ships with the app, run on this computer through the llama-helper sidecar. No network.
pub struct BuiltinBackend {
    model: String,
}

impl BuiltinBackend {
    pub fn new(model: &str) -> Self {
        Self { model: model.to_string() }
    }
}

#[async_trait]
impl LlmBackend for BuiltinBackend {
    /// The sidecar has no JSON mode; the prompts already demand a JSON object and the parsers extract it.
    async fn complete(&self, system: &str, user: &str, _json: bool) -> Result<String, AiError> {
        self.run(None, system, user).await
    }

    async fn complete_for(&self, task: TaskProfile, system: &str, user: &str, _json: bool) -> Result<String, AiError> {
        self.run(Some(task), system, user).await
    }
}

impl BuiltinBackend {
    async fn run(&self, task: Option<TaskProfile>, system: &str, user: &str) -> Result<String, AiError> {
        let dir = APP_DATA_DIR.get().ok_or_else(|| AiError::Unreachable("the app data directory is not known yet".into()))?;
        let def = super::models::find_model(&self.model).ok_or_else(|| AiError::Server(format!("Unknown built-in model '{}'", self.model)))?;
        if super::models::model_file_state(dir, def) != super::models::ModelFile::Downloaded {
            return Err(AiError::Unreachable(format!("The model '{}' is not downloaded yet. Download it on the AI page.", def.label)));
        }
        let overrides = task.and_then(|t| t.overrides_for(def));
        crate::summary::summary_engine::generate_with_builtin_overrides(dir, def.summary_model, system, user, None, overrides.as_ref())
            .await
            .map_err(|e| AiError::Server(e.to_string().chars().take(300).collect()))
    }
}

pub struct HttpBackend {
    kind: Kind,
    client: reqwest::Client,
    base: String,
    model: String,
    key: Option<String>,
}

impl HttpBackend {
    pub fn new(kind: Kind, base_url: &str, model: &str, key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300)) // a local model on modest hardware can be slow
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self { kind, client, base: base_url.trim_end_matches('/').to_string(), model: model.to_string(), key }
    }

    fn redact(&self, s: String) -> String {
        match &self.key {
            Some(k) if !k.is_empty() => s.replace(k.as_str(), "<key>"),
            _ => s,
        }
    }

    async fn run(&self, task: Option<TaskProfile>, system: &str, user: &str, json_mode: bool) -> Result<String, AiError> {
        let temperature = task.map_or(0.2, |t| t.temperature());
        let max_tokens = task.map_or(2048, |t| t.max_tokens());
        self.run_with(temperature, max_tokens, system, user, json_mode).await
    }

    async fn post(&self, url: &str, body: &Value, headers: &[(&str, String)]) -> Result<(u16, Value), AiError> {
        let mut req = self.client.post(url).json(body);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let resp = req.send().await.map_err(|e| AiError::Unreachable(self.redact(e.to_string())))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| AiError::Unreachable(self.redact(e.to_string())))?;
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        match status {
            200..=299 => Ok((status, json)),
            401 | 403 => Err(AiError::Unauthorized),
            429 => Err(AiError::RateLimited),
            _ => {
                let msg = json
                    .pointer("/error/message")
                    .or_else(|| json.get("error"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| format!("HTTP {status}"));
                Err(AiError::Server(self.redact(msg.chars().take(300).collect())))
            }
        }
    }
}

fn text_of(v: Option<&Value>, what: &str) -> Result<String, AiError> {
    v.and_then(|x| x.as_str()).map(String::from).ok_or_else(|| AiError::BadResponse(format!("no {what} in the response")))
}

#[async_trait]
impl LlmBackend for HttpBackend {
    async fn complete(&self, system: &str, user: &str, json_mode: bool) -> Result<String, AiError> {
        self.run(None, system, user, json_mode).await
    }

    async fn complete_for(&self, task: TaskProfile, system: &str, user: &str, json_mode: bool) -> Result<String, AiError> {
        self.run(Some(task), system, user, json_mode).await
    }
}

impl HttpBackend {
    async fn run_with(&self, temperature: f32, max_tokens: i32, system: &str, user: &str, json_mode: bool) -> Result<String, AiError> {
        let messages = json!([{"role": "system", "content": system}, {"role": "user", "content": user}]);
        match self.kind {
            Kind::Ollama => {
                let mut body = json!({"model": self.model, "messages": messages, "stream": false, "options": {"temperature": temperature, "num_predict": max_tokens}});
                if json_mode {
                    body["format"] = json!("json");
                }
                let (_, r) = self.post(&format!("{}/api/chat", self.base), &body, &[]).await?;
                text_of(r.pointer("/message/content"), "message")
            }
            Kind::OpenAiCompatible => {
                let mut body = json!({"model": self.model, "messages": messages, "temperature": temperature});
                if json_mode {
                    body["response_format"] = json!({"type": "json_object"});
                }
                let mut headers = vec![];
                if let Some(k) = &self.key {
                    headers.push(("Authorization", format!("Bearer {k}")));
                }
                let url = format!("{}/chat/completions", self.base);
                let first = self.post(&url, &body, &headers).await;
                // some servers reject response_format: ask again without it
                let (_, r) = match first {
                    Err(AiError::Server(m)) if json_mode && m.to_lowercase().contains("response_format") => {
                        body.as_object_mut().map(|o| o.remove("response_format"));
                        self.post(&url, &body, &headers).await?
                    }
                    other => other?,
                };
                text_of(r.pointer("/choices/0/message/content"), "message")
            }
            Kind::Anthropic => {
                let body = json!({"model": self.model, "max_tokens": max_tokens, "temperature": temperature, "system": system, "messages": [{"role": "user", "content": user}]});
                let headers = [("x-api-key", self.key.clone().unwrap_or_default()), ("anthropic-version", "2023-06-01".to_string())];
                let (_, r) = self.post(&format!("{}/v1/messages", self.base), &body, &headers).await?;
                text_of(r.pointer("/content/0/text"), "text")
            }
        }
    }
}

/// Loopback addresses only. Anything else (a LAN machine, a cloud service) is treated as leaving this computer.
pub fn is_local_url(url: &str) -> bool {
    let rest = url.trim().strip_prefix("http://").or_else(|| url.trim().strip_prefix("https://")).unwrap_or(url.trim());
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false; // user:pass@host tricks
    }
    let host = if let Some(stripped) = authority.strip_prefix('[') {
        stripped.split(']').next().unwrap_or("")
    } else {
        authority.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { authority }).unwrap_or(authority)
    };
    matches!(host.to_lowercase().as_str(), "localhost" | "127.0.0.1" | "::1")
}
