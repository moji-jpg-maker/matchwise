//! Durable agent request accounting. The atomic INSERT prevents concurrent
//! callers from reserving the same final slot. Human requests are excluded.
use crate::mm::{autonomy_repo::AutonomyRepo, repository::ORG};
use sqlx::SqlitePool;

pub const MODEL_CALL_LIMIT: &str = "model_call_limit";

pub async fn reserve_model_call(pool: &SqlitePool, run_id: i64, kind: &str) -> Result<(), String> {
    let policy = AutonomyRepo::load_policy(pool)
        .await
        .map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let result = sqlx::query("INSERT INTO mm_agent_model_calls (org_id, run_id, kind, started_at) SELECT ?, ?, ?, ? WHERE (SELECT COUNT(*) FROM mm_agent_model_calls WHERE org_id = ? AND started_at > ?) < ?")
        .bind(ORG).bind(run_id).bind(kind).bind(now).bind(ORG).bind(now - 3600)
        .bind(i64::from(policy.budgets.model_calls_per_hour))
        .execute(pool).await.map_err(|e| e.to_string())?;
    if result.rows_affected() == 0 {
        return Err(MODEL_CALL_LIMIT.into());
    }
    Ok(())
}

/// Introduction admission for the future SendIntroduction tool. Count both
/// sides of each still-open introduction, including ones initiated by a human.
/// The caller must hold runtime_lock through this check and the transition.
/// Messaging tools remain unavailable until step 5.1.
#[allow(dead_code)]
pub async fn introduction_allowed(pool: &SqlitePool, a: &str, b: &str) -> Result<bool, String> {
    let policy = AutonomyRepo::load_policy(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut counts = [0u64; 2];
    for (index, profile) in [a, b].iter().enumerate() {
        counts[index] = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM mm_matches WHERE org_id = ? AND (profile_a = ? OR profile_b = ?) AND status IN ('introduction_proposed', 'both_interested', 'contact_exchanged', 'conversation', 'meeting', 'feedback')")
            .bind(ORG).bind(profile).bind(profile).fetch_one(pool).await.map_err(|e| e.to_string())? as u64;
    }
    Ok(policy.budgets.introduction_allowed(counts[0], counts[1]))
}
