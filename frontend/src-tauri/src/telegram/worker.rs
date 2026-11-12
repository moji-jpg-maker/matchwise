//! Running the bot: long polling for updates, delivering the outbox, and sending reminders. Everything here
//! stops when the app closes: with long polling the bot only answers while Matchwise is running.

use super::api::{perform, BotInfo, TelegramApi, TgError};
use super::engine;
use super::notify;
use super::repo;
use super::types::{Reply, Update};
use crate::mm::photo_repo::{self, PhotoRepo, PhotoStore, MAX_PHOTO_BYTES};
use serde::Serialize;
use sqlx::SqlitePool;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub const MAX_ATTEMPTS: i64 = 5;
/// How often the worker tries to fetch one photo before marking it `failed`, so a broken file does not retry forever.
pub const MAX_PHOTO_ATTEMPTS: i64 = 3;
/// Photos fetched per poll round. Keeps a backlog from holding up the next batch of messages.
const PHOTOS_PER_ROUND: i64 = 10;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DownloadStats {
    pub stored: usize,
    pub duplicates: usize,
    pub failed: usize,
    pub deferred: usize,
}

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

/// Fetch the photos Telegram told us about but whose bytes we do not have yet.
///
/// A row only exists here because someone linked and agreed to the notice; the engine wrote it `pending`. Each
/// file is fetched, checked against the accepted formats by its bytes (never by its name or the type Telegram
/// claimed), and written where the profile's photos live. A transient failure is retried on a later round; after
/// [`MAX_PHOTO_ATTEMPTS`] the row is `failed`, which the UI shows instead of a broken image.
///
/// Nothing about a photo is logged: not its id, name, hash or Telegram path.
pub async fn download_pending_photos(pool: &SqlitePool, api: &dyn TelegramApi, store: &PhotoStore) -> Result<DownloadStats, String> {
    let mut stats = DownloadStats::default();
    for photo in PhotoRepo::pending_downloads(pool, PHOTOS_PER_ROUND).await.map_err(err)? {
        let Some(file_id) = photo.telegram_file_id.as_deref() else {
            continue;
        };
        match fetch_photo(pool, api, store, &photo, file_id).await {
            Ok(Fetched::Stored) => stats.stored += 1,
            Ok(Fetched::Duplicate) => stats.duplicates += 1,
            // Telegram is throttling the whole bot: leave the rest of the queue for the next round.
            Err(TgError::RateLimited { .. }) | Err(TgError::Unauthorized) => {
                stats.deferred += 1;
                break;
            }
            Err(e) => {
                let attempt = PhotoRepo::record_download_failure(pool, &photo.profile_id, &photo.id, MAX_PHOTO_ATTEMPTS)
                    .await
                    .map_err(err)?;
                if attempt >= MAX_PHOTO_ATTEMPTS {
                    stats.failed += 1;
                    log::warn!("a Telegram photo could not be downloaded after {attempt} attempts: {}", e);
                } else {
                    stats.deferred += 1;
                    log::debug!("a Telegram photo download will be retried: {}", e);
                }
            }
        }
    }
    Ok(stats)
}

enum Fetched {
    Stored,
    /// The bytes repeat a photo the profile already has; the pending row is gone and no slot is used.
    Duplicate,
}

async fn fetch_photo(pool: &SqlitePool, api: &dyn TelegramApi, store: &PhotoStore, photo: &crate::mm::photo_repo::PhotoRow, file_id: &str) -> Result<Fetched, TgError> {
    let info = api.get_file(file_id).await?;
    if info.file_size.is_some_and(|n| n > MAX_PHOTO_BYTES as i64) {
        // Recorded as a failure rather than fetched and thrown away: the row still has to stop being `pending`.
        return Err(TgError::BadRequest("the file is larger than the 10 MB limit".into()));
    }
    let bytes = api.download_file(&info).await?;
    if bytes.len() as u64 > MAX_PHOTO_BYTES {
        return Err(TgError::BadRequest("the file is larger than the 10 MB limit".into()));
    }
    let Some((mime, _)) = photo_repo::image_kind(&bytes) else {
        return Err(TgError::BadRequest("the bytes are not a JPEG, PNG or WebP image".into()));
    };
    let digest = photo_repo::sha256_hex(&bytes);
    if PhotoRepo::hashes(pool, &photo.profile_id).await.map_err(|e| TgError::Other(e.to_string()))?.contains(&digest) {
        PhotoRepo::delete_pending(pool, &photo.profile_id, &photo.id).await.map_err(|e| TgError::Other(e.to_string()))?;
        return Ok(Fetched::Duplicate);
    }
    let file = store.save(&photo.profile_id, &bytes, mime).map_err(|e| TgError::Other(e.to_string()))?;
    match PhotoRepo::complete_download(pool, &photo.profile_id, &photo.id, &file, mime).await {
        Ok(true) => Ok(Fetched::Stored),
        // Another worker (or a delete) got there first. The row is no longer ours, so the file goes.
        Ok(false) => {
            let _ = store.remove(&photo.profile_id, &file.stored_name);
            Err(TgError::Other("the photo is no longer waiting to download".into()))
        }
        Err(e) => {
            let _ = store.remove(&photo.profile_id, &file.stored_name);
            Err(TgError::Other(e.to_string()))
        }
    }
}

