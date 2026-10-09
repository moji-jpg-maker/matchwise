//! Talking to language models. The rest of the app depends on [`LlmBackend`] only, so models are interchangeable
//! and tests never need one. Three kinds: Ollama (local), any OpenAI-compatible server (local or cloud), Anthropic.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ollama,
    OpenAiCompatible,
    Anthropic,
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
        let messages = json!([{"role": "system", "content": system}, {"role": "user", "content": user}]);
        match self.kind {
            Kind::Ollama => {
                let mut body = json!({"model": self.model, "messages": messages, "stream": false, "options": {"temperature": 0.2}});
                if json_mode {
                    body["format"] = json!("json");
                }
                let (_, r) = self.post(&format!("{}/api/chat", self.base), &body, &[]).await?;
                text_of(r.pointer("/message/content"), "message")
            }
            Kind::OpenAiCompatible => {
                let mut body = json!({"model": self.model, "messages": messages, "temperature": 0.2});
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
                let body = json!({"model": self.model, "max_tokens": 2048, "temperature": 0.2, "system": system, "messages": [{"role": "user", "content": user}]});
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
