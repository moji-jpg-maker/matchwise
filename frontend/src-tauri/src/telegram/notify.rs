//! Turning things that happen in the app into messages for candidates. Nothing here talks to Telegram directly:
//! messages go to the outbox and the worker delivers them. A person who is not linked, has not agreed to the
//! notice, or has stopped notifications is simply skipped (the matchmaker is told who could be reached).

use super::repo::{self, Link};
use super::render;
use super::types::{button, Keyboard};
use crate::mm::match_repo::{MatchRepo, MatchRow};
use crate::mm::repository::{MmRepository, StoredProfile};
use matchmaking_core::{build_introduction_card_for, first_name_for, IntroCard, Locale, MatchStatus};
use sqlx::SqlitePool;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Reachable = linked, agreed to the notice, and notifications on.
pub async fn reachable_link(pool: &SqlitePool, profile_id: &str) -> Result<Option<Link>, String> {
    Ok(repo::link_by_profile(pool, profile_id).await.map_err(err)?.filter(|l| l.consented && l.notifications_enabled))
}

pub async fn card_for(pool: &SqlitePool, profile: &StoredProfile, locale: Locale) -> Result<IntroCard, String> {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let fields = repo::introduction_fields(pool).await.map_err(err)?;
    let first = repo::show_first_name(pool).await.map_err(err)?;
    Ok(build_introduction_card_for(&profile.profile, &reg, &fields, first, locale))
}

pub fn intro_keyboard(locale: Locale, match_id: &str) -> Keyboard {
    vec![vec![
        button(locale.pick("I'm interested", "علاقه‌مندم"), format!("intro:y:{match_id}")),
        button(locale.pick("Not interested", "علاقه‌ای ندارم"), format!("intro:n:{match_id}")),
    ]]
}

/// The introduction message `recipient` would receive about `other`.
pub async fn introduction_message(pool: &SqlitePool, m: &MatchRow, other_id: &str, locale: Locale) -> Result<(String, Keyboard), String> {
    let other = MmRepository::get_profile(pool, other_id).await.map_err(err)?.ok_or("Profile not found")?;
    let card = card_for(pool, &other, locale).await?;
    Ok((render::introduction_text(locale, &card, true), intro_keyboard(locale, &m.id)))
}

fn other_of<'a>(m: &'a MatchRow, me: &str) -> &'a str {
    if m.profile_a == me { &m.profile_b } else { &m.profile_a }
}

/// Which sides of a match could receive an introduction right now: (profile_a reachable, profile_b reachable).
pub async fn reachable_sides(pool: &SqlitePool, m: &MatchRow) -> Result<(bool, bool), String> {
    Ok((reachable_link(pool, &m.profile_a).await?.is_some(), reachable_link(pool, &m.profile_b).await?.is_some()))
}

/// Called after a match changes status, whoever caused it (the matchmaker in the app, or a person in the bot).
/// Failures are logged, never returned: a notification problem must not undo a decision.
pub async fn after_status_change(pool: &SqlitePool, match_id: &str, from: MatchStatus, to: MatchStatus) {
    if let Err(e) = after_status_change_inner(pool, match_id, from, to).await {
        log::warn!("notification for match {match_id} failed: {e}");
    }
}

