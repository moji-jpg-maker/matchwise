pub mod commands;
pub mod protocol;
pub mod repo;
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
    ensure_open(pool, session).await?;
    let stored = MmRepository::get_profile(pool, target)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("Target not found")?;
    if stored.status != "active" {
        return Err("Target is inactive".into());
    }
    let tool = action.validate()?;
    let policy = AutonomyRepo::load_policy(pool)
        .await
        .map_err(|error| error.to_string())?;
    let context = DecisionContext {
        gate: ai_gate(pool).await?,
        now: chrono::Utc::now().timestamp(),
    };
    if context.gate != AiGate::Open {
        return Err("AI gate is closed".into());
    }
    if tool.step == Some(Step::AcceptExtraction) {
        return tools::acceptance_decision(pool, target, action, &context, &policy).await;
    }
    Ok(tool.step.map_or(Decision::Execute, |step| {
        decide(&PolicyAction::Step(step), &context, &policy)
    }))
}

async fn perform(
    pool: &SqlitePool,
    session: &Session<'_>,
    target: &str,
    run_id: i64,
    trace_id: i64,
    action: &protocol::Action,
    approved: bool,
) -> Result<Value, String> {
    let result = match policy_decision(pool, action, target, session).await {
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
        Err(_) => ("failed", json!({"error":"tool_failed"})),
    };
    repo::update(pool, trace_id, status, &observation).await?;
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
    ensure_open(pool, session).await?;
    let stored = MmRepository::get_profile(pool, target)
        .await
        .map_err(|error| error.to_string())?
        .ok_or("Target not found")?;
    if stored.status != "active" {
        return Err("Target is inactive".into());
    }
    let run_id = repo::start(pool, target, batch_id).await?;
    let outcome = run_loop(pool, session, target, run_id).await;
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
) -> Result<&'static str, String> {
    let mut history = Vec::new();
    let constraint = protocol::constraint();
    for turn in 0..20 {
        if ensure_open(pool, session).await.is_err() {
            return Ok("stopped");
        }
        let user = json!({"target_profile_id":target,"untrusted_observations":history}).to_string();
        let reply = {
            let _model = crate::ai::service::lock().lock().await;
            if ensure_open(pool, session).await.is_err() {
                return Ok("stopped");
            }
            tokio::time::timeout(
                std::time::Duration::from_secs(TaskProfile::Agent.timeout_secs()),
                session.backend.complete_structured(
                    TaskProfile::Agent,
                    &protocol::system_prompt(),
                    &user,
                    &constraint,
                ),
            )
            .await
        };
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
            Ok(Decision::Schedule { .. }) => {
                ("denied", json!({"error":"review_windows_not_available"}))
            }
            _ => ("denied", json!({"error":"policy_or_invariant_denied"})),
        };
        let trace_id = repo::record(pool, run_id, turn, &action, status, &observation).await?;
        let observation = if status == "executing" {
            perform(pool, session, target, run_id, trace_id, &action, false).await?
        } else {
            json!({"status":status,"result":observation})
        };
        if action.tool == "finish" && status == "executing" {
            return Ok("finished");
        }
        history.push(json!({"tool":action.tool,"observation":observation}));
        if status == "denied" && ensure_open(pool, session).await.is_err() {
            return Ok("stopped");
        }
    }
    Ok("step_limit")
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
        .find(|trace| trace.id == id)
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
        perform(pool, session, &target, proposal.run_id, id, &action, true).await?;
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

#[cfg(test)]
mod tests;
