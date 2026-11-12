//! What the agent does on its own (M12 phase 1): accept high-confidence extraction suggestions, and undo it.
//!
//! There is no model loop yet. After an extraction run a deterministic step looks at the pending field suggestions,
//! asks the policy engine about each one and, only on `Execute`, applies it through the same `decide_suggestion`
//! path a person uses, recorded with the agent actor. Nothing here writes to a profile directly except `undo`.

use super::autonomy_repo::{AgentAction, AutonomyRepo};
use super::repository::MmRepository;
use crate::ai::repo as ai_repo;
use crate::ai::service::{decide_suggestion_as, Decision as SuggestionDecision};
use crate::ai::settings::{self, GateBlock};
use matchmaking_core::autonomy::{acceptance_check, decide, Action, Actor, AiGate, AutonomyPolicy, Decision, DecisionContext, Step};
use matchmaking_core::profile::Entry;
use matchmaking_core::{Locale, Provenance, Value};
use sqlx::SqlitePool;
use std::collections::BTreeSet;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

/// The AI gate as the policy sees it. A closed gate or a missing consent stops the agent, rules-only runs included.
pub async fn ai_gate(pool: &SqlitePool) -> Result<AiGate, String> {
    let config = settings::load_config(pool).await?;
    let consent = settings::load_consent(pool).await?;
    let has_key = settings::load_key().ok().flatten().is_some();
    Ok(match settings::gate_block(&config, consent.as_ref(), has_key) {
        None => AiGate::Open,
        Some(GateBlock::Off | GateBlock::Invalid) => AiGate::Closed,
        Some(GateBlock::NeedsConsent | GateBlock::NeedsKey) => AiGate::NeedsConsent,
    })
}

/// Run the acceptance step after an extraction. Returns how many fields were accepted.
pub async fn settle_extraction(pool: &SqlitePool, profile_id: &str, run_id: i64, ambiguous: &BTreeSet<String>) -> Result<usize, String> {
    let policy = AutonomyRepo::load_policy(pool).await.map_err(err)?;
    if policy.paused || policy.mode_of(Step::AcceptExtraction) == matchmaking_core::autonomy::StepMode::Off {
        return Ok(0);
    }
    let gate = ai_gate(pool).await?;
    settle_with(pool, profile_id, run_id, ambiguous, &policy, gate, Actor::agent_deterministic()).await
}

/// [`settle_extraction`] with the policy, gate and actor given, so a test can drive every combination.
pub async fn settle_with(
    pool: &SqlitePool,
    profile_id: &str,
    run_id: i64,
    ambiguous: &BTreeSet<String>,
    policy: &AutonomyPolicy,
    gate: AiGate,
    actor: Actor,
) -> Result<usize, String> {
    let context = DecisionContext { gate, now: chrono::Utc::now().timestamp() };
    let mut accepted = 0;
    for suggestion in ai_repo::suggestions_for_run(pool, run_id).await.map_err(err)? {
        if suggestion.status != "pending" || suggestion.profile_id != profile_id {
            continue;
        }
        let Some(stored) = MmRepository::get_profile(pool, profile_id).await.map_err(err)? else { return Ok(accepted) };
        let Ok(value) = serde_json::from_value::<Value>(suggestion.payload.clone()) else { continue };
        let existing = stored.profile.fields.get(&suggestion.field);
        let check = acceptance_check(
            suggestion.kind == "field",
            suggestion.confidence,
            suggestion.conflict,
            ambiguous.contains(&suggestion.field),
            existing.map_or(false, |entry| entry.value == value),
            existing.map(|entry| entry.source.rank()),
            policy.min_confidence,
        );
        if check.is_err() {
            continue;
        }
        let provenance = if suggestion.source == "rule" { Provenance::RuleExtracted } else { Provenance::AiInferred };
        let action = Action::ApplyField { step: Step::AcceptExtraction, field: suggestion.field.clone(), provenance };
        if decide(&action, &context, policy) != Decision::Execute {
            continue;
        }
        let prior = existing.cloned();
        if decide_suggestion_as(pool, actor, suggestion.id, SuggestionDecision::Accept, Locale::En).await.is_err() {
            continue;
        }
        let Some(applied) = MmRepository::get_profile(pool, profile_id).await.map_err(err)?.and_then(|after| after.profile.fields.get(&suggestion.field).cloned()) else { continue };
        let run = match actor {
            Actor::Agent { run_id } if run_id != Actor::DETERMINISTIC_RUN => Some(run_id),
            _ => None,
        };
        let prior_json = prior.as_ref().map(serde_json::to_value).transpose().map_err(err)?;
        let applied_json = serde_json::to_value(&applied).map_err(err)?;
        AutonomyRepo::insert_action(pool, profile_id, &suggestion.field, &suggestion.kind, prior_json.as_ref(), &applied_json, Some(suggestion.id), &actor.as_text(), run)
            .await
            .map_err(err)?;
        accepted += 1;
    }
    Ok(accepted)
}

/// Undo one agent-applied field: put back what was there before, or remove the field if it was absent. Refused once
/// the value is no longer the one the agent wrote, so a later edit by a person is never overwritten.
pub async fn undo_action(pool: &SqlitePool, action_id: i64, actor: Actor, locale: Locale) -> Result<AgentAction, String> {
    let action = AutonomyRepo::get_action(pool, action_id).await.map_err(err)?.ok_or_else(|| locale.pick("Action not found", "اقدام پیدا نشد").to_string())?;
    if action.undone_at.is_some() {
        return Err(locale.pick("This change was already undone", "این تغییر پیش‌تر برگردانده شده است").to_string());
    }
    let mut stored = MmRepository::get_profile(pool, &action.profile_id).await.map_err(err)?.ok_or_else(|| locale.pick("Profile not found", "پروفایل پیدا نشد").to_string())?;
    let applied: Entry = serde_json::from_value(action.applied.clone()).map_err(err)?;
    if stored.profile.fields.get(&action.field) != Some(&applied) {
        return Err(locale
            .pick(
                "This field was changed after the AI accepted it, so it is not restored",
                "این فیلد پس از پذیرش توسط هوش مصنوعی تغییر کرده است، پس بازگردانده نمی‌شود",
            )
            .to_string());
    }
    match action.prior.clone() {
        Some(json) => {
            let prior: Entry = serde_json::from_value(json).map_err(err)?;
            if !stored.profile.set(&action.field, prior.value, prior.source) {
                return Err(locale.pick("The earlier value could not be restored", "مقدار قبلی بازگردانده نشد").to_string());
            }
        }
        None => {
            stored.profile.remove(&action.field);
        }
    }
    MmRepository::update_profile_data(pool, &stored.profile).await.map_err(err)?;
    AutonomyRepo::mark_undone(pool, action.id).await.map_err(err)?;
    MmRepository::audit_as(pool, actor, "agent_undo", "profile", &action.profile_id, Some(&action.field)).await;
    Ok(action)
}