async fn after_status_change_inner(pool: &SqlitePool, match_id: &str, from: MatchStatus, to: MatchStatus) -> Result<(), String> {
    let Some(m) = MatchRepo::get(pool, match_id).await.map_err(err)? else { return Ok(()) };
    match to {
        MatchStatus::IntroductionProposed => {
            for me in [&m.profile_a, &m.profile_b] {
                let Some(link) = reachable_link(pool, me).await? else { continue };
                let (text, kb) = introduction_message(pool, &m, other_of(&m, me), link.locale).await?;
                repo::enqueue(pool, me, "introduction", &text, Some(&kb), Some(&m.id)).await.map_err(err)?;
            }
        }
        MatchStatus::BothInterested => {
            for me in [&m.profile_a, &m.profile_b] {
                let Some(link) = reachable_link(pool, me).await? else { continue };
                let other = MmRepository::get_profile(pool, other_of(&m, me)).await.map_err(err)?.ok_or("Profile not found")?;
                let name = first_name_for(&other.profile, repo::show_first_name(pool).await.map_err(err)?, link.locale);
                repo::enqueue(pool, me, "both_interested", &render::both_interested_text(link.locale, &name), None, Some(&m.id)).await.map_err(err)?;
            }
        }
        MatchStatus::Declined => {
            // Tell the person who did not decline, neutrally: never say who said no.
            for (me, mine) in [(&m.profile_a, m.a_response), (&m.profile_b, m.b_response)] {
                if mine == matchmaking_core::Interest::NotInterested {
                    continue;
                }
                let Some(link) = reachable_link(pool, me).await? else { continue };
                if was_introduced(pool, me, &m.id).await? {
                    repo::enqueue(pool, me, "not_going_ahead", &render::not_going_ahead_text(link.locale), None, Some(&m.id)).await.map_err(err)?;
                }
            }
        }
        MatchStatus::Rejected if from == MatchStatus::IntroductionProposed => {
            for me in [&m.profile_a, &m.profile_b] {
                let Some(link) = reachable_link(pool, me).await? else { continue };
                if was_introduced(pool, me, &m.id).await? {
                    repo::enqueue(pool, me, "withdrawn", &render::withdrawn_text(link.locale), None, Some(&m.id)).await.map_err(err)?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

async fn was_introduced(pool: &SqlitePool, profile_id: &str, match_id: &str) -> Result<bool, String> {
    Ok(repo::count_kind(pool, profile_id, "introduction", Some(match_id)).await.map_err(err)? > 0)
}

pub const REMIND_AFTER_DAYS: i64 = 3;
pub const MAX_INTRO_REMINDERS: i64 = 2;
pub const MAX_PROFILE_REMINDERS: i64 = 3;

fn older_than(ts: &Option<String>, days: i64) -> bool {
    match ts.as_deref().and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok()) {
        Some(t) => t < chrono::Utc::now() - chrono::Duration::days(days),
        None => true,
    }
}

/// Follow-up reminders: introductions still waiting for an answer, and incomplete profiles. Safe to call often.
/// Returns how many reminders were queued.
pub async fn queue_reminders(pool: &SqlitePool) -> Result<usize, String> {
    let mut queued = 0;
    // introductions still waiting for someone's answer
    for (m, _) in MatchRepo::list(pool).await.map_err(err)? {
        if m.status != MatchStatus::IntroductionProposed || m.hold_reason.is_some() {
            continue;
        }
        for (me, mine) in [(&m.profile_a, m.a_response), (&m.profile_b, m.b_response)] {
            if mine != matchmaking_core::Interest::Unknown {
                continue;
            }
            let Some(link) = reachable_link(pool, me).await? else { continue };
            let last = repo::last_created(pool, me, &["introduction", "intro_reminder"], Some(&m.id)).await.map_err(err)?;
            if last.is_none() {
                continue; // never introduced through Telegram: nothing to remind about
            }
            let sent = repo::count_kind(pool, me, "intro_reminder", Some(&m.id)).await.map_err(err)?;
            if sent >= MAX_INTRO_REMINDERS || !older_than(&last, REMIND_AFTER_DAYS) {
                continue;
            }
            let other = MmRepository::get_profile(pool, other_of(&m, me)).await.map_err(err)?.ok_or("Profile not found")?;
            let name = first_name_for(&other.profile, repo::show_first_name(pool).await.map_err(err)?, link.locale);
            repo::enqueue(pool, me, "intro_reminder", &render::reminder_intro_text(link.locale, &name), Some(&intro_keyboard(link.locale, &m.id)), Some(&m.id)).await.map_err(err)?;
            queued += 1;
        }
    }
    // profiles that still lack required information
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    for sp in MmRepository::list_profiles(pool, false).await.map_err(err)? {
        let missing = sp.profile.missing_required(&reg);
        if missing.is_empty() {
            continue;
        }
        let Some(link) = reachable_link(pool, &sp.profile.id).await? else { continue };
        // The first reminder is due a few days after linking, not straight away.
        let last = repo::last_created(pool, &sp.profile.id, &["profile_reminder"], None).await.map_err(err)?.or(Some(link.linked_at.clone()));
        let sent = repo::count_kind(pool, &sp.profile.id, "profile_reminder", None).await.map_err(err)?;
        if sent >= MAX_PROFILE_REMINDERS || !older_than(&last, REMIND_AFTER_DAYS) {
            continue;
        }
        let labels: Vec<String> = missing.iter().filter_map(|k| reg.get(k).map(|d| d.label_for(link.locale).to_string())).collect();
        repo::enqueue(pool, &sp.profile.id, "profile_reminder", &render::reminder_profile_text(link.locale, &labels), None, None).await.map_err(err)?;
        queued += 1;
    }
    Ok(queued)
}

/// Candidate-facing agent actions share the human lifecycle and safe card renderer.
/// Validate before proposing, and again immediately before delivery admission.
pub(crate) async fn check_agent_introduction(
    pool: &SqlitePool,
    id: &str,
    recipient: Option<&str>,
    locale: Locale,
) -> Result<MatchRow, String> {
    use matchmaking_core::{autonomy::Actor, Interest, TransitionRequest};
    let m = MatchRepo::get(pool, id)
        .await
        .map_err(err)?
        .ok_or("Match not found")?;
    if m.hidden || m.hold_reason.is_some() {
        return Err(locale
            .pick(
                "Introduction is hidden or on hold",
                "معرفی پنهان یا در انتظار اطلاعات است",
            )
            .into());
    }
    if let Some(me) = recipient {
        let response = if me == m.profile_a {
            m.a_response
        } else if me == m.profile_b {
            m.b_response
        } else {
            return Err("Recipient is outside match".into());
        };
        if m.status != MatchStatus::IntroductionProposed || response != Interest::Unknown {
            return Err(locale
                .pick(
                    "Introduction is no longer awaiting this answer",
                    "معرفی دیگر منتظر این پاسخ نیست",
                )
                .into());
        }
        if !was_introduced(pool, me, id).await? {
            return Err("Recipient has not been introduced".into());
        }
    } else {
        crate::mm::match_commands::check_transition_as(
            pool,
            Actor::agent_deterministic(),
            id,
            MatchStatus::IntroductionProposed,
            &TransitionRequest::default(),
            locale,
        )
        .await?;
    }
    for me in [&m.profile_a, &m.profile_b] {
        if MmRepository::get_profile(pool, me)
            .await
            .map_err(err)?
            .ok_or("Profile not found")?
            .status
            != "active"
        {
            return Err("Profile inactive".into());
        }
        if reachable_link(pool, me).await?.is_none() {
            return Err(locale
                .pick(
                    "Both candidates must be linked with consent and notifications enabled",
                    "هر دو متقاضی باید متصل باشند و رضایت و اعلان فعال داشته باشند",
                )
                .into());
        }
    }
    Ok(m)
}

/// Commit the lifecycle event, durable message receipts, outbox and audit together.
/// No outbound text is supplied by the model. Resends never write Interest.
pub(crate) async fn queue_agent_introduction(
    pool: &SqlitePool,
    actor: matchmaking_core::autonomy::Actor,
    id: &str,
    recipient: Option<&str>,
    locale: Locale,
) -> Result<usize, String> {
    use crate::mm::{autonomy_repo::AutonomyRepo, repository::ORG};
    if !actor.is_agent() {
        return Err("Agent actor required".into());
    }
    let m = check_agent_introduction(pool, id, recipient, locale).await?;
    let policy = AutonomyRepo::load_policy(pool).await.map_err(err)?;
    let mut messages = Vec::new();
    for me in [&m.profile_a, &m.profile_b] {
        if recipient.is_some_and(|selected| selected != me) {
            continue;
        }
        let link = reachable_link(pool, me)
            .await?
            .ok_or("Recipient unreachable")?;
        let (text, keyboard) =
            introduction_message(pool, &m, other_of(&m, me), link.locale).await?;
        messages.push((me.clone(), text, keyboard));
    }
    let now = chrono::Utc::now();
    let mut tx = pool.begin().await.map_err(err)?;
    // Acquire the write lock and reject stale status/hold/visibility before reserving anything.
    let admitted = sqlx::query("UPDATE mm_matches SET updated_at = updated_at WHERE id = ? AND status = ? AND hidden = 0 AND hold_reason IS NULL")
        .bind(id).bind(m.status.as_str()).execute(&mut *tx).await.map_err(err)?;
    if admitted.rows_affected() != 1 {
        return Err("Match changed".into());
    }
    for me in [&m.profile_a, &m.profile_b] {
        let reachable: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mm_telegram_links l JOIN mm_profiles p ON p.id=l.profile_id WHERE l.profile_id=? AND p.status='active' AND l.consent_version=? AND l.consented_at IS NOT NULL AND l.notifications_enabled=1")
            .bind(me).bind(repo::CONSENT_VERSION).fetch_one(&mut *tx).await.map_err(err)?;
        if reachable != 1 {
            return Err("Recipient unreachable".into());
        }
        if recipient.is_none() {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mm_matches WHERE org_id=? AND (profile_a=? OR profile_b=?) AND status IN ('introduction_proposed','both_interested','contact_exchanged','conversation','meeting','feedback')")
                .bind(ORG).bind(me).bind(me).fetch_one(&mut *tx).await.map_err(err)?;
            if count as u64 >= u64::from(policy.budgets.open_introductions_per_profile) {
                return Err("introduction_limit".into());
            }
        }
    }
    if let Some(me) = recipient {
        let unanswered: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mm_matches WHERE id=? AND ((profile_a=? AND a_response='unknown') OR (profile_b=? AND b_response='unknown'))")
            .bind(id).bind(me).bind(me).fetch_one(&mut *tx).await.map_err(err)?;
        if unanswered != 1 {
            return Err("Answer changed".into());
        }
    }
    for (me, text, keyboard) in &messages {
        // Rolling 24 hours, including pending/failed deliveries. Cleanup cannot reset usage.
        let used: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM mm_agent_messages WHERE profile_id=? AND reserved_at>?",
        )
        .bind(me)
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
        .bind(me)
        .bind(id)
        .bind(actor.as_text())
        .bind(now.timestamp())
        .execute(&mut *tx)
        .await
        .map_err(err)?;
        repo::enqueue_on_as(
            &mut tx,
            actor,
            me,
            if recipient.is_some() {
                "interest_buttons"
            } else {
                "introduction"
            },
            text,
            Some(keyboard),
            Some(id),
        )
        .await
        .map_err(err)?;
    }
    if recipient.is_none() {
        MatchRepo::record_transition_on(
            &mut tx,
            actor,
            id,
            MatchStatus::Approved,
            MatchStatus::IntroductionProposed,
            None,
            None,
            None,
        )
        .await
        .map_err(err)?;
    }
    let action = if recipient.is_some() {
        "resend_interest_buttons"
    } else {
        "propose_introduction"
    };
    sqlx::query("INSERT INTO mm_audit_log(org_id,actor,action,entity,entity_id,detail,at) VALUES (?,?,?,'match',?,NULL,?)")
        .bind(ORG).bind(actor.as_text()).bind(action).bind(id).bind(now.to_rfc3339()).execute(&mut *tx).await.map_err(err)?;
    tx.commit().await.map_err(err)?;
    Ok(messages.len())
}
