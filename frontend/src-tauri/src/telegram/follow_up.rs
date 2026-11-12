//! Policy-checked template reminders. No model-written outbound text is accepted.
use super::{notify, render, repo, types::Keyboard};
use crate::mm::{
    autonomy_repo::AutonomyRepo,
    match_repo::MatchRepo,
    repository::{MmRepository, StoredProfile, ORG},
};
use matchmaking_core::{autonomy::Actor, first_name_for, Interest, MatchStatus};
use serde_json::{json, Value};
use sqlx::{SqliteConnection, SqlitePool};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

struct Prepared {
    profiles: Vec<StoredProfile>,
    kind: &'static str,
    text: String,
    keyboard: Option<Keyboard>,
    linked_at: String,
}

async fn prepare(
    pool: &SqlitePool,
    target: &str,
    recipient: &str,
    match_id: Option<&str>,
) -> Result<Option<Prepared>, String> {
    let Some(profile) = MmRepository::get_profile(pool, recipient)
        .await
        .map_err(err)?
    else {
        return Ok(None);
    };
    if profile.status != "active" {
        return Ok(None);
    }
    let Some(link) = notify::reachable_link(pool, recipient).await? else {
        return Ok(None);
    };
    let mut profiles = vec![profile];
    let (kind, text, keyboard) = if let Some(id) = match_id {
        let Some(m) = MatchRepo::get(pool, id).await.map_err(err)? else {
            return Ok(None);
        };
        if (m.profile_a != target && m.profile_b != target)
            || m.hidden
            || m.hold_reason.is_some()
            || m.status != MatchStatus::IntroductionProposed
        {
            return Ok(None);
        }
        let other = if m.profile_a == recipient && m.a_response == Interest::Unknown {
            &m.profile_b
        } else if m.profile_b == recipient && m.b_response == Interest::Unknown {
            &m.profile_a
        } else {
            return Ok(None);
        };
        let Some(other) = MmRepository::get_profile(pool, other).await.map_err(err)? else {
            return Ok(None);
        };
        if other.status != "active" {
            return Ok(None);
        }
        let name = first_name_for(
            &other.profile,
            repo::show_first_name(pool).await.map_err(err)?,
            link.locale,
        );
        profiles.push(other);
        (
            "intro_reminder",
            render::reminder_intro_text(link.locale, &name),
            Some(notify::intro_keyboard(link.locale, id)),
        )
    } else {
        if recipient != target {
            return Ok(None);
        }
        let registry = MmRepository::registry(pool).await.map_err(err)?;
        let missing = profiles[0].profile.missing_required(&registry);
        if missing.is_empty() {
            return Ok(None);
        }
        let labels = missing
            .iter()
            .filter_map(|k| registry.get(k).map(|d| d.label_for(link.locale).to_owned()))
            .collect::<Vec<_>>();
        (
            "profile_reminder",
            render::reminder_profile_text(link.locale, &labels),
            None,
        )
    };
    Ok(Some(Prepared {
        profiles,
        kind,
        text,
        keyboard,
        linked_at: link.linked_at,
    }))
}

async fn due(
    connection: &mut SqliteConnection,
    recipient: &str,
    match_id: Option<&str>,
    prepared: &Prepared,
) -> Result<bool, String> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mm_reminder_history WHERE profile_id=? AND match_id IS ? AND kind=?",
    )
    .bind(recipient)
    .bind(match_id)
    .bind(prepared.kind)
    .fetch_one(&mut *connection)
    .await
    .map_err(err)?;
    let cap = if match_id.is_some() {
        notify::MAX_INTRO_REMINDERS
    } else {
        notify::MAX_PROFILE_REMINDERS
    };
    if count >= cap {
        return Ok(false);
    }
    let last: Option<String> = sqlx::query_scalar("SELECT MAX(created_at) FROM mm_reminder_history WHERE profile_id=? AND match_id IS ? AND (kind=? OR kind='introduction' OR kind='interest_buttons')")
        .bind(recipient).bind(match_id).bind(prepared.kind).fetch_one(&mut *connection).await.map_err(err)?;
    // An introduction reminder requires an actual introduction receipt.
    if match_id.is_some() {
        let introduced: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mm_reminder_history WHERE profile_id=? AND match_id=? AND kind='introduction'")
            .bind(recipient).bind(match_id).fetch_one(&mut *connection).await.map_err(err)?;
        if introduced == 0 {
            return Ok(false);
        }
    }
    let timestamp = last.as_deref().unwrap_or(&prepared.linked_at);
    Ok(chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .is_some_and(|t| {
            t < chrono::Utc::now() - chrono::Duration::days(notify::REMIND_AFTER_DAYS)
        }))
}

pub(crate) async fn check(
    pool: &SqlitePool,
    target: &str,
    recipient: &str,
    match_id: Option<&str>,
) -> Result<(), String> {
    let prepared = prepare(pool, target, recipient, match_id)
        .await?
        .ok_or("Follow-up is not eligible for target")?;
    if !due(
        &mut *pool.acquire().await.map_err(err)?,
        recipient,
        match_id,
        &prepared,
    )
    .await?
    {
        return Err("Follow-up is not due or its reminder cap was reached".into());
    }
    Ok(())
}

