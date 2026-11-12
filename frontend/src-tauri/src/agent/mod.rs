pub(crate) mod budgets;
pub mod commands;
pub mod protocol;
pub mod repo;
pub mod scheduler;
mod tools;

use crate::ai::{models::TaskProfile, service::Session, settings};
use crate::mm::{autonomy::ai_gate, autonomy_repo::AutonomyRepo, repository::MmRepository};
use matchmaking_core::autonomy::{
    decide, Action as PolicyAction, Actor, AiGate, Decision, DecisionContext, Step,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::OnceLock;

fn runtime_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub(crate) async fn ensure_open(pool: &SqlitePool, session: &Session<'_>) -> Result<(), String> {
    let policy = AutonomyRepo::load_policy(pool)
        .await
        .map_err(|error| error.to_string())?;
    let config = settings::load_config(pool).await?;
    let consent = settings::load_consent(pool).await?;
    let mode = settings::check_gate_for(
        &config,
        consent.as_ref(),
        settings::load_key().ok().flatten().is_some(),
        session.locale,
    )?;
    if policy.paused
        || session.agent_config.as_ref() != Some(&config)
        || config.provider != session.provider
        || config.model != session.model
        || mode.is_cloud != session.mode.is_cloud
        || mode.include_sensitive != session.mode.include_sensitive
    {
        return Err("Agent paused or AI configuration changed".into());
    }
    Ok(())
}

async fn policy_decision(
    pool: &SqlitePool,
    action: &protocol::Action,
    target: &str,
    session: &Session<'_>,
) -> Result<Decision, String> {
    policy_decision_review(pool, action, target, session, None).await
}

async fn policy_decision_review(
    pool: &SqlitePool,
    action: &protocol::Action,
    target: &str,
    session: &Session<'_>,
    review: Option<(i64, i64)>,
) -> Result<Decision, String> {
    ensure_open(pool, session).await?;
    let stored = MmRepository::get_profile(pool, target)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("Target not found")?;
    if stored.status != "active" {
        return Err("Target is inactive".into());
    }
    let step = action.step()?;
    let mut policy = AutonomyRepo::load_policy(pool)
        .await
        .map_err(|error| error.to_string())?;
    let now = chrono::Utc::now().timestamp();
    if let Some((anchor, due)) = review {
        // Only elapsed windows become Auto for this evaluation. Suggest/Off and
        // every invariant still apply, including the independent override step.
        for mode in policy.steps.values_mut() {
            if let matchmaking_core::autonomy::StepMode::ReviewWindow { minutes } = *mode {
                if now >= due.max(anchor + i64::from(minutes) * 60) {
                    *mode = matchmaking_core::autonomy::StepMode::Auto;
                }
            }
        }
    }
    let context = DecisionContext {
        gate: ai_gate(pool).await?,
        now: review.map_or(now, |(anchor, _)| anchor),
    };
    if context.gate != AiGate::Open {
        return Err("AI gate is closed".into());
    }
    if matches!(action.tool.as_str(), "transition_match" | "add_note" | "propose_introduction" | "resend_interest_buttons") {
        tools::check_match_action(pool, target, action, session.locale).await?;
    }
    if action.tool == "send_follow_up" {
        tools::check_follow_up_action(pool, target, action).await?;
    }
    if step == Some(Step::AcceptExtraction) {
        return tools::acceptance_decision(pool, target, action, &context, &policy).await;
    }
    let decision = step.map_or(Decision::Execute, |step| {
        decide(&PolicyAction::Step(step), &context, &policy)
    });
    if action.tool == "transition_match" && action.override_reason().is_some() {
        // Neither permission can bypass the other, including edited proposal approvals.
        let override_decision = decide(&PolicyAction::Step(Step::RuleOverride), &context, &policy);
        Ok(decision.requiring(override_decision))
    } else {
        Ok(decision)
    }
}

async fn perform(
    pool: &SqlitePool,
    session: &Session<'_>,
    target: &str,
    run_id: i64,
    trace_id: i64,
    action: &protocol::Action,
    approved: bool,
    review: Option<(i64, i64)>,
) -> Result<Value, String> {
    let result = match policy_decision_review(pool, action, target, session, review).await {
        Ok(Decision::Execute) => {
            tools::execute(pool, session, target, Actor::Agent { run_id }, action).await
        }
        Ok(Decision::Propose) if approved => {
            tools::execute(pool, session, target, Actor::Agent { run_id }, action).await
        }
        _ => Err("Policy changed".into()),
    };
    let (status, observation) = match result {
        Ok(observation) => ("executed", observation),
        Err(error) if error == budgets::MODEL_CALL_LIMIT => (
            budgets::MODEL_CALL_LIMIT,
            json!({"budget":"model_calls_per_hour"}),
        ),
        Err(error) if matches!(error.as_str(), "introduction_limit" | "message_limit") => {
            let budget = if error == "introduction_limit" { "open_introductions_per_profile" } else { "messages_per_candidate_per_day" };
            (if error == "introduction_limit" { "introduction_limit" } else { "message_limit" }, json!({"budget":budget}))
        }
        Err(_) => ("failed", json!({"error":"tool_failed"})),
    };
    repo::update(pool, trace_id, status, &observation).await?;
    if matches!(status, budgets::MODEL_CALL_LIMIT | "introduction_limit" | "message_limit") {
        repo::finish(pool, run_id, status).await?;
    }
    MmRepository::audit_as(
        pool,
        Actor::Agent { run_id },
        "agent_tool",
        "agent_run",
        &run_id.to_string(),
        Some(&format!("{}:{status}", action.tool)),
    )
    .await;
    Ok(json!({"status":status,"result":observation}))
}

pub async fn run(
    pool: &SqlitePool,
    session: &Session<'_>,
    target: &str,
    batch_id: Option<&str>,
) -> Result<i64, String> {
    let _guard = runtime_lock()
        .try_lock()
        .map_err(|_| "An agent operation is already running")?;
    run_subject(pool, session, target, batch_id).await
}

async fn run_subject(
    pool: &SqlitePool,
    session: &Session<'_>,
    target: &str,
    batch_id: Option<&str>,
) -> Result<i64, String> {
    run_subject_cancellable(
        pool,
        session,
        target,
        batch_id,
        &tokio_util::sync::CancellationToken::new(),
    )
    .await
}

async fn run_subject_cancellable(
    pool: &SqlitePool,
    session: &Session<'_>,
    target: &str,
    batch_id: Option<&str>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<i64, String> {
    ensure_open(pool, session).await?;
    let stored = MmRepository::get_profile(pool, target)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("Target not found")?;
    if stored.status != "active" {
        return Err("Target is inactive".into());
    }
    let run_id = repo::start(pool, target, batch_id).await?;
    let outcome = run_loop(pool, session, target, run_id, cancel).await;
    repo::finish(pool, run_id, outcome.as_deref().unwrap_or("failed")).await?;
    MmRepository::audit_as(
        pool,
        Actor::Agent { run_id },
        "agent_run_finished",
        "agent_run",
        &run_id.to_string(),
        Some(outcome.as_deref().unwrap_or("failed")),
    )
    .await;
    Ok(run_id)
}

pub async fn run_batch(
    pool: &SqlitePool,
    session: &Session<'_>,
    batch_id: &str,
) -> Result<Vec<i64>, String> {
    let _guard = runtime_lock()
        .try_lock()
        .map_err(|_| "An agent operation is already running")?;
    let notes = crate::mm::intake_repo::IntakeRepo::notes_for_batch(pool, batch_id)
        .await
        .map_err(|error| error.to_string())?;
    if notes.is_empty() {
        return Err("Intake batch not found".into());
    }
    let mut runs = Vec::new();
    for note in notes {
        runs.push(run_subject(pool, session, &note.profile_id, Some(batch_id)).await?);
    }
    Ok(runs)
}

async fn run_loop(
    pool: &SqlitePool,
    session: &Session<'_>,
    target: &str,
    run_id: i64,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<&'static str, String> {
    let mut history = Vec::new();
    let constraint = protocol::constraint();
    for turn in 0usize.. {
        if cancel.is_cancelled() || ensure_open(pool, session).await.is_err() {
            return Ok("stopped");
        }
        let policy = AutonomyRepo::load_policy(pool)
            .await
            .map_err(|e| e.to_string())?;
        if !policy.budgets.step_allowed(turn as u64) {
            return Ok("step_limit");
        }
        let user = json!({"target_profile_id":target,"untrusted_observations":history}).to_string();
        let completion = async {
            let _model = crate::ai::service::lock().lock().await;
            if ensure_open(pool, session).await.is_err() {
                return None;
            }
            let policy = match AutonomyRepo::load_policy(pool).await {
                Ok(policy) => policy,
                Err(error) => return Some(Err(error.to_string())),
            };
            if !policy.budgets.step_allowed(turn as u64) {
                return Some(Err("step_limit".into()));
            }
            if let Err(error) = budgets::reserve_model_call(pool, run_id, "agent").await {
                return Some(Err(error));
            }
            Some(Ok(tokio::time::timeout(
                std::time::Duration::from_secs(TaskProfile::Agent.timeout_secs()),
                session.backend.complete_structured(
                    TaskProfile::Agent,
                    &protocol::system_prompt(),
                    &user,
                    &constraint,
                ),
            )
            .await))
        };
        tokio::pin!(completion);
        let mut policy_tick = tokio::time::interval(scheduler::TICK);
        let policy_closed = async {
            loop {
                policy_tick.tick().await;
                if ensure_open(pool, session).await.is_err() {
                    break;
                }
            }
        };
        let reply = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok("stopped"),
            _ = policy_closed => return Ok("stopped"),
            reply = &mut completion => match reply {
                Some(Ok(reply)) => reply,
                Some(Err(error)) if error == budgets::MODEL_CALL_LIMIT => return Ok(budgets::MODEL_CALL_LIMIT),
                Some(Err(error)) if error == "step_limit" => return Ok("step_limit"),
                Some(Err(error)) => return Err(error),
                None => return Ok("stopped"),
            }
        };
        if cancel.is_cancelled() || ensure_open(pool, session).await.is_err() {
            return Ok("stopped");
        }
        let full = format!("{}\n\n{user}", protocol::system_prompt());
        let _ = crate::ai::repo::log_call(
            pool,
            "agent",
            &session.provider,
            &session.model,
            session.mode,
            Some(target),
            None,
            full.chars().count(),
            reply
                .as_ref()
                .ok()
                .and_then(|result| result.as_ref().ok())
                .map_or(0, |raw| raw.chars().count()),
            matches!(&reply, Ok(Ok(_))),
            None,
            Some(&full),
        )
        .await;
        let action = match reply {
            Ok(Ok(raw)) => serde_json::from_str::<protocol::Action>(&raw)
                .ok()
                .filter(|action| action.validate().is_ok()),
            _ => None,
        };
        let Some(action) = action else {
            repo::record(
                pool,
                run_id,
                turn,
                &protocol::Action {
                    tool: "finish".into(),
                    args: json!({}),
                },
                "failed",
                &json!({"error":"invalid_action_or_model_failure"}),
            )
            .await?;
            return Ok("failed");
        };
        let decision = policy_decision(pool, &action, target, session).await;
        let (status, observation) = match decision {
            Ok(Decision::Execute) => ("executing", json!({})),
            Ok(Decision::Propose) => ("proposed", json!({"approval_required":true})),
            Ok(Decision::Schedule { at }) => (
                "scheduled",
                json!({"scheduled_at":chrono::Utc::now().timestamp(),"due_at":at}),
            ),
            _ => ("denied", json!({"error":"policy_or_invariant_denied"})),
        };
        let trace_id = repo::record(pool, run_id, turn, &action, status, &observation).await?;
        if status == "scheduled" {
            MmRepository::audit_as(
                pool,
                Actor::Agent { run_id },
                "agent_action_scheduled",
                "agent_step",
                &trace_id.to_string(),
                None,
            )
            .await;
        }
        let observation = if status == "executing" {
            perform(
                pool, session, target, run_id, trace_id, &action, false, None,
            )
            .await?
        } else {
            json!({"status":status,"result":observation})
        };
        if let Some(status @ ("model_call_limit" | "introduction_limit" | "message_limit")) = observation["status"].as_str() {
            return Ok(match status { "introduction_limit" => "introduction_limit", "message_limit" => "message_limit", _ => budgets::MODEL_CALL_LIMIT });
        }
        if action.tool == "finish" && status == "executing" {
            return Ok("finished");
        }
        history.push(json!({"tool":action.tool,"observation":observation}));
        if status == "denied" && ensure_open(pool, session).await.is_err() {
            return Ok("stopped");
        }
    }
    unreachable!("unbounded turn iterator")
}

pub async fn decide_proposal(
    pool: &SqlitePool,
    session: Option<&Session<'_>>,
    id: i64,
    approve: bool,
    args: Option<Value>,
) -> Result<(), String> {
    let _guard = runtime_lock()
        .try_lock()
        .map_err(|_| "An agent operation is already running")?;
    let proposal = repo::traces(pool, None, true)
        .await?
        .into_iter()
        .find(|trace| trace.id == id && trace.status == "proposed")
        .ok_or("Proposal not found")?;
    let mut action = proposal.action;
    if let Some(args) = args {
        action.args = args;
    }
    action.validate()?;
    if !approve {
        repo::claim(pool, id, &action, true).await?;
    } else {
        let session = session.ok_or("AI session required")?;
        let target = repo::target(pool, proposal.run_id).await?;
        match policy_decision(pool, &action, &target, session).await? {
            Decision::Execute | Decision::Propose => {}
            _ => return Err("Current policy denies this proposal".into()),
        }
        repo::claim(pool, id, &action, false).await?;
        perform(
            pool,
            session,
            &target,
            proposal.run_id,
            id,
            &action,
            true,
            None,
        )
        .await?;
    }
    MmRepository::audit_as(
        pool,
        Actor::Matchmaker,
        "agent_proposal_decided",
        "agent_step",
        &id.to_string(),
        Some(if approve { "approved" } else { "rejected" }),
    )
    .await;
    Ok(())
}

/// Veto does not need an AI session. A conditional update is the linearization
/// point shared with due-action claims; a successful veto can never execute.
pub async fn veto(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let changed = sqlx::query("UPDATE mm_agent_steps SET status='vetoed', decided_by='matchmaker' WHERE id=? AND status='scheduled' AND run_id IN (SELECT id FROM mm_agent_runs WHERE org_id=?)")
        .bind(id).bind(crate::mm::repository::ORG).execute(pool).await.map_err(|e| e.to_string())?.rows_affected();
    if changed != 1 {
        return Err("Scheduled action already claimed or not found".into());
    }
    MmRepository::audit_as(
        pool,
        Actor::Matchmaker,
        "agent_action_vetoed",
        "agent_step",
        &id.to_string(),
        None,
    )
    .await;
    Ok(())
}

/// Caller holds runtime_lock. Pending receipts survive restart and closed gates.
/// Claim before execution prevents duplicate writes; an interrupted claimed
/// action is never automatically retried, since its side effects may have run.
async fn process_due(
    pool: &SqlitePool,
    session: &Session<'_>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<bool, String> {
    if cancel.is_cancelled() || ensure_open(pool, session).await.is_err() {
        return Ok(false);
    }
    let now = chrono::Utc::now().timestamp();
    let pending = repo::next_due(pool, now).await?;
    let Some(trace) = pending else {
        return Ok(false);
    };
    let anchor = trace.scheduled_at.ok_or("Missing review anchor")?;
    let due = trace.due_at.ok_or("Missing review deadline")?;
    let target = repo::target(pool, trace.run_id).await?;
    let decision =
        policy_decision_review(pool, &trace.action, &target, session, Some((anchor, due))).await;
    if cancel.is_cancelled() || ensure_open(pool, session).await.is_err() {
        return Ok(false);
    }
    let status = match decision {
        Ok(Decision::Execute) => "executing",
        Ok(Decision::Propose) => "proposed",
        Ok(Decision::Schedule { at }) => {
            sqlx::query(
                "UPDATE mm_agent_steps SET due_at=MAX(due_at, ?) WHERE id=? AND status='scheduled'",
            )
            .bind(at)
            .bind(trace.id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            return Ok(true);
        }
        _ => "denied",
    };
    let changed =
        sqlx::query("UPDATE mm_agent_steps SET status=? WHERE id=? AND status='scheduled'")
            .bind(status)
            .bind(trace.id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?
            .rows_affected();
    if changed != 1 {
        return Ok(true);
    } // A veto won the race.
    if status == "executing" {
        perform(
            pool,
            session,
            &target,
            trace.run_id,
            trace.id,
            &trace.action,
            false,
            Some((anchor, due)),
        )
        .await?;
    } else {
        repo::update(
            pool,
            trace.id,
            status,
            &json!({"review_finished":true,"approval_required":status == "proposed"}),
        )
        .await?;
        MmRepository::audit_as(
            pool,
            Actor::Agent {
                run_id: trace.run_id,
            },
            "agent_review_finished",
            "agent_step",
            &trace.id.to_string(),
            Some(status),
        )
        .await;
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
