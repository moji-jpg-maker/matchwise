//! Tauri commands for the matchmaker's side of Telegram: the bot token and runtime, invitations, the
//! conversation with a candidate, introduction previews, and the inbox. The token never leaves the Rust side.

use super::api::{HttpTelegramApi, TelegramApi};
use super::notify;
use super::render;
use super::repo;
use super::worker::TelegramRuntime;
use crate::mm::match_repo::MatchRepo;
use crate::mm::repository::MmRepository;
use crate::state::AppState;
use matchmaking_core::NEVER_SHARED;
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::Arc;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

const KEYRING_SERVICE: &str = "com.matchwise.desktop";
const KEYRING_ACCOUNT: &str = "telegram-bot-token";
const TOKEN_ENV: &str = "MATCHWISE_TELEGRAM_TOKEN";

/// `123456789:AA...` as issued by BotFather.
pub fn valid_token_format(t: &str) -> bool {
    match t.split_once(':') {
        Some((id, secret)) => {
            !id.is_empty() && id.len() <= 20 && id.chars().all(|c| c.is_ascii_digit()) && secret.len() >= 30 && secret.len() <= 80
                && secret.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        }
        None => false,
    }
}

/// The token comes from the environment (for WSL or headless use) or the operating system's credential store.
fn load_token() -> Result<Option<(String, &'static str)>, String> {
    if let Ok(t) = std::env::var(TOKEN_ENV) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Ok(Some((t, "environment")));
        }
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| format!("OS credential store unavailable: {e}"))?;
    match entry.get_password() {
        Ok(t) => Ok(Some((t, "credential store"))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("OS credential store unavailable: {e}")),
    }
}

#[derive(Serialize)]
pub struct TelegramStatus {
    pub configured: bool,
    pub token_source: Option<String>,
    pub running: bool,
    pub bot_username: Option<String>,
    pub last_error: Option<String>,
    pub last_activity: Option<String>,
    pub processed_updates: u64,
    pub sent_messages: u64,
    pub consent_version: i64,
    pub introduction_fields: Vec<String>,
    pub show_first_name: bool,
    pub never_shared: Vec<String>,
    pub unread_messages: i64,
    pub open_requests: usize,
}

async fn status(pool: &SqlitePool, rt: &TelegramRuntime) -> Result<TelegramStatus, String> {
    let (configured, source) = match load_token() {
        Ok(Some((_, s))) => (true, Some(s.to_string())),
        _ => (false, None),
    };
    let s = rt.status();
    Ok(TelegramStatus {
        configured,
        token_source: source,
        running: s.running,
        bot_username: s.bot_username.or(repo::get_meta(pool, "telegram_bot_username").await.map_err(err)?),
        last_error: s.last_error,
        last_activity: s.last_activity,
        processed_updates: s.processed_updates,
        sent_messages: s.sent_messages,
        consent_version: repo::CONSENT_VERSION,
        introduction_fields: repo::introduction_fields(pool).await.map_err(err)?,
        show_first_name: repo::show_first_name(pool).await.map_err(err)?,
        never_shared: NEVER_SHARED.iter().map(|s| s.to_string()).collect(),
        unread_messages: repo::unread_counts(pool).await.map_err(err)?.values().sum(),
        open_requests: repo::open_requests(pool).await.map_err(err)?.len(),
    })
}

#[tauri::command]
pub async fn tg_get_status(state: State<'_, AppState>, rt: State<'_, TelegramRuntime>) -> Result<TelegramStatus, String> {
    status(state.db_manager.pool(), &rt).await
}

/// Check a token with Telegram, then store it in the credential store. Does not start the bot.
#[tauri::command]
pub async fn tg_save_token(state: State<'_, AppState>, rt: State<'_, TelegramRuntime>, token: String) -> Result<TelegramStatus, String> {
    let token = token.trim().to_string();
    if !valid_token_format(&token) {
        return Err("That does not look like a bot token. It looks like 123456789:ABC... and comes from BotFather.".into());
    }
    let me = HttpTelegramApi::new(&token).get_me().await.map_err(|e| format!("Telegram did not accept the token: {e}"))?;
    rt.stop().await;
    if std::env::var(TOKEN_ENV).map_or(false, |v| !v.trim().is_empty()) {
        return Err(format!("{TOKEN_ENV} is set and takes precedence; unset it to store a token in the credential store."));
    }
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .and_then(|e| e.set_password(&token))
        .map_err(|e| format!("Could not store the token in the OS credential store: {e}. On WSL without a Secret Service, set {TOKEN_ENV} instead."))?;
    let pool = state.db_manager.pool();
    repo::set_meta(pool, "telegram_bot_username", &me.username).await.map_err(err)?;
    MmRepository::audit(pool, "telegram_token_saved", "settings", "telegram", None).await;
    status(pool, &rt).await
}