/// Bounded observations contain identifiers only, never rendered message text.
pub(crate) async fn due_for_target(pool: &SqlitePool, target: &str) -> Result<Vec<Value>, String> {
    let mut candidates = vec![(target.to_owned(), None)];
    for (m, _) in MatchRepo::list(pool).await.map_err(err)? {
        if m.profile_a == target || m.profile_b == target {
            for recipient in [m.profile_a, m.profile_b] {
                candidates.push((recipient, Some(m.id.clone())));
            }
        }
    }
    let mut result = Vec::new();
    for (recipient, id) in candidates {
        if let Some(prepared) = prepare(pool, target, &recipient, id.as_deref()).await? {
            if due(
                &mut *pool.acquire().await.map_err(err)?,
                &recipient,
                id.as_deref(),
                &prepared,
            )
            .await?
            {
                result.push(if let Some(id) = id {
                    json!({"profile_id":recipient,"match_id":id})
                } else {
                    json!({"profile_id":recipient})
                });
                if result.len() == 100 {
                    break;
                }
            }
        }
    }
    Ok(result)
}

/// Reserve budget, enforce caps and enqueue the existing template atomically.
pub(crate) async fn queue(
    pool: &SqlitePool,
    actor: Actor,
    target: &str,
    recipient: &str,
    match_id: Option<&str>,
) -> Result<(), String> {
    if !actor.is_agent() {
        return Err("Agent actor required".into());
    }
    let prepared = prepare(pool, target, recipient, match_id)
        .await?
        .ok_or("Follow-up is not eligible for target")?;
    let policy = AutonomyRepo::load_policy(pool).await.map_err(err)?;
    let now = chrono::Utc::now();
    let mut tx = pool.begin().await.map_err(err)?;
    // Acquire SQLite's write lock before reading receipts. Snapshot checks also
    // reject edits/deactivation made while the templates were being prepared.
    for profile in &prepared.profiles {
        let admitted = sqlx::query("UPDATE mm_profiles SET updated_at=updated_at WHERE id=? AND status='active' AND updated_at=?")
            .bind(&profile.profile.id).bind(&profile.updated_at).execute(&mut *tx).await.map_err(err)?;
        if admitted.rows_affected() != 1 {
            return Err("Profile changed".into());
        }
    }
    if let Some(id) = match_id {
        let waiting: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mm_matches WHERE id=? AND hidden=0 AND hold_reason IS NULL AND status='introduction_proposed' AND ((profile_a=? AND a_response='unknown') OR (profile_b=? AND b_response='unknown'))")
            .bind(id).bind(recipient).bind(recipient).fetch_one(&mut *tx).await.map_err(err)?;
        if waiting != 1 {
            return Err("Answer changed".into());
        }
    }
    let reachable: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mm_telegram_links WHERE profile_id=? AND consent_version=? AND consented_at IS NOT NULL AND notifications_enabled=1 AND linked_at=?")
        .bind(recipient).bind(repo::CONSENT_VERSION).bind(&prepared.linked_at).fetch_one(&mut *tx).await.map_err(err)?;
    if reachable != 1 {
        return Err("Recipient unreachable".into());
    }
    if !due(&mut tx, recipient, match_id, &prepared).await? {
        return Err("Follow-up is not due or its reminder cap was reached".into());
    }
    let used: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM mm_agent_messages WHERE profile_id=? AND reserved_at>?",
    )
    .bind(recipient)
    .bind(now.timestamp() - 86400)
    .fetch_one(&mut *tx)
    .await
    .map_err(err)?;
    if !policy.budgets.message_allowed(used as u64) {
        return Err("message_limit".into());
    }
    sqlx::query(
        "INSERT INTO mm_agent_messages(profile_id,match_id,actor,reserved_at) VALUES (?,?,?,?)",
    )
    .bind(recipient)
    .bind(match_id)
    .bind(actor.as_text())
    .bind(now.timestamp())
    .execute(&mut *tx)
    .await
    .map_err(err)?;
    repo::enqueue_on_as(
        &mut tx,
        actor,
        recipient,
        prepared.kind,
        &prepared.text,
        prepared.keyboard.as_ref(),
        match_id,
    )
    .await
    .map_err(err)?;
    sqlx::query("INSERT INTO mm_audit_log(org_id,actor,action,entity,entity_id,detail,at) VALUES (?,?,'send_follow_up','profile',?,NULL,?)")
        .bind(ORG).bind(actor.as_text()).bind(recipient).bind(now.to_rfc3339()).execute(&mut *tx).await.map_err(err)?;
    tx.commit().await.map_err(err)?;
    Ok(())
}

/// Delivery rechecks current need and active participants without consuming a cap.
pub(crate) async fn still_eligible(
    pool: &SqlitePool,
    recipient: &str,
    match_id: Option<&str>,
) -> Result<bool, String> {
    Ok(prepare(pool, recipient, recipient, match_id)
        .await?
        .is_some())
}
