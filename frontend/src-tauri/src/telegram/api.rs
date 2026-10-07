//! Talking to Telegram. The bot logic never touches the network: it depends on [`TelegramApi`], which has an
//! HTTP implementation for production and is trivially replaced in tests.

use super::types::{Keyboard, Reply, Update};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum TgError {
    /// The token is wrong or was revoked (HTTP 401).
    Unauthorized,
    /// The user blocked the bot or deleted the chat (HTTP 403).
    Forbidden,
    /// Too many requests; wait this long.
    RateLimited { retry_after_secs: u64 },
    /// Telegram rejected the request itself; retrying will not help.
    BadRequest(String),
    /// Connection problem or timeout; worth retrying.
    Network(String),
    Other(String),
}

impl std::fmt::Display for TgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TgError::Unauthorized => write!(f, "Telegram rejected the bot token"),
            TgError::Forbidden => write!(f, "the person blocked the bot or left the chat"),
            TgError::RateLimited { retry_after_secs } => write!(f, "rate limited, retry in {retry_after_secs}s"),
            TgError::BadRequest(m) => write!(f, "Telegram rejected the request: {m}"),
            TgError::Network(m) => write!(f, "network problem: {m}"),
            TgError::Other(m) => write!(f, "{m}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BotInfo {
    pub id: i64,
    pub username: String,
}

#[async_trait]
pub trait TelegramApi: Send + Sync {
    async fn get_me(&self) -> Result<BotInfo, TgError>;
    async fn get_updates(&self, offset: i64, timeout_secs: u32) -> Result<Vec<Update>, TgError>;
    async fn send_message(&self, chat_id: i64, html: &str, keyboard: Option<&Keyboard>) -> Result<i64, TgError>;
    async fn answer_callback(&self, id: &str, text: Option<&str>) -> Result<(), TgError>;
    async fn edit_markup(&self, chat_id: i64, message_id: i64, keyboard: Option<&Keyboard>) -> Result<(), TgError>;
    async fn set_commands(&self) -> Result<(), TgError>;
}

/// Perform one reply. Errors for purely cosmetic actions (a toast, button clean-up) are ignored by the caller.
pub async fn perform(api: &dyn TelegramApi, reply: &Reply) -> Result<(), TgError> {
    match reply {
        Reply::Send { chat_id, text, keyboard } => api.send_message(*chat_id, text, keyboard.as_ref()).await.map(|_| ()),
        Reply::AnswerCallback { id, text } => api.answer_callback(id, text.as_deref()).await,
        Reply::EditMarkup { chat_id, message_id, keyboard } => api.edit_markup(*chat_id, *message_id, keyboard.as_ref()).await,
    }
}

pub struct HttpTelegramApi {
    client: reqwest::Client,
    base: String,
    token: String,
}

impl HttpTelegramApi {
    pub fn new(token: &str) -> Self {
        Self::with_base("https://api.telegram.org", token)
    }

    /// `base` is configurable so tests can talk to a local stand-in server.
    pub fn with_base(base: &str, token: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(45)) // longer than the 25s long-poll
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("reqwest client");
        Self { client, base: base.trim_end_matches('/').to_string(), token: token.to_string() }
    }

    /// Error text must never contain the token: request URLs do.
    fn redact(&self, s: String) -> String {
        s.replace(&self.token, "<token>")
    }

    async fn call(&self, method: &str, body: Value) -> Result<Value, TgError> {
        let url = format!("{}/bot{}/{}", self.base, self.token, method);
        let resp = self.client.post(&url).json(&body).send().await.map_err(|e| TgError::Network(self.redact(e.to_string())))?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| TgError::Network(self.redact(e.to_string())))?;
        let json: Value = serde_json::from_str(&text).map_err(|_| TgError::Other(format!("unexpected response (HTTP {status})")))?;
        if json.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            return Ok(json.get("result").cloned().unwrap_or(Value::Null));
        }
        let code = json.get("error_code").and_then(|v| v.as_i64()).unwrap_or(status.as_u16() as i64);
        let desc = json.get("description").and_then(|v| v.as_str()).unwrap_or("unknown error").to_string();
        Err(match code {
            401 => TgError::Unauthorized,
            403 => TgError::Forbidden,
            429 => TgError::RateLimited {
                retry_after_secs: json.pointer("/parameters/retry_after").and_then(|v| v.as_u64()).unwrap_or(5),
            },
            400 => TgError::BadRequest(self.redact(desc)),
            _ => TgError::Other(self.redact(format!("{code}: {desc}"))),
        })
    }
}

fn markup(keyboard: Option<&Keyboard>) -> Value {
    json!({ "inline_keyboard": keyboard.cloned().unwrap_or_default() })
}

#[async_trait]
impl TelegramApi for HttpTelegramApi {
    async fn get_me(&self) -> Result<BotInfo, TgError> {
        let r = self.call("getMe", json!({})).await?;
        Ok(BotInfo {
            id: r.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
            username: r.get("username").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        })
    }

    async fn get_updates(&self, offset: i64, timeout_secs: u32) -> Result<Vec<Update>, TgError> {
        let r = self
            .call("getUpdates", json!({ "offset": offset, "timeout": timeout_secs, "allowed_updates": ["message", "callback_query"] }))
            .await?;
        // One malformed update must not stop the others: parse individually and skip what we cannot read.
        let items = r.as_array().cloned().unwrap_or_default();
        Ok(items.into_iter().filter_map(|u| serde_json::from_value::<Update>(u).ok()).collect())
    }

    async fn send_message(&self, chat_id: i64, html: &str, keyboard: Option<&Keyboard>) -> Result<i64, TgError> {
        let mut body = json!({ "chat_id": chat_id, "text": html, "parse_mode": "HTML", "disable_web_page_preview": true });
        if let Some(k) = keyboard {
            body["reply_markup"] = markup(Some(k));
        }
        let r = self.call("sendMessage", body).await?;
        Ok(r.get("message_id").and_then(|v| v.as_i64()).unwrap_or(0))
    }

    async fn answer_callback(&self, id: &str, text: Option<&str>) -> Result<(), TgError> {
        let mut body = json!({ "callback_query_id": id });
        if let Some(t) = text {
            body["text"] = json!(t);
        }
        self.call("answerCallbackQuery", body).await.map(|_| ())
    }

    async fn edit_markup(&self, chat_id: i64, message_id: i64, keyboard: Option<&Keyboard>) -> Result<(), TgError> {
        self.call("editMessageReplyMarkup", json!({ "chat_id": chat_id, "message_id": message_id, "reply_markup": markup(keyboard) }))
            .await
            .map(|_| ())
    }

    async fn set_commands(&self) -> Result<(), TgError> {
        let commands = json!([
            {"command": "profile", "description": "See your profile"},
            {"command": "edit", "description": "Complete or change your profile"},
            {"command": "preferences", "description": "What you are looking for"},
            {"command": "matches", "description": "Your introductions"},
            {"command": "status", "description": "Where things stand"},
            {"command": "settings", "description": "Notifications and privacy"},
            {"command": "help", "description": "How this works"}
        ]);
        self.call("setMyCommands", json!({ "commands": commands })).await.map(|_| ())
    }
}