#[tauri::command]
pub async fn tg_clear_token(state: State<'_, AppState>, rt: State<'_, TelegramRuntime>) -> Result<TelegramStatus, String> {
    rt.stop().await;
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT) {
        let _ = e.delete_credential();
    }
    let pool = state.db_manager.pool();
    repo::delete_meta(pool, "telegram_bot_username").await.map_err(err)?;
    MmRepository::audit(pool, "telegram_token_removed", "settings", "telegram", None).await;
    status(pool, &rt).await
}

#[tauri::command]
pub async fn tg_start(state: State<'_, AppState>, rt: State<'_, TelegramRuntime>) -> Result<TelegramStatus, String> {
    let (token, _) = load_token()?.ok_or("No bot token is stored yet")?;
    let pool = state.db_manager.pool();
    let api: Arc<dyn TelegramApi> = Arc::new(HttpTelegramApi::new(&token));
    rt.start(pool.clone(), api).await?;
    MmRepository::audit(pool, "telegram_started", "settings", "telegram", None).await;
    status(pool, &rt).await
}

#[tauri::command]
pub async fn tg_stop(state: State<'_, AppState>, rt: State<'_, TelegramRuntime>) -> Result<TelegramStatus, String> {
    rt.stop().await;
    status(state.db_manager.pool(), &rt).await
}

/// Choose which facts an introduction may show, and whether it shows first names.
#[tauri::command]
pub async fn tg_set_sharing(state: State<'_, AppState>, rt: State<'_, TelegramRuntime>, fields: Vec<String>, show_first_name: bool) -> Result<TelegramStatus, String> {
    let pool = state.db_manager.pool();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    if fields.len() > 30 {
        return Err("Too many fields".into());
    }
    for f in &fields {
        if NEVER_SHARED.contains(&f.as_str()) {
            return Err(format!("'{f}' can never be shared in an introduction"));
        }
        let def = reg.get(f).ok_or_else(|| format!("unknown field '{f}'"))?;
        if matches!(def.kind, matchmaking_core::FieldKind::Records(_)) {
            return Err(format!("'{}' cannot be shared", def.label));
        }
    }
    repo::set_sharing(pool, &fields, show_first_name).await.map_err(err)?;
    MmRepository::audit(pool, "telegram_sharing_changed", "settings", "telegram", Some(&format!("{} fields", fields.len()))).await;
    status(pool, &rt).await
}

#[derive(Serialize)]
pub struct LinkView {
    pub linked: bool,
    pub consented: bool,
    pub notifications_enabled: bool,
    pub username: Option<String>,
    pub linked_at: Option<String>,
    pub open_invite: bool,
    pub unread: i64,
}

#[tauri::command]
pub async fn tg_get_link(state: State<'_, AppState>, profile_id: String) -> Result<LinkView, String> {
    let pool = state.db_manager.pool();
    let link = repo::link_by_profile(pool, &profile_id).await.map_err(err)?;
    Ok(LinkView {
        linked: link.is_some(),
        consented: link.as_ref().map_or(false, |l| l.consented),
        notifications_enabled: link.as_ref().map_or(false, |l| l.notifications_enabled),
        username: link.as_ref().and_then(|l| l.username.clone()),
        linked_at: link.as_ref().map(|l| l.linked_at.clone()),
        open_invite: repo::has_open_invite(pool, &profile_id).await.map_err(err)?,
        unread: repo::unread_counts(pool).await.map_err(err)?.get(&profile_id).copied().unwrap_or(0),
    })
}

#[derive(Serialize)]
pub struct InviteView {
    /// Shown once. Only a hash is kept, so it cannot be shown again.
    pub code: String,
    pub link: Option<String>,
    pub expires_at: String,
}

