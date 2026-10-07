//! The small part of the Telegram Bot API this app uses.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub callback_query: Option<CallbackQuery>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub message_id: i64,
    #[serde(default)]
    pub from: Option<User>,
    pub chat: Chat,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CallbackQuery {
    pub id: String,
    pub from: User,
    #[serde(default)]
    pub message: Option<Message>,
    #[serde(default)]
    pub data: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineButton {
    pub text: String,
    pub callback_data: String,
}

pub type Keyboard = Vec<Vec<InlineButton>>;

pub fn button(text: impl Into<String>, data: impl Into<String>) -> InlineButton {
    InlineButton { text: text.into(), callback_data: data.into() }
}

/// What the bot wants to do in response to an update. The caller performs these against the real API (or a test double).
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Send { chat_id: i64, text: String, keyboard: Option<Keyboard> },
    /// Stop the spinner on a tapped button, optionally with a short toast.
    AnswerCallback { id: String, text: Option<String> },
    /// Replace (Some) or remove (None) the buttons of an earlier message.
    EditMarkup { chat_id: i64, message_id: i64, keyboard: Option<Keyboard> },
}

impl Reply {
    pub fn text(chat_id: i64, text: impl Into<String>) -> Reply {
        Reply::Send { chat_id, text: text.into(), keyboard: None }
    }
    pub fn with_keyboard(chat_id: i64, text: impl Into<String>, keyboard: Keyboard) -> Reply {
        Reply::Send { chat_id, text: text.into(), keyboard: Some(keyboard) }
    }
}
