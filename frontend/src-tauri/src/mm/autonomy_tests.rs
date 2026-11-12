//! M12 phase 1: the deterministic acceptance step, the policy store and undo. No model is involved: suggestions are
//! inserted directly so confidence, conflicts and ambiguity are exact.

use super::autonomy::{ai_gate, settle_extraction, settle_with, undo_action};
use super::autonomy_commands::{get_policy, save_policy, set_paused};
use super::autonomy_repo::AutonomyRepo;
use super::repository::MmRepository;
use crate::ai::repo;
use crate::ai::service;
use crate::ai::settings::{save_config, AiConfig};
use matchmaking_core::autonomy::{Actor, AiGate, AutonomyPolicy, Level, Step, StepMode};
use matchmaking_core::{Locale, Profile, Provenance, Value};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Row, SqlitePool};
use std::collections::BTreeSet;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn txt(text: &str) -> Value {
    Value::Text(text.into())
}

async fn profile_with(pool: &SqlitePool, fields: &[(&str, Value, Provenance)]) {
    let mut profile = Profile::new("p1");
    for (key, value, source) in fields {
        profile.set(key, value.clone(), *source);
    }
    MmRepository::insert_profile(pool, &profile).await.unwrap();
}

async fn run_with(pool: &SqlitePool, suggestions: &[(&str, &str, f64, bool)]) -> i64 {
    let run = repo::insert_run(pool, Some("p1"), None, "extract", "rules", "rules", false, "h", None, &json!({})).await.unwrap();
    for (field, value, confidence, conflict) in suggestions {
        repo::insert_suggestion(pool, "p1", run, "field", field, &json!(value), "evidence", *confidence, *conflict, "rules", "ai").await.unwrap();
    }
    run
}

async fn status_of(pool: &SqlitePool, run: i64, field: &str) -> String {
    repo::suggestions_for_run(pool, run).await.unwrap().into_iter().find(|row| row.field == field).unwrap().status
}

async fn field_of(pool: &SqlitePool, field: &str) -> Option<matchmaking_core::profile::Entry> {
    MmRepository::get_profile(pool, "p1").await.unwrap().unwrap().profile.fields.get(field).cloned()
}

async fn settle(pool: &SqlitePool, run: i64, policy: &AutonomyPolicy, gate: AiGate) -> usize {
    settle_with(pool, "p1", run, &BTreeSet::new(), policy, gate, Actor::agent_deterministic()).await.unwrap()
}

async fn audit_actors(pool: &SqlitePool, action: &str) -> Vec<String> {
    sqlx::query("SELECT actor FROM mm_audit_log WHERE action = ? ORDER BY id").bind(action).fetch_all(pool).await.unwrap().iter().map(|row| row.get("actor")).collect()
}

#[tokio::test]
async fn l2_accepts_fields_at_or_above_the_threshold_and_leaves_the_rest() {
    let pool = test_pool().await;
    profile_with(&pool, &[("about", txt("text"), Provenance::Matchmaker)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.9, false), ("city", "Tehran", 0.85, false), ("education", "Masters", 0.84, false)]).await;

    let accepted = settle(&pool, run, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::Open).await;

    assert_eq!(accepted, 2);
    assert_eq!(status_of(&pool, run, "occupation").await, "accepted");
    assert_eq!(status_of(&pool, run, "city").await, "accepted");
    assert_eq!(status_of(&pool, run, "education").await, "pending");
    let entry = field_of(&pool, "occupation").await.unwrap();
    assert_eq!((entry.value, entry.source), (txt("Nurse"), Provenance::AiInferred));
    assert!(field_of(&pool, "education").await.is_none());
}

#[tokio::test]
async fn the_threshold_is_the_policy_setting() {
    let pool = test_pool().await;
    profile_with(&pool, &[("about", txt("text"), Provenance::Matchmaker)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.7, false)]).await;
    let mut policy = AutonomyPolicy::preset(Level::L2Supervised);

    assert_eq!(settle(&pool, run, &policy, AiGate::Open).await, 0);
    policy.min_confidence = 0.6;
    assert_eq!(settle(&pool, run, &policy, AiGate::Open).await, 1);
}

