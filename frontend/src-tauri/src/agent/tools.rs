use super::protocol::Action;
use crate::ai::{
    repo as ai_repo,
    service::{self, Session},
};
use crate::mm::{
    autonomy_repo::AutonomyRepo,
    match_commands::{
        add_match_note_as, check_transition_as, compute_scorecard, create_match_as,
        transition_match_as,
    },
    match_repo::MatchRepo,
    repository::MmRepository,
    scoring_config_repo::ScoringConfigRepo,
};
use matchmaking_core::{
    autonomy::{
        acceptance_check, decide, Action as PolicyAction, Actor, AutonomyPolicy, Decision,
        DecisionContext, Step,
    },
    can_record_outcome, MatchStatus, Outcome, Provenance, TransitionRequest, Value,
};
use serde_json::{json, Value as Json};
use sqlx::SqlitePool;
use std::collections::BTreeMap;

pub async fn acceptance_decision(
    pool: &SqlitePool,
    target: &str,
    action: &Action,
    context: &DecisionContext,
    policy: &AutonomyPolicy,
) -> Result<Decision, String> {
    let id = action.args["suggestion_id"]
        .as_i64()
        .ok_or("Invalid suggestion")?;
    let suggestion = ai_repo::get_suggestion(pool, id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("Suggestion not found")?;
    if suggestion.profile_id != target || suggestion.status != "pending" {
        return Err("Suggestion is not pending for target".into());
    }
    if matchmaking_core::ai::never_for_ai(&suggestion.field) {
        return Err("Field is not available to the agent".into());
    }
    let result: String = sqlx::query_scalar("SELECT r.result FROM mm_ai_runs r JOIN mm_ai_suggestions s ON s.run_id = r.id WHERE s.id = ?").bind(id).fetch_one(pool).await.map_err(|error| error.to_string())?;
    let result: Json = serde_json::from_str(&result).map_err(|error| error.to_string())?;
    let ambiguous = result["ambiguous"].as_array().is_some_and(|fields| {
        fields
            .iter()
            .any(|field| field.as_str() == Some(&suggestion.field))
    });
    let stored = MmRepository::get_profile(pool, target)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("Profile not found")?;
    let value: Value =
        serde_json::from_value(suggestion.payload).map_err(|error| error.to_string())?;
    let existing = stored.profile.fields.get(&suggestion.field);
    acceptance_check(
        suggestion.kind == "field",
        suggestion.confidence,
        suggestion.conflict,
        ambiguous,
        existing.is_some_and(|entry| entry.value == value),
        existing.map(|entry| entry.source.rank()),
        policy.min_confidence,
    )
    .map_err(|error| format!("{error:?}"))?;
    let provenance = if suggestion.source == "rule" {
        Provenance::RuleExtracted
    } else {
        Provenance::AiInferred
    };
    Ok(decide(
        &PolicyAction::ApplyField {
            step: Step::AcceptExtraction,
            field: suggestion.field,
            provenance,
        },
        context,
        policy,
    ))
}

async fn redacted_profile(
    pool: &SqlitePool,
    id: &str,
    session: &Session<'_>,
) -> Result<Json, String> {
    let stored = MmRepository::get_profile(pool, id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("Profile not found")?;
    if stored.status != "active" {
        return Err("Profile inactive".into());
    }
    let registry = MmRepository::registry(pool)
        .await
        .map_err(|error| error.to_string())?;
    let names = match stored.profile.get("full_name") {
        Some(Value::Text(name)) => vec![name.clone()],
        _ => vec![],
    };
    let mut fields = serde_json::Map::new();
    for (key, entry) in &stored.profile.fields {
        let Some(definition) = registry.get(key) else {
            continue;
        };
        if matchmaking_core::ai::never_for_ai(key)
            || (definition.sensitive && !session.mode.include_sensitive)
            || matches!(entry.value, Value::Records(_))
        {
            continue;
        }
        let value = match &entry.value {
            Value::Text(text) => json!(matchmaking_core::ai::redact_text(
                &text.chars().take(2000).collect::<String>(),
                &names
            )),
            Value::List(items) => json!(items
                .iter()
                .take(50)
                .map(|text| matchmaking_core::ai::redact_text(
                    &text.chars().take(300).collect::<String>(),
                    &names
                ))
                .collect::<Vec<_>>()),
            other => serde_json::to_value(other).map_err(|error| error.to_string())?,
        };
        fields.insert(key.clone(), value);
    }
    let requestable_fields: Vec<_> = registry.defs().filter(|definition| {
        !stored.profile.fields.contains_key(&definition.key) && !definition.sensitive
            && !matchmaking_core::ai::never_for_ai(&definition.key)
            && !matchmaking_core::NEVER_SHARED.contains(&definition.key.as_str())
            && !matches!(definition.kind, matchmaking_core::FieldKind::Records(_))
    }).map(|definition| definition.key.clone()).collect();
    Ok(json!({"profile_id":id,"fields":fields,"requestable_fields":requestable_fields}))
}

/// Validate scope and lifecycle before a proposal is recorded, and before a tool executes.
pub async fn check_match_action(
    pool: &SqlitePool,
    target: &str,
    action: &Action,
    locale: matchmaking_core::Locale,
) -> Result<(), String> {
    let id = action.string("match_id");
    let pair = MatchRepo::get(pool, id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Match not found")?;
    if pair.profile_a != target && pair.profile_b != target {
        return Err("Match is outside target".into());
    }
    if action.tool == "record_outcome" {
        let outcome: Outcome = action.string("outcome").parse()?;
        let _ = outcome;
        if !can_record_outcome(pair.status) {
            return Err("An outcome can be recorded once contact has been exchanged, or when a person declined".into());
        }
    }
    if matches!(action.tool.as_str(), "propose_introduction" | "resend_interest_buttons") {
        crate::telegram::notify::check_agent_introduction(pool, id,
            (action.tool == "resend_interest_buttons").then(|| action.string("profile_id")), locale).await?;
    }
    if action.tool == "transition_match" {
        let to: MatchStatus = action.string("to_status").parse()?;
        check_transition_as(
            pool,
            Actor::agent_deterministic(),
            id,
            to,
            &TransitionRequest {
                reason: action.reason(),
                override_reason: action.override_reason(),
                outcome: None,
            },
            locale,
        )
        .await?;
    }
    Ok(())
}

pub async fn check_follow_up_action(pool: &SqlitePool, target: &str, action: &Action) -> Result<(), String> {
    action.validate()?;
    crate::telegram::follow_up::check(pool, target, action.string("profile_id"),
        action.args.get("match_id").and_then(Json::as_str)).await
}

pub async fn execute(
    pool: &SqlitePool,
    session: &Session<'_>,
    target: &str,
    actor: Actor,
    action: &Action,
) -> Result<Json, String> {
    super::ensure_open(pool, session).await?;
    match action.tool.as_str() {
        "finish" => Ok(json!({"finished":true})),
        "list_work" => {
            let suggestions = ai_repo::pending_suggestions(pool, target)
                .await
                .map_err(|error| error.to_string())?;
            let matches = MatchRepo::list(pool)
                .await
                .map_err(|error| error.to_string())?;
            let rules = MmRepository::list_rule_sets(pool, false)
                .await
                .map_err(|error| error.to_string())?;
            Ok(
                json!({"suggestions":suggestions.iter().take(100).map(|suggestion| json!({"suggestion_id":suggestion.id,"confidence":suggestion.confidence})).collect::<Vec<_>>(),
                "matches":matches.iter().filter(|(pair, _)| pair.profile_a == target || pair.profile_b == target).take(100).map(|(pair, card)| json!({"match_id":pair.id,"profile_a":pair.profile_a,"profile_b":pair.profile_b,"a_response":pair.a_response,"b_response":pair.b_response,"status":pair.status,"outcome":pair.outcome.map(|outcome| outcome.as_str()),"eligible":card.as_ref().and_then(|c| c["eligible"].as_bool()).unwrap_or(false)})).collect::<Vec<_>>(),
                "follow_ups":crate::telegram::follow_up::due_for_target(pool, target).await?,
                "rule_sets":rules.iter().map(|(rule, _)| json!({"rule_set_id":rule.id})).collect::<Vec<_>>()}),
            )
        }
        "get_profile_redacted" => {
            redacted_profile(pool, action.string("profile_id"), session).await
        }
        "run_extraction" => {
            let view =
                service::extract_profile_as(pool, Some(session), target, None, None, false, actor)
                    .await?;
            Ok(
                json!({"extraction_run_id":view.run_id,"suggestion_ids":view.suggestions.iter().map(|suggestion| suggestion.id).collect::<Vec<_>>()}),
            )
        }
        "accept_suggestion" => {
            let id = action.args["suggestion_id"]
                .as_i64()
                .ok_or("Invalid suggestion")?;
            let suggestion = ai_repo::get_suggestion(pool, id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or("Suggestion not found")?;
            let prior = MmRepository::get_profile(pool, target)
                .await
                .map_err(|error| error.to_string())?
                .and_then(|stored| stored.profile.fields.get(&suggestion.field).cloned());
            service::decide_suggestion_as(
                pool,
                actor,
                id,
                service::Decision::Accept,
                session.locale,
            )
            .await?;
            let applied = MmRepository::get_profile(pool, target)
                .await
                .map_err(|error| error.to_string())?
                .and_then(|stored| stored.profile.fields.get(&suggestion.field).cloned())
                .ok_or("Field was not applied")?;
            let prior = prior
                .map(serde_json::to_value)
                .transpose()
                .map_err(|error| error.to_string())?;
            let applied = serde_json::to_value(applied).map_err(|error| error.to_string())?;
            let run_id = match actor {
                Actor::Agent { run_id } => Some(run_id),
                _ => None,
            };
            AutonomyRepo::insert_action(
                pool,
                target,
                &suggestion.field,
                &suggestion.kind,
                prior.as_ref(),
                &applied,
                Some(id),
                &actor.as_text(),
                run_id,
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(json!({"suggestion_id":id,"accepted":true}))
        }
        "find_matches" => {
            let rule_id = action.string("rule_set_id");
            let (_, rules, _) = MmRepository::get_rule_set(pool, rule_id, None)
                .await
                .map_err(|error| error.to_string())?
                .ok_or("Rules not found")?;
            let config = ScoringConfigRepo::effective_for_rule_set(pool, rule_id)
                .await
                .map_err(|error| error.to_string())?;
            let profiles = MmRepository::list_profiles(pool, false)
                .await
                .map_err(|error| error.to_string())?;
            let mut matches = Vec::new();
            for candidate in profiles
                .into_iter()
                .filter(|candidate| candidate.profile.id != target)
            {
                let score = compute_scorecard(
                    pool,
                    &rules,
                    &config.definition,
                    target,
                    &candidate.profile.id,
                    &BTreeMap::new(),
                )
                .await?;
                if score.eligible {
                    matches.push((candidate.profile.id, score.overall));
                }
            }
            matches.sort_by(|left, right| {
                right
                    .1
                    .partial_cmp(&left.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.0.cmp(&right.0))
            });
            Ok(
                json!({"candidates":matches.into_iter().take(20).map(|(id, score)| json!({"profile_id":id,"score":score})).collect::<Vec<_>>()}),
            )
        }
        "create_match" => {
            let id = create_match_as(
                pool,
                target,
                action.string("profile_id"),
                action.string("rule_set_id"),
                Some(target),
                session.locale,
                actor,
            )
            .await?;
            Ok(json!({"match_id":id}))
        }
        "transition_match" | "add_note" => {
            action.validate()?;
            check_match_action(pool, target, action, session.locale).await?;
            let id = action.string("match_id");
            if action.tool == "transition_match" {
                let to: MatchStatus = action.string("to_status").parse()?;
                transition_match_as(
                    pool,
                    actor,
                    id,
                    to,
                    action.reason(),
                    action.override_reason(),
                    None,
                    session.locale,
                )
                .await?;
                Ok(json!({"match_id":id,"status":to}))
            } else {
                add_match_note_as(pool, actor, id, action.string("text"), session.locale).await?;
                Ok(json!({"match_id":id,"note_added":true}))
            }
        }
        "record_outcome" => {
            action.validate()?;
            check_match_action(pool, target, action, session.locale).await?;
            let id = action.string("match_id");
            let outcome: Outcome = action.string("outcome").parse()?;
            MatchRepo::set_outcome_as(pool, actor, id, outcome)
                .await
                .map_err(|error| error.to_string())?;
            MmRepository::audit_as(pool, actor, "match_outcome", "match", id, Some(outcome.as_str())).await;
            Ok(json!({"match_id":id,"outcome":outcome.as_str()}))
        }
        "propose_introduction" | "resend_interest_buttons" => {
            action.validate()?;
            check_match_action(pool, target, action, session.locale).await?;
            let id = action.string("match_id");
            let queued = crate::telegram::notify::queue_agent_introduction(pool, actor, id,
                (action.tool == "resend_interest_buttons").then(|| action.string("profile_id")), session.locale).await?;
            Ok(json!({"match_id":id,"queued":queued}))
        }
        "request_info" => {
            action.validate()?;
            crate::telegram::agent_message::queue_info(pool, actor, target, action.string("field"), action.string("text")).await?;
            Ok(json!({"profile_id":target,"field":action.string("field"),"queued":1}))
        }
        "send_message" => {
            action.validate()?;
            crate::telegram::agent_message::queue(pool, actor, target, action.string("profile_id"), action.string("match_id"), action.string("text")).await?;
            Ok(json!({"profile_id":action.string("profile_id"),"queued":1}))
        }
        "send_follow_up" => {
            check_follow_up_action(pool, target, action).await?;
            crate::telegram::follow_up::queue(pool, actor, target, action.string("profile_id"),
                action.args.get("match_id").and_then(Json::as_str)).await?;
            Ok(json!({"profile_id":action.string("profile_id"),"queued":1}))
        }
        "analyse_pair" => {
            let id = action.string("match_id");
            let pair = MatchRepo::get(pool, id)
                .await
                .map_err(|error| error.to_string())?
                .ok_or("Match not found")?;
            if pair.profile_a != target && pair.profile_b != target {
                return Err("Match is outside target".into());
            }
            let view = service::analyse_pair_as(pool, session, id, actor).await?;
            Ok(json!({"match_id":id,"analysis_run_id":view.run_id}))
        }
        _ => Err("Unknown tool".into()),
    }
}
