//! Desktop-only background agent. Work is ordered by its durable queue id;
//! concurrent events coalesce per profile. Every subject uses the existing
//! runtime, policy checks, redaction, tools and run audit.
use super::{ensure_open, run_subject_cancellable, runtime_lock};
use crate::ai::{commands::prepare_for, service::Session};
use crate::mm::autonomy_repo::AutonomyRepo;
use matchmaking_core::{
    autonomy::{Step, StepMode},
    Locale,
};
use sqlx::{Row, SqlitePool};
use std::time::Duration;
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;

pub(crate) const TICK: Duration = Duration::from_secs(1);
const SCAN_INTERVAL: Duration = Duration::from_secs(15 * 60);

struct Running {
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

#[derive(Default)]
pub struct AgentRuntime {
    running: Mutex<Option<Running>>,
}

impl AgentRuntime {
    pub async fn start(&self, pool: SqlitePool) {
        let mut running = self.running.lock().await;
        if running
            .as_ref()
            .is_some_and(|worker| !worker.task.is_finished())
        {
            return;
        }
        if let Some(previous) = running.take() {
            let _ = previous.task.await;
        }
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let task = tokio::spawn(worker(
            pool,
            token,
            TICK,
            SCAN_INTERVAL,
            |pool, token| async move { tick_once(&pool, &token).await },
        ));
        *running = Some(Running { cancel, task });
    }

    pub async fn stop(&self) {
        let mut running = self.running.lock().await;
        if let Some(worker) = running.take() {
            worker.cancel.cancel();
            // Let an already executing validated tool finish and retain its audit.
            let _ = worker.task.await;
        }
    }
}

/// The same loop is exercised with a scripted backend and short clock periods
/// in tests; production supplies tick_once and the fixed 15-minute interval.
pub(super) async fn worker<F, Fut>(
    pool: SqlitePool,
    token: CancellationToken,
    tick_every: Duration,
    scan_every: Duration,
    execute: F,
) where
    F: Fn(SqlitePool, CancellationToken) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut tick = tokio::time::interval(tick_every);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut next_scan = tokio::time::Instant::now();
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => break,
            _ = tick.tick() => {}
        }
        // Paused/L0 installs neither scan nor construct a model backend.
        if !enabled(&pool).await.unwrap_or(false) {
            continue;
        }
        if tokio::time::Instant::now() >= next_scan {
            match scan(&pool).await {
                Ok(()) => next_scan = tokio::time::Instant::now() + scan_every,
                Err(_) => {
                    log::warn!("Agent background work scan failed");
                    continue;
                }
            }
        }
        if execute(pool.clone(), token.clone()).await.is_err() {
            // No target text, arguments or provider errors in logs.
            log::warn!("Agent background tick failed");
        }
    }
}

async fn enabled(pool: &SqlitePool) -> Result<bool, String> {
    let policy = AutonomyRepo::load_policy(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(!policy.paused
        && Step::ALL.into_iter().any(|step| {
            matches!(
                policy.mode_of(step),
                StepMode::Auto | StepMode::Suggest | StepMode::ReviewWindow { .. }
            )
        }))
}

/// Called only after the whole intake batch has completed, including extraction
/// and deterministic acceptance. Queueing never starts a second concurrent run.
pub(crate) async fn enqueue_batch(pool: &SqlitePool, batch_id: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO mm_agent_work(org_id, profile_id, batch_id)
         SELECT n.org_id, n.profile_id, n.batch_id FROM mm_intake_notes n
         JOIN mm_profiles p ON p.id=n.profile_id AND p.org_id=n.org_id
         WHERE n.org_id='default' AND n.batch_id=? AND p.status='active'
         ORDER BY n.profile_id
         ON CONFLICT(org_id, profile_id) DO UPDATE
         SET batch_id=excluded.batch_id, generation=mm_agent_work.generation+1",
    )
    .bind(batch_id)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Periodic scan, also performed at startup. Existing queued work keeps
/// its place; active subjects are added in stable profile-id order.
pub(crate) async fn scan(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO mm_agent_work(org_id, profile_id)
         SELECT org_id, id FROM mm_profiles WHERE org_id='default' AND status='active'
         ORDER BY id ON CONFLICT(org_id, profile_id) DO NOTHING",
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub(super) async fn tick_once(pool: &SqlitePool, cancel: &CancellationToken) -> Result<(), String> {
    let Ok(_guard) = runtime_lock().try_lock() else {
        return Ok(());
    };
    if !enabled(pool).await? || cancel.is_cancelled() {
        return Ok(());
    }
    let has_work: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM mm_agent_work w
         JOIN mm_profiles p ON p.id=w.profile_id AND p.org_id=w.org_id
         WHERE w.org_id='default' AND p.status='active' AND NOT EXISTS (
           SELECT 1 FROM mm_agent_runs r JOIN mm_agent_steps s ON s.run_id=r.id
           WHERE r.org_id=w.org_id AND r.profile_id=w.profile_id AND s.status IN ('proposed', 'scheduled')))",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    let has_due: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mm_agent_steps s JOIN mm_agent_runs r ON r.id=s.run_id WHERE r.org_id='default' AND s.status='scheduled' AND s.due_at<=?)")
        .bind(chrono::Utc::now().timestamp()).fetch_one(pool).await.map_err(|e| e.to_string())?;
    if !has_work && !has_due {
        return Ok(());
    }
    // Gate validation precedes construction and any model call. Closed gates
    // leave work pending for a later tick after the matchmaker enables AI.
    let locale = Locale::parse_or_default("");
    let Ok((config, mode, backend)) = prepare_for(pool, locale).await else {
        return Ok(());
    };
    let session = Session {
        backend: backend.as_ref(),
        provider: config.provider.clone(),
        model: config.model.clone(),
        mode,
        locale,
        agent_config: Some(config),
    };
    process_next(pool, &session, cancel).await
}

/// Caller holds runtime_lock, shared with manual runs and proposal decisions.
pub(crate) async fn process_next(
    pool: &SqlitePool,
    session: &Session<'_>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    if !enabled(pool).await? || cancel.is_cancelled() || ensure_open(pool, session).await.is_err() {
        return Ok(());
    }
    // Due receipts take priority and consume no new model turns.
    if super::process_due(pool, session, cancel).await? {
        return Ok(());
    }
    // Inactive targets cannot block the queue. Pending proposals defer that
    // subject until a person decides them, preventing repeated proposal copies.
    sqlx::query(
        "DELETE FROM mm_agent_work WHERE org_id='default' AND profile_id IN
                 (SELECT id FROM mm_profiles WHERE status<>'active')",
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    let work = sqlx::query(
        "SELECT w.id, w.profile_id, w.batch_id, w.generation FROM mm_agent_work w
         WHERE w.org_id='default' AND NOT EXISTS (
           SELECT 1 FROM mm_agent_runs r JOIN mm_agent_steps s ON s.run_id=r.id
           WHERE r.org_id=w.org_id AND r.profile_id=w.profile_id AND s.status IN ('proposed', 'scheduled'))
         ORDER BY w.id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some(work) = work else { return Ok(()) };
    let target: String = work.get("profile_id");
    let batch: Option<String> = work.get("batch_id");
    run_subject_cancellable(pool, session, &target, batch.as_deref(), cancel).await?;
    // Pausing or stopping preserves work; it can be retried after resume.
    if !cancel.is_cancelled() && ensure_open(pool, session).await.is_ok() {
        sqlx::query("DELETE FROM mm_agent_work WHERE id=? AND generation=?")
            .bind(work.get::<i64, _>("id"))
            .bind(work.get::<i64, _>("generation"))
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