#[tokio::test]
async fn conflicts_ambiguous_fields_and_person_owned_values_are_never_accepted() {
    let pool = test_pool().await;
    profile_with(&pool, &[("occupation", txt("Teacher"), Provenance::User), ("education", txt("Bachelors"), Provenance::Questionnaire)]).await;
    let run = run_with(
        &pool,
        &[("occupation", "Nurse", 0.99, false), ("education", "Masters", 0.99, false), ("city", "Tehran", 0.99, true), ("smoking", "never", 0.99, false)],
    )
    .await;
    let ambiguous: BTreeSet<String> = ["smoking".to_string()].into();

    let accepted = settle_with(&pool, "p1", run, &ambiguous, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::Open, Actor::agent_deterministic()).await.unwrap();

    assert_eq!(accepted, 0);
    for field in ["occupation", "education", "city", "smoking"] {
        assert_eq!(status_of(&pool, run, field).await, "pending", "{field}");
    }
    assert_eq!(field_of(&pool, "occupation").await.unwrap().value, txt("Teacher"));
}

#[tokio::test]
async fn an_earlier_agent_value_may_be_replaced_but_a_person_may_not() {
    let pool = test_pool().await;
    profile_with(&pool, &[("occupation", txt("Student"), Provenance::AiInferred)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.95, false)]).await;

    assert_eq!(settle(&pool, run, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::Open).await, 1);
    assert_eq!(field_of(&pool, "occupation").await.unwrap().value, txt("Nurse"));
}

#[tokio::test]
async fn l1_l0_paused_and_a_closed_gate_accept_nothing() {
    let pool = test_pool().await;
    profile_with(&pool, &[("about", txt("text"), Provenance::Matchmaker)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.99, false)]).await;
    let mut paused = AutonomyPolicy::preset(Level::L2Supervised);
    paused.paused = true;

    assert_eq!(settle(&pool, run, &AutonomyPolicy::preset(Level::L1Assisted), AiGate::Open).await, 0);
    assert_eq!(settle(&pool, run, &AutonomyPolicy::preset(Level::L0Manual), AiGate::Open).await, 0);
    assert_eq!(settle(&pool, run, &paused, AiGate::Open).await, 0);
    assert_eq!(settle(&pool, run, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::Closed).await, 0);
    assert_eq!(settle(&pool, run, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::NeedsConsent).await, 0);
    assert_eq!(status_of(&pool, run, "occupation").await, "pending");
    assert!(field_of(&pool, "occupation").await.is_none());
}

#[tokio::test]
async fn audit_rows_and_actions_carry_the_agent_actor() {
    let pool = test_pool().await;
    profile_with(&pool, &[("about", txt("text"), Provenance::Matchmaker)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.9, false)]).await;
    settle(&pool, run, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::Open).await;

    assert_eq!(audit_actors(&pool, "ai_suggestion_accepted").await, vec!["agent:0".to_string()]);
    let actions = AutonomyRepo::active_actions(&pool, "p1").await.unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!((actions[0].field.as_str(), actions[0].actor.as_str(), actions[0].run_id), ("occupation", "agent:0", None));
    assert!(actions[0].prior.is_none());

    let value: String = sqlx::query_scalar("SELECT detail FROM mm_audit_log WHERE action = 'ai_suggestion_accepted'").fetch_one(&pool).await.unwrap();
    assert_eq!(value, "occupation", "the audit row names the field, never the value");
}

#[tokio::test]
async fn a_person_accepting_is_still_recorded_as_the_matchmaker() {
    let pool = test_pool().await;
    profile_with(&pool, &[("about", txt("text"), Provenance::Matchmaker)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.5, false)]).await;
    let id = repo::suggestions_for_run(&pool, run).await.unwrap()[0].id;

    service::decide_suggestion(&pool, id, service::Decision::Accept, Locale::En).await.unwrap();

    assert_eq!(audit_actors(&pool, "ai_suggestion_accepted").await, vec!["matchmaker".to_string()]);
    assert!(AutonomyRepo::active_actions(&pool, "p1").await.unwrap().is_empty());
}

