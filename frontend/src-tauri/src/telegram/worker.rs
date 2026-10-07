//! Running the bot: long polling for updates, delivering the outbox, and sending reminders. Everything here
//! stops when the app closes: with long polling the bot only answers while Matchwise is running.

use super::api::{perform, BotInfo, TelegramApi, TgError};
use super::engine;
use super::notify;
use super::repo;
use super::types::{Reply, Update};
use serde::Serialize;
use sqlx::SqlitePool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub const MAX_ATTEMPTS: i64 = 5;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct FlushStats {
    pub sent: usize,
    pub failed: usize,
    pub retried: usize,
    pub cancelled: usize,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Deliver due outbox messages. A person who is no longer reachable (unlinked, withdrew consent, stopped
/// notifications) never receives anything that was queued earlier: the message is cancelled instead.
pub async fn flush_outbox(pool: &SqlitePool, api: &dyn TelegramApi) -> Result<FlushStats, String> {
    let mut stats = FlushStats::default();
    for row in repo::due(pool, 20).await.map_err(err)? {
        let link = repo::link_by_profile(pool, &row.profile_id).await.map_err(err)?.filter(|l| l.consented && l.notifications_enabled);
        let Some(link) = link else {
            repo::cancel(pool, row.id, "recipient not reachable").await.map_err(err)?;
            stats.cancelled += 1;
            continue;
        };
        match api.send_message(link.chat_id, &row.text, row.keyboard.as_ref()).await {
            Ok(_) => {
                repo::mark_sent(pool, row.id).await.map_err(err)?;
                stats.sent += 1;
            }
            Err(TgError::Unauthorized) => return Err(TgError::Unauthorized.to_string()),
            Err(TgError::Forbidden) => {
                // The person blocked the bot: stop trying and stop queueing for them.
                repo::mark_failed(pool, row.id, "the person blocked the bot", None).await.map_err(err)?;
                repo::set_notifications(pool, &row.profile_id, false).await.map_err(err)?;
                stats.failed += 1;
            }
            Err(TgError::RateLimited { retry_after_secs }) => {
                repo::mark_failed(pool, row.id, "rate limited", Some(retry_after_secs as i64 + 1)).await.map_err(err)?;
                stats.retried += 1;
                break; // the whole bot is being throttled: try again on the next round
            }
            Err(TgError::BadRequest(m)) => {
                repo::mark_failed(pool, row.id, &m, None).await.map_err(err)?;
                stats.failed += 1;
            }
            Err(e @ (TgError::Network(_) | TgError::Other(_))) => {
                if row.attempts + 1 >= MAX_ATTEMPTS {
                    repo::mark_failed(pool, row.id, &e.to_string(), None).await.map_err(err)?;
                    stats.failed += 1;
                } else {
                    let backoff = 30i64 * (1i64 << row.attempts.min(6));
                    repo::mark_failed(pool, row.id, &e.to_string(), Some(backoff)).await.map_err(err)?;
                    stats.retried += 1;
                }
            }
        }
    }
    Ok(stats)
}

/// Update ids are persisted so a restart does not process the same update twice.
async fn process_update(pool: &SqlitePool, api: &dyn TelegramApi, update: Update) {
    let chat = update.message.as_ref().map(|m| m.chat.id).or_else(|| update.callback_query.as_ref().map(|c| c.from.id));
    let replies = match engine::handle_update(pool, update).await {
        Ok(r) => r,
        Err(e) => {
            log::error!("Telegram update failed: {e}");
            chat.map(|c| vec![Reply::text(c, "Sorry, something went wrong on my side. Please try again in a moment.")]).unwrap_or_default()
        }
    };
    for r in &replies {
        if let Err(e) = perform(api, r).await {
            // cosmetic failures (a toast, tidying buttons) are not worth a warning
            if matches!(r, Reply::Send { .. }) {
                log::warn!("could not send a reply: {e}");
            }
        }
    }
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct RuntimeStatus {
    pub running: bool,
    pub bot_username: Option<String>,
    pub last_error: Option<String>,
    pub last_activity: Option<String>,
    pub processed_updates: u64,
    pub sent_messages: u64,
}

struct Running {
    cancel: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}

#[derive(Default)]
pub struct TelegramRuntime {
    running: tokio::sync::Mutex<Option<Running>>,
    status: Arc<Mutex<RuntimeStatus>>,
}

impl TelegramRuntime {
    pub fn status(&self) -> RuntimeStatus {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub async fn start(&self, pool: SqlitePool, api: Arc<dyn TelegramApi>) -> Result<BotInfo, String> {
        let mut guard = self.running.lock().await;
        if guard.is_some() {
            return Err("The bot is already running".into());
        }
        let me = api.get_me().await.map_err(|e| e.to_string())?;
        let _ = api.set_commands().await; // cosmetic
        repo::set_meta(&pool, "telegram_bot_username", &me.username).await.map_err(err)?;
        if let Ok(mut s) = self.status.lock() {
            *s = RuntimeStatus { running: true, bot_username: Some(me.username.clone()), ..Default::default() };
        }
        let cancel = CancellationToken::new();
        let poll = tokio::spawn(poll_loop(pool.clone(), api.clone(), cancel.clone(), self.status.clone()));
        let outbox = tokio::spawn(outbox_loop(pool, api, cancel.clone(), self.status.clone()));
        *guard = Some(Running { cancel, tasks: vec![poll, outbox] });
        Ok(me)
    }

    pub async fn stop(&self) {
        let running = self.running.lock().await.take();
        if let Some(r) = running {
            r.cancel.cancel();
            for t in r.tasks {
                let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
            }
        }
        if let Ok(mut s) = self.status.lock() {
            s.running = false;
        }
    }
}

fn touch(status: &Arc<Mutex<RuntimeStatus>>, f: impl FnOnce(&mut RuntimeStatus)) {
    if let Ok(mut s) = status.lock() {
        s.last_activity = Some(repo::now_str());
        f(&mut s);
    }
}

async fn sleep_or_cancel(cancel: &CancellationToken, d: Duration) -> bool {
    tokio::select! { _ = cancel.cancelled() => true, _ = tokio::time::sleep(d) => false }
}

async fn poll_loop(pool: SqlitePool, api: Arc<dyn TelegramApi>, cancel: CancellationToken, status: Arc<Mutex<RuntimeStatus>>) {
    let mut offset = repo::last_update_id(&pool).await.unwrap_or(0) + 1;
    loop {
        let res = tokio::select! {
            _ = cancel.cancelled() => break,
            r = api.get_updates(offset, 25) => r,
        };
        match res {
            Ok(updates) => {
                touch(&status, |s| s.last_error = None);
                for u in updates {
                    offset = offset.max(u.update_id + 1);
                    let id = u.update_id;
                    process_update(&pool, api.as_ref(), u).await;
                    let _ = repo::set_last_update_id(&pool, id).await;
                    touch(&status, |s| s.processed_updates += 1);
                }
            }
            Err(TgError::Unauthorized) => {
                touch(&status, |s| {
                    s.last_error = Some("Telegram rejected the bot token. Check it in the Telegram settings.".into());
                    s.running = false;
                });
                break;
            }
            Err(TgError::RateLimited { retry_after_secs }) => {
                if sleep_or_cancel(&cancel, Duration::from_secs(retry_after_secs + 1)).await {
                    break;
                }
            }
            Err(e) => {
                touch(&status, |s| s.last_error = Some(e.to_string()));
                if sleep_or_cancel(&cancel, Duration::from_secs(5)).await {
                    break;
                }
            }
        }
    }
}

async fn outbox_loop(pool: SqlitePool, api: Arc<dyn TelegramApi>, cancel: CancellationToken, status: Arc<Mutex<RuntimeStatus>>) {
    let mut ticks: u64 = 0;
    loop {
        match flush_outbox(&pool, api.as_ref()).await {
            Ok(st) => {
                if st.sent > 0 {
                    touch(&status, |s| s.sent_messages += st.sent as u64);
                }
            }
            Err(e) => touch(&status, |s| s.last_error = Some(e)),
        }
        if ticks % 300 == 0 {
            // every ~10 minutes (2s ticks): reminders; every ~hour: tidy old delivered messages
            if let Err(e) = notify::queue_reminders(&pool).await {
                log::warn!("reminders failed: {e}");
            }
        }
        if ticks % 1800 == 0 {
            let _ = repo::purge_outbox(&pool, 30).await;
        }
        ticks += 1;
        if sleep_or_cancel(&cancel, Duration::from_secs(2)).await {
            break;
        }
    }
}
