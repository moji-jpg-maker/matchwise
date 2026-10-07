//! Turning things that happen in the app into messages for candidates. Nothing here talks to Telegram directly:
//! messages go to the outbox and the worker delivers them. A person who is not linked, has not agreed to the
//! notice, or has stopped notifications is simply skipped (the matchmaker is told who could be reached).

use super::repo::{self, Link};
use super::render;
use super::types::{button, Keyboard};
use crate::mm::match_repo::{MatchRepo, MatchRow};
use crate::mm::repository::{MmRepository, StoredProfile};
use matchmaking_core::{build_introduction_card, first_name, IntroCard, MatchStatus};
use sqlx::SqlitePool;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// Reachable = linked, agreed to the notice, and notifications on.
pub async fn reachable_link(pool: &SqlitePool, profile_id: &str) -> Result<Option<Link>, String> {
    Ok(repo::link_by_profile(pool, profile_id).await.map_err(err)?.filter(|l| l.consented && l.notifications_enabled))
}

pub async fn card_for(pool: &SqlitePool, profile: &StoredProfile) -> Result<IntroCard, String> {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let fields = repo::introduction_fields(pool).await.map_err(err)?;
    let first = repo::show_first_name(pool).await.map_err(err)?;
    Ok(build_introduction_card(&profile.profile, &reg, &fields, first))
}

pub fn intro_keyboard(match_id: &str) -> Keyboard {
    vec![vec![button("I'm interested", format!("intro:y:{match_id}")), button("Not interested", format!("intro:n:{match_id}"))]]
}

/// The introduction message `recipient` would receive about `other`.
pub async fn introduction_message(pool: &SqlitePool, m: &MatchRow, other_id: &str) -> Result<(String, Keyboard), String> {
    let other = MmRepository::get_profile(pool, other_id).await.map_err(err)?.ok_or("Profile not found")?;
    let card = card_for(pool, &other).await?;
    Ok((render::introduction_text(&card, true), intro_keyboard(&m.id)))
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
                if reachable_link(pool, me).await?.is_none() {
                    continue;
                }
                let (text, kb) = introduction_message(pool, &m, other_of(&m, me)).await?;
                repo::enqueue(pool, me, "introduction", &text, Some(&kb), Some(&m.id)).await.map_err(err)?;
            }
        }
        MatchStatus::BothInterested => {
            for me in [&m.profile_a, &m.profile_b] {
                if reachable_link(pool, me).await?.is_none() {
                    continue;
                }
                let other = MmRepository::get_profile(pool, other_of(&m, me)).await.map_err(err)?.ok_or("Profile not found")?;
                let name = first_name(&other.profile, repo::show_first_name(pool).await.map_err(err)?);
                repo::enqueue(pool, me, "both_interested", &render::both_interested_text(&name), None, Some(&m.id)).await.map_err(err)?;
            }
        }
        MatchStatus::Declined => {
            // Tell the person who did not decline, neutrally: never say who said no.
            for (me, mine) in [(&m.profile_a, m.a_response), (&m.profile_b, m.b_response)] {
                if mine == matchmaking_core::Interest::NotInterested {
                    continue;
                }
                if reachable_link(pool, me).await?.is_some() && was_introduced(pool, me, &m.id).await? {
                    repo::enqueue(pool, me, "not_going_ahead", &render::not_going_ahead_text(), None, Some(&m.id)).await.map_err(err)?;
                }
            }
        }
        MatchStatus::Rejected if from == MatchStatus::IntroductionProposed => {
            for me in [&m.profile_a, &m.profile_b] {
                if reachable_link(pool, me).await?.is_some() && was_introduced(pool, me, &m.id).await? {
                    repo::enqueue(pool, me, "withdrawn", &render::withdrawn_text(), None, Some(&m.id)).await.map_err(err)?;
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
            if mine != matchmaking_core::Interest::Unknown || reachable_link(pool, me).await?.is_none() {
                continue;
            }
            let last = repo::last_created(pool, me, &["introduction", "intro_reminder"], Some(&m.id)).await.map_err(err)?;
            if last.is_none() {
                continue; // never introduced through Telegram: nothing to remind about
            }
            let sent = repo::count_kind(pool, me, "intro_reminder", Some(&m.id)).await.map_err(err)?;
            if sent >= MAX_INTRO_REMINDERS || !older_than(&last, REMIND_AFTER_DAYS) {
                continue;
            }
            let other = MmRepository::get_profile(pool, other_of(&m, me)).await.map_err(err)?.ok_or("Profile not found")?;
            let name = first_name(&other.profile, repo::show_first_name(pool).await.map_err(err)?);
            repo::enqueue(pool, me, "intro_reminder", &render::reminder_intro_text(&name), Some(&intro_keyboard(&m.id)), Some(&m.id)).await.map_err(err)?;
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
        let labels: Vec<String> = missing.iter().filter_map(|k| reg.get(k).map(|d| d.label.clone())).collect();
        repo::enqueue(pool, &sp.profile.id, "profile_reminder", &render::reminder_profile_text(&labels), None, None).await.map_err(err)?;
        queued += 1;
    }
    Ok(queued)
}
