//! Orchestration: gate -> redacted prompt -> model -> validation -> stored suggestions or analysis.
//! Suggestions are applied only by a matchmaker or a policy-checked agent tool.

use super::backend::LlmBackend;
use super::repo::{self, SuggestionRow};
use super::settings::Mode;
use crate::mm::intake_repo::IntakeRepo;
use crate::mm::match_commands::compute_scorecard;
use crate::mm::match_repo::MatchRepo;
use crate::mm::repository::MmRepository;
use matchmaking_core::{
    build_pair_facts, extract_with_rules, output_constraint, pair_prompt_for, parse_pair_analysis, parse_slim_extraction, profile_text_for_ai,
    slim_extraction_prompt_for, validate_preference, AiFact, ExtractionConstraint, FieldKind, FieldRegistry, Locale, PairAnalysis, Profile, Provenance,
    Value, NEVER_SHARED,
};
use super::models::TaskProfile;
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
pub(crate) fn lock() -> &'static tokio::sync::Mutex<()> {
    static L: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    L.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub struct Session<'a> {
    pub backend: &'a dyn LlmBackend,
    pub provider: String,
    pub model: String,
    pub mode: Mode,
    pub locale: Locale,
    pub agent_config: Option<super::settings::AiConfig>,
}

/// Ask the model, check the answer with `parse`, and on unreadable output ask once more. Every attempt is logged.
async fn ask<T>(
    pool: &SqlitePool,
    s: &Session<'_>,
    kind: &str,
    ids: (Option<&str>, Option<&str>),
    system: &str,
    user: &str,
    actor: matchmaking_core::autonomy::Actor,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<T, String> {
    ask_with(pool, s, kind, ids, system, user, TaskProfile::for_kind(kind), None, actor, parse).await
}

/// [`ask`] with an explicit task profile and, optionally, an output constraint. When the backend enforces the
/// constraint a second attempt cannot produce a different kind of answer, so it is skipped.
#[allow(clippy::too_many_arguments)]
async fn ask_with<T>(
    pool: &SqlitePool,
    s: &Session<'_>,
    kind: &str,
    ids: (Option<&str>, Option<&str>),
    system: &str,
    user: &str,
    task: TaskProfile,
    constraint: Option<&ExtractionConstraint>,
    actor: matchmaking_core::autonomy::Actor,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = lock().lock().await;
    let _ = repo::purge_old_prompts(pool, 30).await;
    let mut prompt = user.to_string();
    let mut last_err = String::new();
    let attempts = if constraint.is_some() && s.backend.enforces_constraint() { 1 } else { 2 };
    for attempt in 0..attempts {
        if s.agent_config.is_some() {
            crate::agent::ensure_open(pool, s).await?;
        }
        if let matchmaking_core::autonomy::Actor::Agent { run_id } = actor {
            crate::agent::budgets::reserve_model_call(pool, run_id, kind).await?;
        }
        let result = match constraint {
            Some(constraint) => s.backend.complete_structured(task, system, &prompt, constraint).await,
            None => s.backend.complete_for(task, system, &prompt, true).await,
        };
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
    Err(match s.locale {
        Locale::En => format!("The model's answer could not be used: {last_err}"),
        Locale::Fa => format!("پاسخ مدل قابل استفاده نبود: {last_err}"),
    })
}

// ------------------------------------------------------------------ profile understanding

/// The free text to read a profile with: what the caller typed now, or — when they typed nothing — the raw entry
/// the profile was created from by a bulk intake run. Both travel the same way: as the `extra` argument, so the
/// gate's `include_sensitive` rule decides locally-versus-cloud redaction in one place and the model-side code
/// never learns where the text came from.
async fn profile_extra<'a>(pool: &SqlitePool, profile_id: &str, extra: Option<&'a str>) -> Result<Option<String>, String> {
    match extra {
        Some(text) if !text.trim().is_empty() => Ok(Some(text.to_string())),
        _ => IntakeRepo::note_for_profile(pool, profile_id).await.map_err(err),
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Timings {
    pub rules_ms: u64,
    pub llm_ms: u64,
}

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
    /// "ok" | "skipped_nothing_left" | "unavailable" | "failed"
    pub llm_status: String,
    pub llm_error: Option<String>,
    pub rule_count: usize,
    pub ai_count: usize,
    pub timings: Timings,
}

const NO_TEXT_EN: &str = "There is no free text to read. Add notes about the person (for example from an intake interview) first.";
const NO_TEXT_FA: &str = "متنی برای خواندن وجود ندارد. ابتدا یادداشت‌هایی دربارهٔ فرد بنویسید (مثلاً از مصاحبهٔ ورودی).";

/// Fields extraction may read at all. Names are read by the local rules; they never leave the computer.
fn eligible(registry: &FieldRegistry, key: &str) -> bool {
    matchmaking_core::extract_rules::rules_may_read(key) && registry.get(key).is_some_and(|def| !matches!(def.kind, FieldKind::Records(_)))
}

/// Fields the model may be asked for. It never sees a name, so asking for one only invites an invention.
fn llm_eligible(registry: &FieldRegistry, key: &str) -> bool {
    eligible(registry, key) && !NEVER_SHARED.contains(&key)
}

/// Which fields to read: the explicit list, else the default preset, else every eligible field. Registry order.
pub async fn resolve_targets(pool: &SqlitePool, registry: &FieldRegistry, explicit: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let all: Vec<String> = registry.defs().map(|def| def.key.clone()).collect();
    let chosen = match explicit {
        Some(list) => list,
        None => match repo::default_extraction_preset(pool, &all).await.map_err(err)? {
            Some(preset) if !preset.fields.is_empty() => preset.fields,
            _ => all.clone(),
        },
    };
    Ok(all.into_iter().filter(|key| chosen.contains(key) && eligible(registry, key)).collect())
}

/// What the rules read: the profile's text fields and the intake text exactly as typed. Nothing leaves the
/// computer here, so no redaction; only the contact details the rules never read are left out.
fn raw_text_for_rules(profile: &Profile, registry: &FieldRegistry, extra: Option<&str>) -> String {
    let mut parts = vec![];
    for def in registry.defs() {
        if !matchmaking_core::extract_rules::rules_may_read(&def.key) || !matches!(def.kind, FieldKind::Text) {
            continue;
        }
        if let Some(Value::Text(text)) = profile.get(&def.key) {
            if !text.trim().is_empty() {
                parts.push(format!("{}: {}", def.label, text.trim()));
            }
        }
    }
    if let Some(text) = extra.map(str::trim).filter(|text| !text.is_empty()) {
        parts.push(text.to_string());
    }
    parts.join("\n")
}

/// The fields only a model can still fill: no rule hit, no stored value, plus the ones the rules could not decide.
fn llm_targets(registry: &FieldRegistry, profile: &Profile, targets: &[String], hit: &[String], ambiguous: &std::collections::BTreeSet<String>, include_sensitive: bool) -> Vec<String> {
    targets
        .iter()
        .filter(|key| {
            let Some(def) = registry.get(key) else { return false };
            llm_eligible(registry, key)
                && (include_sensitive || !def.sensitive)
                && (ambiguous.contains(*key) || (!hit.contains(key) && profile.get(key).is_none()))
        })
        .cloned()
        .collect()
}

async fn load_for_extraction(pool: &SqlitePool, profile_id: &str, extra: Option<&str>, locale: Locale) -> Result<(FieldRegistry, Profile, Option<String>), String> {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let p = MmRepository::get_profile(pool, profile_id).await.map_err(err)?.ok_or_else(|| locale.pick("Profile not found", "پروفایل پیدا نشد").to_string())?;
    let extra = profile_extra(pool, profile_id, extra).await?;
    Ok((reg, p.profile, extra))
}

/// The exact prompt that would be sent, for the matchmaker to read first. `None` when the rules leave the model
/// nothing to read, so nothing would be sent.
pub async fn preview_extraction(
    pool: &SqlitePool,
    profile_id: &str,
    extra: Option<&str>,
    mode: Mode,
    locale: Locale,
    targets: Option<Vec<String>>,
    with_preferences: bool,
) -> Result<Option<(String, String)>, String> {
    let (reg, profile, extra) = load_for_extraction(pool, profile_id, extra, locale).await?;
    let raw = raw_text_for_rules(&profile, &reg, extra.as_deref());
    if raw.trim().is_empty() {
        return Err(locale.pick(NO_TEXT_EN, NO_TEXT_FA).to_string());
    }
    let targets = resolve_targets(pool, &reg, targets).await?;
    let rules = extract_with_rules(&reg, &raw, Some(&targets), &profile);
    let hit: Vec<String> = rules.suggestions.iter().map(|sug| sug.field.clone()).collect();
    let fields = llm_targets(&reg, &profile, &targets, &hit, &rules.ambiguous, mode.include_sensitive);
    let text = profile_text_for_ai(&profile, &reg, extra.as_deref(), mode.include_sensitive);
    if fields.is_empty() || text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(slim_extraction_prompt_for(&reg, &text, &fields, locale, with_preferences)))
}

/// Two tiers: the local rules always run and their suggestions are stored first; the model, when there is a
/// session, only reads the selected fields the rules left open. A model failure keeps the rule results.
pub async fn extract_profile(
    pool: &SqlitePool,
    session: Option<&Session<'_>>,
    profile_id: &str,
    extra: Option<&str>,
    targets: Option<Vec<String>>,
    with_preferences: bool,
) -> Result<ExtractionView, String> {
    extract_profile_as(pool, session, profile_id, extra, targets, with_preferences, matchmaking_core::autonomy::Actor::Matchmaker).await
}

pub async fn extract_profile_as(
    pool: &SqlitePool,
    session: Option<&Session<'_>>,
    profile_id: &str,
    extra: Option<&str>,
    targets: Option<Vec<String>>,
    with_preferences: bool,
    actor: matchmaking_core::autonomy::Actor,
) -> Result<ExtractionView, String> {
    let locale = session.map_or(Locale::En, |s| s.locale);
    let (reg, profile, extra) = load_for_extraction(pool, profile_id, extra, locale).await?;
    let raw = raw_text_for_rules(&profile, &reg, extra.as_deref());
    if raw.trim().is_empty() {
        return Err(locale.pick(NO_TEXT_EN, NO_TEXT_FA).to_string());
    }
    let targets = resolve_targets(pool, &reg, targets).await?;

    let rules_started = std::time::Instant::now();
    let rules = extract_with_rules(&reg, &raw, Some(&targets), &profile);
    let rules_ms = rules_started.elapsed().as_millis() as u64;

    let (provider, model, is_cloud) = session.map_or(("rules".to_string(), "rules".to_string(), false), |s| (s.provider.clone(), s.model.clone(), s.mode.is_cloud));
    let run_id = repo::insert_run(pool, Some(profile_id), None, "extract", &provider, &model, is_cloud, &hash(&raw), None, &json!({"rules": rules.suggestions, "ambiguous": rules.ambiguous}))
        .await
        .map_err(err)?;
    repo::supersede_pending(pool, profile_id).await.map_err(err)?;
    for sug in &rules.suggestions {
        let payload = serde_json::to_value(&sug.value).map_err(err)?;
        repo::insert_suggestion(pool, profile_id, run_id, "field", &sug.field, &payload, &sug.evidence, sug.confidence, sug.conflict, "rules", "rule").await.map_err(err)?;
    }

    let mut view = ExtractionView {
        run_id,
        model: model.clone(),
        is_cloud,
        created_at: chrono::Utc::now().to_rfc3339(),
        suggestions: vec![],
        contradictions: vec![],
        missing: vec![],
        questions: vec![],
        dropped: vec![],
        llm_status: "unavailable".into(),
        llm_error: None,
        rule_count: rules.suggestions.len(),
        ai_count: 0,
        timings: Timings { rules_ms, llm_ms: 0 },
    };

    if let Some(s) = session {
        let hit: Vec<String> = rules.suggestions.iter().map(|sug| sug.field.clone()).collect();
        let fields = llm_targets(&reg, &profile, &targets, &hit, &rules.ambiguous, s.mode.include_sensitive);
        let text = profile_text_for_ai(&profile, &reg, extra.as_deref(), s.mode.include_sensitive);
        if fields.is_empty() || text.trim().is_empty() {
            view.llm_status = "skipped_nothing_left".into();
        } else {
            let (system, user) = slim_extraction_prompt_for(&reg, &text, &fields, s.locale, with_preferences);
            let constraint = output_constraint(&reg, &fields, with_preferences);
            let task = TaskProfile::SlimExtraction { fields: fields.len().min(u16::MAX as usize) as u16, with_preferences };
            let llm_started = std::time::Instant::now();
            let outcome = ask_with(pool, s, "extract", (Some(profile_id), None), &system, &user, task, Some(&constraint), actor, |raw_reply| {
                parse_slim_extraction(raw_reply, &reg, &text, &profile, s.mode.include_sensitive, &fields)
            })
            .await;
            if matches!(actor, matchmaking_core::autonomy::Actor::Agent { .. }) {
                crate::agent::ensure_open(pool, s).await?;
            }
            view.timings.llm_ms = llm_started.elapsed().as_millis() as u64;
            match outcome {
                Ok(ex) => {
                    for sug in &ex.suggestions {
                        let payload = serde_json::to_value(&sug.value).map_err(err)?;
                        repo::insert_suggestion(pool, profile_id, run_id, "field", &sug.field, &payload, &sug.evidence, sug.confidence, sug.conflict, &s.model, "ai").await.map_err(err)?;
                    }
                    view.ai_count = ex.suggestions.len();
                    if with_preferences {
                        for ps in &ex.preferences {
                            let payload = serde_json::to_value(&ps.preference).map_err(err)?;
                            repo::insert_suggestion(pool, profile_id, run_id, "preference", &ps.preference.condition.field, &payload, &ps.evidence, 0.5, false, &s.model, "ai").await.map_err(err)?;
                        }
                        view.contradictions = ex.contradictions;
                        view.missing = ex.missing;
                        view.questions = ex.questions;
                    }
                    view.dropped = ex.dropped;
                    view.llm_status = "ok".into();
                }
                Err(e) => {
                    if actor.is_agent() && e == crate::agent::budgets::MODEL_CALL_LIMIT { return Err(e); }
                    view.llm_status = "failed".into();
                    view.llm_error = Some(e);
                }
            }
        }
    }

    MmRepository::audit_as(pool, actor, "ai_extract", "profile", profile_id, Some(if session.is_none() { "rules" } else if is_cloud { "cloud" } else { "local" })).await;
    if !matches!(actor, matchmaking_core::autonomy::Actor::Agent { .. }) {
        crate::mm::autonomy::settle_extraction(pool, profile_id, run_id, &rules.ambiguous).await?;
    }
    view.suggestions = repo::suggestions_for_run(pool, run_id).await.map_err(err)?;
    Ok(view)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Store the value as AI-inferred: any person's entry outranks it, and the UI keeps flagging it for verification.
    Accept,
    /// The matchmaker checked it and takes responsibility: stored as matchmaker-entered, may replace a person's value.
    AcceptVerified,
    Reject,
}

pub async fn decide_suggestion(pool: &SqlitePool, id: i64, decision: Decision, locale: Locale) -> Result<(), String> {
    decide_suggestion_as(pool, matchmaking_core::autonomy::Actor::Matchmaker, id, decision, locale).await
}

/// [`decide_suggestion`] on behalf of `actor`, which the audit row records.
pub async fn decide_suggestion_as(pool: &SqlitePool, actor: matchmaking_core::autonomy::Actor, id: i64, decision: Decision, locale: Locale) -> Result<(), String> {
    let sug = repo::get_suggestion(pool, id).await.map_err(err)?.ok_or_else(|| locale.pick("Suggestion not found", "پیشنهاد پیدا نشد").to_string())?;
    if sug.status != "pending" {
        return Err(locale.pick("This suggestion has already been handled or replaced", "این پیشنهاد پیش‌تر بررسی یا جایگزین شده است").to_string());
    }
    if decision == Decision::Reject {
        repo::set_suggestion_status(pool, id, "rejected").await.map_err(err)?;
        MmRepository::audit_as(pool, actor, "ai_suggestion_rejected", "profile", &sug.profile_id, Some(&sug.field)).await;
        return Ok(());
    }
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    match sug.kind.as_str() {
        "field" => {
            let value: Value = serde_json::from_value(sug.payload.clone()).map_err(err)?;
            reg.validate_value(&sug.field, &value)?; // the registry may have changed since the suggestion was made
            let mut p = MmRepository::get_profile(pool, &sug.profile_id).await.map_err(err)?.ok_or_else(|| locale.pick("Profile not found", "پروفایل پیدا نشد").to_string())?;
            let source = match (decision, sug.source.as_str()) {
                (Decision::AcceptVerified, _) => Provenance::Matchmaker,
                (_, "rule") => Provenance::RuleExtracted,
                _ => Provenance::AiInferred,
            };
            if !p.profile.set(&sug.field, value, source) {
                return Err(locale.pick("A person already entered a different value here. Use “Accept as verified” if you have checked and want to replace it.", "فرد پیش‌تر در اینجا مقدار دیگری وارد کرده است. اگر بررسی کرده‌اید و می‌خواهید جایگزین شود، «پذیرش به‌عنوان تأییدشده» را بزنید.").to_string());
            }
            MmRepository::update_profile_data(pool, &p.profile).await.map_err(err)?;
        }
        "preference" => {
            let mut pref: matchmaking_core::Preference = serde_json::from_value(sug.payload.clone()).map_err(err)?;
            pref.id = uuid::Uuid::new_v4().to_string();
            validate_preference(&pref, &reg)?;
            let mut prefs = MmRepository::get_preferences(pool, &sug.profile_id).await.map_err(err)?;
            if prefs.len() >= 100 {
                return Err(locale.pick("This profile already has the maximum number of preferences", "این پروفایل به حداکثر تعداد ترجیحات رسیده است").to_string());
            }
            prefs.push(pref);
            MmRepository::save_preferences(pool, &sug.profile_id, &prefs).await.map_err(err)?;
        }
        other => return Err(match locale {
            Locale::En => format!("Unknown suggestion kind '{other}'"),
            Locale::Fa => format!("نوع پیشنهاد ناشناخته است: «{other}»"),
        }),
    }
    repo::set_suggestion_status(pool, id, "accepted").await.map_err(err)?;
    // Which field, never the value or the quoted evidence.
    MmRepository::audit_as(pool, actor, if decision == Decision::AcceptVerified { "ai_suggestion_verified" } else { "ai_suggestion_accepted" }, "profile", &sug.profile_id, Some(&sug.field)).await;
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
    input_hash: String,
    rule_set_version: u32,
    a: String,
    b: String,
}

async fn pair_input(pool: &SqlitePool, match_id: &str, include_sensitive: bool, locale: Locale) -> Result<PairInput, String> {
    let m = MatchRepo::get(pool, match_id).await.map_err(err)?.ok_or_else(|| locale.pick("Match not found", "تطبیق پیدا نشد").to_string())?;
    let (_, set, _) = MmRepository::get_rule_set(pool, &m.rule_set_id, Some(m.rule_set_version)).await.map_err(err)?.ok_or_else(|| locale.pick("Rule set version not found", "نسخهٔ مجموعه‌قواعد پیدا نشد").to_string())?;
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let a = MmRepository::get_profile(pool, &m.profile_a).await.map_err(err)?.ok_or_else(|| locale.pick("Profile not found", "پروفایل پیدا نشد").to_string())?;
    let b = MmRepository::get_profile(pool, &m.profile_b).await.map_err(err)?.ok_or_else(|| locale.pick("Profile not found", "پروفایل پیدا نشد").to_string())?;
    let a_prefs = MmRepository::get_preferences(pool, &m.profile_a).await.map_err(err)?;
    let b_prefs = MmRepository::get_preferences(pool, &m.profile_b).await.map_err(err)?;
    // The advisory pair analysis reads a scorecard, so it reads it under the same numbers the match was judged by.
    let config = crate::mm::scoring_config_repo::ScoringConfigRepo::config_for_rule_set(pool, &m.rule_set_id).await.map_err(err)?;
    let card = compute_scorecard(pool, &set, &config, &m.profile_a, &m.profile_b, &m.weight_overrides).await?;
    let facts = build_pair_facts(&a.profile, &b.profile, &a_prefs, &b_prefs, &card, &reg, include_sensitive);
    let (system, user) = pair_prompt_for(&facts, locale);
    let input_hash = hash(&pair_prompt_for(&facts, Locale::En).1);
    Ok(PairInput { facts, system, user, input_hash, rule_set_version: m.rule_set_version, a: m.profile_a, b: m.profile_b })
}

pub async fn preview_pair(pool: &SqlitePool, match_id: &str, mode: Mode, locale: Locale) -> Result<(String, String), String> {
    let i = pair_input(pool, match_id, mode.include_sensitive, locale).await?;
    Ok((i.system, i.user))
}

pub async fn analyse_pair(pool: &SqlitePool, s: &Session<'_>, match_id: &str) -> Result<PairView, String> {
    analyse_pair_as(pool, s, match_id, matchmaking_core::autonomy::Actor::Matchmaker).await
}

pub async fn analyse_pair_as(pool: &SqlitePool, s: &Session<'_>, match_id: &str, actor: matchmaking_core::autonomy::Actor) -> Result<PairView, String> {
    let i = pair_input(pool, match_id, s.mode.include_sensitive, s.locale).await?;
    let facts = i.facts.clone();
    let analysis: PairAnalysis = ask(pool, s, "pair", (Some(&i.a), Some(&i.b)), &i.system, &i.user, actor, |raw| parse_pair_analysis(raw, &facts)).await?;
    if matches!(actor, matchmaking_core::autonomy::Actor::Agent { .. }) {
        crate::agent::ensure_open(pool, s).await?;
    }
    let result = json!({"facts": i.facts, "analysis": analysis, "include_sensitive": s.mode.include_sensitive});
    let run_id = repo::insert_run(pool, None, Some(match_id), "pair", &s.provider, &s.model, s.mode.is_cloud, &i.input_hash, Some(i.rule_set_version), &result).await.map_err(err)?;
    MmRepository::audit_as(pool, actor, "ai_pair_analysis", "match", match_id, Some(if s.mode.is_cloud { "cloud" } else { "local" })).await;
    Ok(PairView { run_id, model: s.model.clone(), is_cloud: s.mode.is_cloud, created_at: chrono::Utc::now().to_rfc3339(), stale: false, facts: i.facts, analysis })
}

/// The latest stored analysis, with a note if what it was based on has changed since.
pub async fn latest_pair_analysis(pool: &SqlitePool, match_id: &str) -> Result<Option<PairView>, String> {
    let Some(run) = repo::latest_match_run(pool, match_id).await.map_err(err)? else { return Ok(None) };
    let include_sensitive = run.result.get("include_sensitive").and_then(|v| v.as_bool()).unwrap_or(false);
    let facts: Vec<AiFact> = serde_json::from_value(run.result.get("facts").cloned().unwrap_or(Json::Null)).map_err(err)?;
    let analysis: PairAnalysis = serde_json::from_value(run.result.get("analysis").cloned().unwrap_or(Json::Null)).map_err(err)?;
    let current = pair_input(pool, match_id, include_sensitive, Locale::En).await?;
    Ok(Some(PairView { run_id: run.id, model: run.model, is_cloud: run.is_cloud, created_at: run.created_at, stale: current.input_hash != run.input_hash, facts, analysis }))
}