#[tokio::test]
async fn undo_removes_a_field_that_was_absent_and_restores_one_that_was_replaced() {
    let pool = test_pool().await;
    profile_with(&pool, &[("occupation", txt("Student"), Provenance::AiInferred)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.9, false), ("city", "Tehran", 0.9, false)]).await;
    settle(&pool, run, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::Open).await;
    let actions = AutonomyRepo::active_actions(&pool, "p1").await.unwrap();
    assert_eq!(actions.len(), 2);

    for action in &actions {
        undo_action(&pool, action.id, Actor::Matchmaker, Locale::En).await.unwrap();
    }

    assert_eq!(field_of(&pool, "occupation").await.unwrap().value, txt("Student"));
    assert!(field_of(&pool, "city").await.is_none());
    assert!(AutonomyRepo::active_actions(&pool, "p1").await.unwrap().is_empty());
    assert_eq!(audit_actors(&pool, "agent_undo").await, vec!["matchmaker".to_string(), "matchmaker".to_string()]);
    assert!(undo_action(&pool, actions[0].id, Actor::Matchmaker, Locale::En).await.is_err(), "a second undo is refused");
}

#[tokio::test]
async fn undo_refuses_after_a_person_edited_the_field() {
    let pool = test_pool().await;
    profile_with(&pool, &[("about", txt("text"), Provenance::Matchmaker)]).await;
    let run = run_with(&pool, &[("occupation", "Nurse", 0.9, false)]).await;
    settle(&pool, run, &AutonomyPolicy::preset(Level::L2Supervised), AiGate::Open).await;
    let action = AutonomyRepo::active_actions(&pool, "p1").await.unwrap().remove(0);

    let mut stored = MmRepository::get_profile(&pool, "p1").await.unwrap().unwrap();
    assert!(stored.profile.set("occupation", txt("Doctor"), Provenance::User));
    MmRepository::update_profile_data(&pool, &stored.profile).await.unwrap();

    assert!(undo_action(&pool, action.id, Actor::Matchmaker, Locale::En).await.is_err());
    let entry = field_of(&pool, "occupation").await.unwrap();
    assert_eq!((entry.value, entry.source), (txt("Doctor"), Provenance::User));
    assert_eq!(AutonomyRepo::active_actions(&pool, "p1").await.unwrap().len(), 1);
}

#[tokio::test]
async fn the_policy_defaults_to_l0_and_is_versioned_and_validated() {
    let pool = test_pool().await;
    let view = get_policy(&pool, None).await.unwrap();
    assert_eq!((view.policy.level, view.policy.version, view.custom), (Level::L0Manual, 1, false));

    let saved = save_policy(&pool, &AutonomyPolicy::preset(Level::L2Supervised)).await.unwrap();
    assert_eq!(saved.policy.version, 1);
    let mut custom = AutonomyPolicy::preset(Level::L2Supervised);
    custom.steps.insert(Step::AcceptExtraction, StepMode::Suggest);
    let saved = save_policy(&pool, &custom).await.unwrap();
    assert_eq!((saved.policy.version, saved.custom), (2, true));
    assert_eq!(AutonomyRepo::load_policy(&pool).await.unwrap(), saved.policy);

    let saved = save_policy(&pool, &AutonomyPolicy::preset(Level::L4Autonomous)).await.unwrap();
    assert_eq!(saved.policy.level, Level::L4Autonomous);
    assert!(!saved.custom);
    let loaded = AutonomyRepo::load_policy(&pool).await.unwrap();
    assert_eq!(loaded, saved.policy);
    assert!(loaded.matches_preset(), "the L4 preset round-trips as a preset");
    assert_eq!(loaded.version, 3);
    assert_eq!(audit_actors(&pool, "agent_policy_saved").await.len(), 3);
}