/// Deliver due outbox messages. A person who is no longer reachable (unlinked, withdrew consent, stopped
/// notifications) never receives anything that was queued earlier: the message is cancelled instead.
pub async fn flush_outbox(pool: &SqlitePool, api: &dyn TelegramApi) -> Result<FlushStats, String> {
    let mut stats = FlushStats::default();
    for row in repo::due(pool, 20).await.map_err(err)? {
        if row.actor.starts_with("agent:") {
            let policy = crate::mm::autonomy_repo::AutonomyRepo::load_policy(pool).await.map_err(err)?;
            if policy.paused || crate::mm::autonomy::ai_gate(pool).await? != matchmaking_core::autonomy::AiGate::Open {
                continue;
            }
            let current = match row.match_id.as_deref() {
                Some(id) => crate::mm::match_repo::MatchRepo::get(pool, id).await.map_err(err)?,
                None => None,
            };
            let waiting = if row.kind == "agent_message" {
                if let Some(id) = row.match_id.as_deref() {
                    super::agent_message::delivery_allowed(pool, &row.profile_id, id, &row.text).await?
                } else { false }
            } else if matches!(row.kind.as_str(), "profile_reminder" | "intro_reminder") {
                super::follow_up::still_eligible(pool, &row.profile_id, row.match_id.as_deref()).await?
            } else { current.is_some_and(|m| !m.hidden && m.hold_reason.is_none()
                && m.status == matchmaking_core::MatchStatus::IntroductionProposed
                && ((m.profile_a == row.profile_id && m.a_response == matchmaking_core::Interest::Unknown)
                    || (m.profile_b == row.profile_id && m.b_response == matchmaking_core::Interest::Unknown))) };
            if !waiting {
                repo::cancel(pool, row.id, "candidate no longer awaiting this message").await.map_err(err)?;
                stats.cancelled += 1;
                continue;
            }
        }
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
    /// Identifies the run that wrote this status. Tasks carry the id they were started with so a task that
    /// outlives the 5 s join timeout cannot mark a newer run as stopped.
    #[serde(skip)]
    pub run: u64,
}

struct Running {
    cancel: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
}

const WORKER_STOPPED: &str = "The Telegram worker stopped unexpectedly. Press Start to restart it.";

/// Marks a run as over when its loop exits, however it exits. A panic inside an update handler unwinds through
/// the loop, so `Drop` runs and the status stops claiming the bot is alive. Cancelling the token also stops the
/// sibling loop, which would otherwise keep polling with a broken counterpart.
pub(super) struct RunGuard {
    status: Arc<Mutex<RuntimeStatus>>,
    run: u64,
    cancel: CancellationToken,
}

impl RunGuard {
    pub(super) fn new(status: Arc<Mutex<RuntimeStatus>>, run: u64, cancel: CancellationToken) -> Self {
        Self { status, run, cancel }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.cancel.cancel();
        // The token above may now have unblocked a newer start; only this run's own status is ours to touch.
        let Ok(mut s) = self.status.lock() else { return };
        if s.run != self.run {
            return;
        }
        s.running = false;
        if std::thread::panicking() && s.last_error.is_none() {
            s.last_error = Some(WORKER_STOPPED.into());
        }
    }
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

    pub async fn start(&self, pool: SqlitePool, api: Arc<dyn TelegramApi>, photos_dir: PathBuf) -> Result<BotInfo, String> {
        let mut guard = self.running.lock().await;
        if let Some(running) = guard.as_ref() {
            // A revoked token ends the poll loop on its own, which leaves a dead handle behind while the
            // status already says "stopped". A task that died, by panic or otherwise, is just as dead. Reap
            // that handle so the operator can simply press Start again; a bot that is genuinely running is
            // still refused.
            let stopped = self.status.lock().map(|s| !s.running).unwrap_or(false);
            let dead = stopped || running.tasks.iter().any(|t| t.is_finished());
            if !dead {
                return Err("The bot is already running".into());
            }
            if let Some(r) = guard.take() {
                r.cancel.cancel();
                for t in r.tasks {
                    let _ = tokio::time::timeout(Duration::from_secs(5), t).await;
                }
            }
        }
        let me = api.get_me().await.map_err(|e| e.to_string())?;
        let _ = api.set_commands().await; // cosmetic
        repo::set_meta(&pool, "telegram_bot_username", &me.username).await.map_err(err)?;
        let run = self.status.lock().map(|s| s.run).unwrap_or(0) + 1;
        if let Ok(mut s) = self.status.lock() {
            *s = RuntimeStatus { running: true, bot_username: Some(me.username.clone()), run, ..Default::default() };
        }
        let cancel = CancellationToken::new();
        let store = PhotoStore::new(photos_dir);
        let poll = tokio::spawn(poll_loop(pool.clone(), api.clone(), store, cancel.clone(), self.status.clone(), run));
        let outbox = tokio::spawn(outbox_loop(pool, api, cancel.clone(), self.status.clone(), run));
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

async fn poll_loop(pool: SqlitePool, api: Arc<dyn TelegramApi>, store: PhotoStore, cancel: CancellationToken, status: Arc<Mutex<RuntimeStatus>>, run: u64) {
    let _guard = RunGuard::new(status.clone(), run, cancel.clone());
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
                // After each batch (an empty one included, which is what a quiet long poll returns) fetch the photos
                // those updates queued, and any that an earlier round could not get.
                if let Err(e) = download_pending_photos(&pool, api.as_ref(), &store).await {
                    log::warn!("photo download round failed: {e}");
                }
            }
            Err(TgError::Unauthorized) => {
                touch(&status, |s| {
                    s.last_error = Some("Telegram rejected the bot token. Check it in the Telegram settings.".into());
                    s.running = false;
                });
                // Nothing can work with a revoked token: stop the outbox loop too, and leave the handle
                // for start() to reap (see above).
                cancel.cancel();
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

async fn outbox_loop(pool: SqlitePool, api: Arc<dyn TelegramApi>, cancel: CancellationToken, status: Arc<Mutex<RuntimeStatus>>, run: u64) {
    let _guard = RunGuard::new(status.clone(), run, cancel.clone());
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
