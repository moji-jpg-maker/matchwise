//! Orchestration: gate -> build a redacted prompt -> ask the model -> validate -> store. Nothing here applies a
//! model's output to a profile, a score or a match: the matchmaker decides (see `decide_suggestion`).

use super::backend::LlmBackend;
use super::repo::{self, SuggestionRow};
use super::settings::Mode;
use crate::mm::match_commands::compute_scorecard;
use crate::mm::match_repo::MatchRepo;
use crate::mm::repository::MmRepository;
use matchmaking_core::{
    build_pair_facts, extraction_prompt, pair_prompt, parse_extraction, parse_pair_analysis, profile_text_for_ai, validate_preference, AiFact,
    Extraction, PairAnalysis, Provenance, Value,
};
use serde::Serialize;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::sync::OnceLock;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn hash(s: &str) -> String {
    Sha256::digest(s.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// One model request at a time: a local model on a laptop should not be asked three things at once.
fn lock() -> &'static tokio::sync::Mutex<()> {
    static L: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub struct Session<'a> {
    pub backend: &'a dyn LlmBackend,
    pub provider: String,
    pub model: String,
    pub mode: Mode,
}

/// Ask the model, check the answer with `parse`, and on unreadable output ask once more. Every attempt is logged.
async fn ask<T>(
    pool: &SqlitePool,
    s: &Session<'_>,
    kind: &str,
    ids: (Option<&str>, Option<&str>),
    system: &str,
    user: &str,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = lock().lock().await;
    let _ = repo::purge_old_prompts(pool, 30).await;
    let mut prompt = user.to_string();
    let mut last_err = String::new();
    for attempt in 0..2 {
        let result = s.backend.complete(system, &prompt, true).await;
        let (ok, out_len, error) = match &result {
            Ok(text) => (true, text.chars().count(), None),
            Err(e) => (false, 0, Some(e.to_string())),
        };
        let full = format!("{system}\n\n{prompt}");
        let _ = repo::log_call(pool, kind, &s.provider, &s.model, s.mode, ids.0, ids.1, full.chars().count(), out_len, ok, error.as_deref(), Some(&full)).await;
        let raw = result.map_err(err)?;
        match parse(&raw) {
            Ok(v) => return Ok(v),
            Err(e) => {
                last_err = e;
                if attempt == 0 {
                    prompt = format!("{user}\n\nYour previous answer could not be used ({last_err}). Reply with ONLY the JSON object described above.");
                }
            }
        }
    }
    Err(format!("The model's answer could not be used: {last_err}"))
}

// ------------------------------------------------------------------ profile understanding

#[derive(Debug, Serialize)]
pub struct ExtractionView {
    pub run_id: i64,
    pub model: String,
    pub is_cloud: bool,
    pub created_at: String,
    pub suggestions: Vec<SuggestionRow>,
    pub contradictions: Vec<matchmaking_core::Contradiction>,
    pub missing: Vec<String>,
    pub questions: Vec<String>,
    /// What was discarded and why (a model's invented or invalid output is never silently hidden).
    pub dropped: Vec<String>,
}

/// The exact prompt that would be sent, for the matchmaker to read first.
pub async fn preview_extraction(pool: &SqlitePool, profile_id: &str, extra: Option<&str>, mode: Mode) -> Result<(String, String), String> {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let p = MmRepository::get_profile(pool, profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    let text = profile_text_for_ai(&p.profile, &reg, extra, mode.include_sensitive);
    if text.trim().is_empty() {
        return Err("There is no free text to read. Add notes about the person (for example from an intake interview) first.".into());
    }
    Ok(extraction_prompt(&reg, &text, mode.include_sensitive))
}

pub async fn extract_profile(pool: &SqlitePool, s: &Session<'_>, profile_id: &str, extra: Option<&str>) -> Result<ExtractionView, String> {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let p = MmRepository::get_profile(pool, profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    let text = profile_text_for_ai(&p.profile, &reg, extra, s.mode.include_sensitive);
    if text.trim().is_empty() {
        return Err("There is no free text to read. Add notes about the person (for example from an intake interview) first.".into());
    }
    let (system, user) = extraction_prompt(&reg, &text, s.mode.include_sensitive);
    let ex: Extraction = ask(pool, s, "extract", (Some(profile_id), None), &system, &user, |raw| {
        parse_extraction(raw, &reg, &text, &p.profile, s.mode.include_sensitive)
    })
    .await?;

    let run_id = repo::insert_run(pool, Some(profile_id), None, "extract", &s.provider, &s.model, s.mode.is_cloud, &hash(&user), None, &serde_json::to_value(&ex).map_err(err)?)
        .await
        .map_err(err)?;
    repo::supersede_pending(pool, profile_id).await.map_err(err)?;
    for sug in &ex.suggestions {
        let payload = serde_json::to_value(&sug.value).map_err(err)?;
        repo::insert_suggestion(pool, profile_id, run_id, "field", &sug.field, &payload, &sug.evidence, sug.confidence, sug.conflict, &s.model).await.map_err(err)?;
    }
    for ps in &ex.preferences {
        let payload = serde_json::to_value(&ps.preference).map_err(err)?;
        repo::insert_suggestion(pool, profile_id, run_id, "preference", &ps.preference.condition.field, &payload, &ps.evidence, 0.5, false, &s.model).await.map_err(err)?;
    }
    MmRepository::audit(pool, "ai_extract", "profile", profile_id, Some(if s.mode.is_cloud { "cloud" } else { "local" })).await;
    let rows = repo::suggestions_for_run(pool, run_id).await.map_err(err)?;
    Ok(ExtractionView {
        run_id,
        model: s.model.clone(),
        is_cloud: s.mode.is_cloud,
        created_at: chrono::Utc::now().to_rfc3339(),
        suggestions: rows,
        contradictions: ex.contradictions,
        missing: ex.missing,
        questions: ex.questions,
        dropped: ex.dropped,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Store the value as AI-inferred: any person's entry outranks it, and the UI keeps flagging it for verification.
    Accept,
    /// The matchmaker checked it and takes responsibility: stored as matchmaker-entered, may replace a person's value.
    AcceptVerified,
    Reject,
}

pub async fn decide_suggestion(pool: &SqlitePool, id: i64, decision: Decision) -> Result<(), String> {
    let sug = repo::get_suggestion(pool, id).await.map_err(err)?.ok_or("Suggestion not found")?;
    if sug.status != "pending" {
        return Err("This suggestion has already been handled or replaced".into());
    }
    if decision == Decision::Reject {
        repo::set_suggestion_status(pool, id, "rejected").await.map_err(err)?;
        MmRepository::audit(pool, "ai_suggestion_rejected", "profile", &sug.profile_id, Some(&sug.field)).await;
        return Ok(());
    }
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    match sug.kind.as_str() {
        "field" => {
            let value: Value = serde_json::from_value(sug.payload.clone()).map_err(err)?;
            reg.validate_value(&sug.field, &value)?; // the registry may have changed since the suggestion was made
            let mut p = MmRepository::get_profile(pool, &sug.profile_id).await.map_err(err)?.ok_or("Profile not found")?;
            let source = if decision == Decision::AcceptVerified { Provenance::Matchmaker } else { Provenance::AiInferred };
            if !p.profile.set(&sug.field, value, source) {
                return Err("A person already entered a different value here. Use “Accept as verified” if you have checked and want to replace it.".into());
            }
            MmRepository::update_profile_data(pool, &p.profile).await.map_err(err)?;
        }
        "preference" => {
            let mut pref: matchmaking_core::Preference = serde_json::from_value(sug.payload.clone()).map_err(err)?;
            pref.id = uuid::Uuid::new_v4().to_string();
            validate_preference(&pref, &reg)?;
            let mut prefs = MmRepository::get_preferences(pool, &sug.profile_id).await.map_err(err)?;
            if prefs.len() >= 100 {
                return Err("This profile already has the maximum number of preferences".into());
            }
            prefs.push(pref);
            MmRepository::save_preferences(pool, &sug.profile_id, &prefs).await.map_err(err)?;
        }
        other => return Err(format!("Unknown suggestion kind '{other}'")),
    }
    repo::set_suggestion_status(pool, id, "accepted").await.map_err(err)?;
    // Which field, never the value or the quoted evidence.
    MmRepository::audit(pool, if decision == Decision::AcceptVerified { "ai_suggestion_verified" } else { "ai_suggestion_accepted" }, "profile", &sug.profile_id, Some(&sug.field)).await;
    Ok(())
}

// ------------------------------------------------------------------ pair analysis

#[derive(Debug, Serialize)]
pub struct PairView {
    pub run_id: i64,
    pub model: String,
    pub is_cloud: bool,
    pub created_at: String,
    /// The inputs changed since this analysis was written (profiles, preferences, rules or weights).
    pub stale: bool,
    pub facts: Vec<AiFact>,
    pub analysis: PairAnalysis,
}

struct PairInput {
    facts: Vec<AiFact>,
    system: String,
    user: String,
    rule_set_version: u32,
    a: String,
    b: String,
}

async fn pair_input(pool: &SqlitePool, match_id: &str, include_sensitive: bool) -> Result<PairInput, String> {
    let m = MatchRepo::get(pool, match_id).await.map_err(err)?.ok_or("Match not found")?;
    let (_, set, _) = MmRepository::get_rule_set(pool, &m.rule_set_id, Some(m.rule_set_version)).await.map_err(err)?.ok_or("Rule set version not found")?;
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let a = MmRepository::get_profile(pool, &m.profile_a).await.map_err(err)?.ok_or("Profile not found")?;
    let b = MmRepository::get_profile(pool, &m.profile_b).await.map_err(err)?.ok_or("Profile not found")?;
    let a_prefs = MmRepository::get_preferences(pool, &m.profile_a).await.map_err(err)?;
    let b_prefs = MmRepository::get_preferences(pool, &m.profile_b).await.map_err(err)?;
    let card = compute_scorecard(pool, &set, &m.profile_a, &m.profile_b, &m.weight_overrides).await?;
    let facts = build_pair_facts(&a.profile, &b.profile, &a_prefs, &b_prefs, &card, &reg, include_sensitive);
    let (system, user) = pair_prompt(&facts);
    Ok(PairInput { facts, system, user, rule_set_version: m.rule_set_version, a: m.profile_a, b: m.profile_b })
}

pub async fn preview_pair(pool: &SqlitePool, match_id: &str, mode: Mode) -> Result<(String, String), String> {
    let i = pair_input(pool, match_id, mode.include_sensitive).await?;
    Ok((i.system, i.user))
}

pub async fn analyse_pair(pool: &SqlitePool, s: &Session<'_>, match_id: &str) -> Result<PairView, String> {
    let i = pair_input(pool, match_id, s.mode.include_sensitive).await?;
    let facts = i.facts.clone();
    let analysis: PairAnalysis = ask(pool, s, "pair", (Some(&i.a), Some(&i.b)), &i.system, &i.user, |raw| parse_pair_analysis(raw, &facts)).await?;
    let result = json!({"facts": i.facts, "analysis": analysis, "include_sensitive": s.mode.include_sensitive});
    let run_id = repo::insert_run(pool, None, Some(match_id), "pair", &s.provider, &s.model, s.mode.is_cloud, &hash(&i.user), Some(i.rule_set_version), &result).await.map_err(err)?;
    MmRepository::audit(pool, "ai_pair_analysis", "match", match_id, Some(if s.mode.is_cloud { "cloud" } else { "local" })).await;
    Ok(PairView { run_id, model: s.model.clone(), is_cloud: s.mode.is_cloud, created_at: chrono::Utc::now().to_rfc3339(), stale: false, facts: i.facts, analysis })
}

/// The latest stored analysis, with a note if what it was based on has changed since.
pub async fn latest_pair_analysis(pool: &SqlitePool, match_id: &str) -> Result<Option<PairView>, String> {
    let Some(run) = repo::latest_match_run(pool, match_id).await.map_err(err)? else { return Ok(None) };
    let include_sensitive = run.result.get("include_sensitive").and_then(|v| v.as_bool()).unwrap_or(false);
    let facts: Vec<AiFact> = serde_json::from_value(run.result.get("facts").cloned().unwrap_or(Json::Null)).map_err(err)?;
    let analysis: PairAnalysis = serde_json::from_value(run.result.get("analysis").cloned().unwrap_or(Json::Null)).map_err(err)?;
    let current = pair_input(pool, match_id, include_sensitive).await?;
    Ok(Some(PairView { run_id: run.id, model: run.model, is_cloud: run.is_cloud, created_at: run.created_at, stale: hash(&current.user) != run.input_hash, facts, analysis }))
}