#[tauri::command]
pub async fn tg_create_invite(state: State<'_, AppState>, profile_id: String) -> Result<InviteView, String> {
    let pool = state.db_manager.pool();
    let p = MmRepository::get_profile(pool, &profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    if p.status != "active" {
        return Err("Reactivate the profile first".into());
    }
    if repo::link_by_profile(pool, &profile_id).await.map_err(err)?.is_some() {
        return Err("This profile is already linked to a Telegram account. Unlink it first to invite a different one.".into());
    }
    let invite = repo::create_invite(pool, &profile_id).await.map_err(err)?;
    let bot = repo::get_meta(pool, "telegram_bot_username").await.map_err(err)?;
    MmRepository::audit(pool, "telegram_invite_created", "profile", &profile_id, None).await;
    Ok(InviteView { link: bot.map(|b| format!("https://t.me/{b}?start={}", invite.code)), code: invite.code, expires_at: invite.expires_at })
}

#[tauri::command]
pub async fn tg_unlink(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    let pool = state.db_manager.pool();
    repo::delete_link(pool, &profile_id).await.map_err(err)?;
    MmRepository::audit(pool, "telegram_unlink", "profile", &profile_id, Some("by matchmaker")).await;
    Ok(())
}

#[derive(Serialize)]
pub struct MessageView {
    pub id: i64,
    pub direction: String,
    pub text: String,
    pub match_id: Option<String>,
    pub created_at: String,
    pub is_read: bool,
}

/// The conversation with a candidate, oldest first.
#[tauri::command]
pub async fn tg_messages(state: State<'_, AppState>, profile_id: String) -> Result<Vec<MessageView>, String> {
    let mut rows = repo::messages(state.db_manager.pool(), &profile_id, 200).await.map_err(err)?;
    rows.reverse();
    Ok(rows.into_iter().map(|m| MessageView { id: m.id, direction: m.direction, text: m.text, match_id: m.match_id, created_at: m.created_at, is_read: m.is_read }).collect())
}

#[tauri::command]
pub async fn tg_mark_read(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    repo::mark_read(state.db_manager.pool(), &profile_id).await.map_err(err)
}

/// Write to a candidate. Refused unless they are linked, agreed to the notice and have notifications on.
#[tauri::command]
pub async fn tg_send_message(state: State<'_, AppState>, profile_id: String, text: String) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let text = text.trim().to_string();
    if text.is_empty() || text.chars().count() > 2000 {
        return Err("A message needs 1 to 2000 characters".into());
    }
    if notify::reachable_link(pool, &profile_id).await?.is_none() {
        return Err("This person cannot be reached on Telegram (not linked, has not agreed to the notice, or has turned notifications off)".into());
    }
    repo::enqueue(pool, &profile_id, "matchmaker_message", &render::matchmaker_message_text(&text), None, None).await.map_err(err)?;
    repo::add_message(pool, &profile_id, "out", &text, None).await.map_err(err)?;
    MmRepository::audit(pool, "telegram_message_sent", "profile", &profile_id, None).await;
    Ok(())
}

#[derive(Serialize)]
pub struct InfoRequestResult {
    /// False when the person is not reachable on Telegram: the request is still on record, ask them another way.
    pub sent: bool,
}

/// Ask one person of a match for more information: puts the match on hold with the request text and, if the person
/// is reachable, sends it to them. Their next free-text reply is filed against the match.
#[tauri::command]
pub async fn tg_request_info(state: State<'_, AppState>, match_id: String, side: String, text: String) -> Result<InfoRequestResult, String> {
    let pool = state.db_manager.pool();
    let m = MatchRepo::get(pool, &match_id).await.map_err(err)?.ok_or("Match not found")?;
    let text = text.trim().to_string();
    if text.is_empty() || text.chars().count() > 1000 {
        return Err("The request needs 1 to 1000 characters".into());
    }
    let who = match side.as_str() {
        "a" => &m.profile_a,
        "b" => &m.profile_b,
        _ => return Err("side must be 'a' or 'b'".into()),
    };
    MatchRepo::set_hold(pool, &match_id, Some(&text)).await.map_err(err)?;
    let mut sent = false;
    if let Some(link) = notify::reachable_link(pool, who).await? {
        repo::enqueue(pool, who, "info_request", &render::info_request_text(&text), None, Some(&match_id)).await.map_err(err)?;
        repo::add_message(pool, who, "out", &text, Some(&match_id)).await.map_err(err)?;
        repo::set_state(pool, link.chat_id, &serde_json::json!({"flow": "reply", "match_id": match_id})).await.map_err(err)?;
        sent = true;
    }
    MmRepository::audit(pool, "telegram_info_requested", "match", &match_id, Some(if sent { "sent" } else { "not reachable" })).await;
    Ok(InfoRequestResult { sent })
}

/// Send the "your profile is nearly ready" reminder now.
#[tauri::command]
pub async fn tg_remind_profile(state: State<'_, AppState>, profile_id: String) -> Result<(), String> {
    let pool = state.db_manager.pool();
    if notify::reachable_link(pool, &profile_id).await?.is_none() {
        return Err("This person cannot be reached on Telegram".into());
    }
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let p = MmRepository::get_profile(pool, &profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    let labels: Vec<String> = p.profile.missing_required(&reg).iter().filter_map(|k| reg.get(k).map(|d| d.label.clone())).collect();
    if labels.is_empty() {
        return Err("Nothing is missing from this profile".into());
    }
    repo::enqueue(pool, &profile_id, "profile_reminder", &render::reminder_profile_text(&labels), None, None).await.map_err(err)?;
    Ok(())
}

#[derive(Serialize)]
pub struct IntroPreviewSide {
    pub profile_id: String,
    pub name: Option<String>,
    pub reachable: bool,
    /// What this person would receive (plain text); None if they cannot be reached.
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct IntroPreview {
    pub a: IntroPreviewSide,
    pub b: IntroPreviewSide,
}

/// Exactly what proposing the introduction would send, before the matchmaker commits to it.
#[tauri::command]
pub async fn tg_preview_introduction(state: State<'_, AppState>, match_id: String) -> Result<IntroPreview, String> {
    let pool = state.db_manager.pool();
    let m = MatchRepo::get(pool, &match_id).await.map_err(err)?.ok_or("Match not found")?;
    async fn side(pool: &SqlitePool, m: &crate::mm::match_repo::MatchRow, me: &str, other: &str) -> Result<IntroPreviewSide, String> {
        let p = MmRepository::get_profile(pool, me).await.map_err(err)?.ok_or("Profile not found")?;
        let name = match p.profile.get("full_name") {
            Some(matchmaking_core::Value::Text(t)) if !t.trim().is_empty() => Some(t.clone()),
            _ => None,
        };
        let reachable = notify::reachable_link(pool, me).await?.is_some();
        let message = if reachable { Some(render::plain(&notify::introduction_message(pool, m, other).await?.0)) } else { None };
        Ok(IntroPreviewSide { profile_id: me.to_string(), name, reachable, message })
    }
    Ok(IntroPreview { a: side(pool, &m, &m.profile_a, &m.profile_b).await?, b: side(pool, &m, &m.profile_b, &m.profile_a).await? })
}

#[derive(Serialize)]
pub struct OutboxView {
    pub kind: String,
    pub text: String,
    pub status: String,
    pub attempts: i64,
    pub created_at: String,
    pub sent_at: Option<String>,
    pub last_error: Option<String>,
}

/// What was sent to a candidate through the bot (the matchmaker's record of it).
#[tauri::command]
pub async fn tg_outbox(state: State<'_, AppState>, profile_id: String) -> Result<Vec<OutboxView>, String> {
    let rows = repo::outbox_for_profile(state.db_manager.pool(), &profile_id, 50).await.map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|r| OutboxView { kind: r.kind, text: render::plain(&r.text), status: r.status, attempts: r.attempts, created_at: r.created_at, sent_at: r.sent_at, last_error: r.last_error })
        .collect())
}

#[derive(Serialize)]
pub struct InboxItem {
    pub profile_id: String,
    pub name: Option<String>,
    pub unread: i64,
}

#[derive(Serialize)]
pub struct RequestItem {
    pub id: i64,
    pub profile_id: String,
    pub name: Option<String>,
    pub kind: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct Inbox {
    pub unread: Vec<InboxItem>,
    pub requests: Vec<RequestItem>,
}

async fn name_of(pool: &SqlitePool, id: &str) -> Option<String> {
    match MmRepository::get_profile(pool, id).await.ok().flatten()?.profile.get("full_name") {
        Some(matchmaking_core::Value::Text(t)) if !t.trim().is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// Unread messages and open data requests across all candidates.
#[tauri::command]
pub async fn tg_inbox(state: State<'_, AppState>) -> Result<Inbox, String> {
    let pool = state.db_manager.pool();
    let mut unread = vec![];
    for (profile_id, n) in repo::unread_counts(pool).await.map_err(err)? {
        unread.push(InboxItem { name: name_of(pool, &profile_id).await, profile_id, unread: n });
    }
    let mut requests = vec![];
    for (id, profile_id, kind, created_at) in repo::open_requests(pool).await.map_err(err)? {
        requests.push(RequestItem { id, name: name_of(pool, &profile_id).await, profile_id, kind, created_at });
    }
    Ok(Inbox { unread, requests })
}

/// Mark a data request as handled (after the matchmaker has actually done it, for example by deleting the profile).
#[tauri::command]
pub async fn tg_resolve_request(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    repo::resolve_request(state.db_manager.pool(), id).await.map_err(err)?;
    MmRepository::audit(state.db_manager.pool(), "data_request_resolved", "settings", &id.to_string(), None).await;
    Ok(())
}

#[cfg(test)]
mod command_tests {
    use super::valid_token_format;

    #[test]
    fn token_format() {
        assert!(valid_token_format("123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw"));
        for bad in ["", "abc", "123:short", "abc:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw", "123456789:AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw!", ":AAHdqTcvCH1vGWJxfSeofSAs0K5PALDsaw", "123456789:AAHdq TcvCH1vGWJxfSeofSAs0K5PALDsaw"] {
            assert!(!valid_token_format(bad), "{bad}");
        }
    }
}