#[tokio::test]
async fn a_database_without_a_policy_row_loads_l0() {
    let fresh = test_pool().await;
    let view = get_policy(&fresh, None).await.unwrap();
    assert_eq!((view.policy.level, view.policy.version, view.custom), (Level::L0Manual, 1, false));
    assert!(view.policy.matches_preset());

    let upgraded = test_pool().await;
    sqlx::query("DELETE FROM mm_agent_policy").execute(&upgraded).await.unwrap();
    let policy = AutonomyRepo::load_policy(&upgraded).await.unwrap();
    assert_eq!((policy.level, policy.version, policy.paused), (Level::L0Manual, 1, false));
    assert!(policy.matches_preset());
}

#[tokio::test]
async fn the_kill_switch_keeps_the_rest_of_the_policy() {
    let pool = test_pool().await;
    save_policy(&pool, &AutonomyPolicy::preset(Level::L2Supervised)).await.unwrap();

    let view = set_paused(&pool, true).await.unwrap();
    assert!(view.policy.paused && view.policy.level == Level::L2Supervised);
    assert!(AutonomyRepo::load_policy(&pool).await.unwrap().paused);
    assert!(!set_paused(&pool, false).await.unwrap().policy.paused);
}

async fn open_the_gate(pool: &SqlitePool) {
    let config = AiConfig { provider: "builtin".into(), model: "qwen3.5:4b".into(), base_url: String::new(), local_include_sensitive: true };
    save_config(pool, &config).await.unwrap();
}

#[tokio::test]
async fn the_gate_is_closed_until_a_provider_is_chosen() {
    let pool = test_pool().await;
    assert_eq!(ai_gate(&pool).await.unwrap(), AiGate::Closed);
    open_the_gate(&pool).await;
    assert_eq!(ai_gate(&pool).await.unwrap(), AiGate::Open);
}

#[tokio::test]
async fn an_extraction_run_settles_under_the_stored_policy() {
    let pool = test_pool().await;
    let about = "I have never smoked and I work as a nurse. I am 29 years old.";
    profile_with(&pool, &[("about", txt(about), Provenance::Matchmaker)]).await;
    open_the_gate(&pool).await;
    let mut policy = AutonomyPolicy::preset(Level::L2Supervised);
    policy.min_confidence = 0.0;
    save_policy(&pool, &policy).await.unwrap();

    let view = service::extract_profile(&pool, None, "p1", None, None, false).await.unwrap();

    assert!(!view.suggestions.is_empty());
    assert!(view.suggestions.iter().all(|row| row.status == "accepted" || row.conflict || row.kind != "field" || row.status == "pending"));
    let accepted = view.suggestions.iter().filter(|row| row.status == "accepted").count();
    assert!(accepted > 0, "the view reflects what the agent accepted");
    assert_eq!(AutonomyRepo::active_actions(&pool, "p1").await.unwrap().len(), accepted);

    set_paused(&pool, true).await.unwrap();
    let again = service::extract_profile(&pool, None, "p1", None, None, false).await.unwrap();
    assert!(again.suggestions.iter().all(|row| row.status == "pending"), "paused: everything stays pending");
    assert_eq!(settle_extraction(&pool, "p1", again.run_id, &BTreeSet::new()).await.unwrap(), 0);
}

#[tokio::test]
async fn l1_leaves_an_extraction_exactly_as_it_is_today() {
    let pool = test_pool().await;
    profile_with(&pool, &[("about", txt("I have never smoked and I work as a nurse."), Provenance::Matchmaker)]).await;
    open_the_gate(&pool).await;
    save_policy(&pool, &AutonomyPolicy::preset(Level::L1Assisted)).await.unwrap();

    let view = service::extract_profile(&pool, None, "p1", None, None, false).await.unwrap();

    assert!(!view.suggestions.is_empty() && view.suggestions.iter().all(|row| row.status == "pending"));
    assert!(AutonomyRepo::active_actions(&pool, "p1").await.unwrap().is_empty());
}
