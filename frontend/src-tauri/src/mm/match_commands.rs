use super::match_repo::{MatchRepo, MatchRow};
use super::repository::MmRepository;
use crate::state::AppState;
use matchmaking_core::{
    apply_responses, can_record_outcome, can_record_responses, score_pair, validate_text, validate_transition, Interest, MatchStatus,
    Outcome, RuleSet, TransitionContext, TransitionRequest,
};
use serde::Serialize;
use serde_json::Value as Json;
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn profile_name(sp: &super::repository::StoredProfile) -> Option<String> {
    match sp.profile.get("full_name") {
        Some(matchmaking_core::Value::Text(t)) if !t.trim().is_empty() => Some(t.clone()),
        _ => None,
    }
}

async fn name_of(pool: &SqlitePool, id: &str) -> Result<Option<String>, String> {
    Ok(MmRepository::get_profile(pool, id).await.map_err(err)?.and_then(|sp| profile_name(&sp)))
}

#[derive(Serialize)]
pub struct NoteView {
    pub id: i64,
    pub text: String,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct EventView {
    pub at: String,
    pub actor: String,
    pub kind: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub detail: Option<String>,
}

#[derive(Serialize)]
pub struct MatchDetail {
    pub id: String,
    pub profile_a: String,
    pub profile_b: String,
    pub name_a: Option<String>,
    pub name_b: Option<String>,
    pub source_profile: Option<String>,
    pub rule_set_id: String,
    pub rule_set_name: Option<String>,
    pub rule_set_version: u32,
    pub status: MatchStatus,
    /// Moves the state machine allows from here (the UI offers exactly these).
    pub allowed_next: Vec<MatchStatus>,
    /// The latest snapshot says the rules allow this pair; if false, recommending or approving needs an override reason.
    pub eligible: bool,
    pub can_record_responses: bool,
    pub can_record_outcome: bool,
    pub hold_reason: Option<String>,
    pub a_response: Interest,
    pub b_response: Interest,
    pub outcome: Option<Outcome>,
    pub override_reason: Option<String>,
    pub weight_overrides: BTreeMap<String, f64>,
    pub hidden: bool,
    pub created_at: String,
    pub updated_at: String,
    /// The latest stored scorecard (see `ScoreCard`).
    pub scorecard: Option<Json>,
    pub scorecard_at: Option<String>,
    pub scorecard_trigger: Option<String>,
    pub snapshot_count: i64,
    pub notes: Vec<NoteView>,
    pub events: Vec<EventView>,
}

fn eligible_of(card: &Option<Json>) -> bool {
    card.as_ref().and_then(|c| c.get("eligible")).and_then(|e| e.as_bool()).unwrap_or(false)
}

async fn detail(pool: &SqlitePool, id: &str) -> Result<MatchDetail, String> {
    let m = MatchRepo::get(pool, id).await.map_err(err)?.ok_or("Match not found")?;
    let snap = MatchRepo::latest_snapshot(pool, id).await.map_err(err)?;
    let rule_set_name = MmRepository::get_rule_set_name(pool, &m.rule_set_id).await.map_err(err)?;
    let notes = MatchRepo::notes(pool, id).await.map_err(err)?;
    let events = MatchRepo::events(pool, id).await.map_err(err)?;
    Ok(MatchDetail {
        name_a: name_of(pool, &m.profile_a).await?,
        name_b: name_of(pool, &m.profile_b).await?,
        allowed_next: m.status.allowed_next(),
        can_record_responses: can_record_responses(m.status),
        can_record_outcome: can_record_outcome(m.status),
        eligible: eligible_of(&snap.as_ref().map(|s| s.scorecard.clone())),
        snapshot_count: MatchRepo::snapshot_count(pool, id).await.map_err(err)?,
        scorecard_at: snap.as_ref().map(|s| s.created_at.clone()),
        scorecard_trigger: snap.as_ref().map(|s| s.trigger.clone()),
        scorecard: snap.map(|s| s.scorecard),
        notes: notes.into_iter().map(|n| NoteView { id: n.id, text: n.text, created_at: n.created_at }).collect(),
        events: events
            .into_iter()
            .map(|e| EventView { at: e.at, actor: e.actor, kind: e.kind, from_status: e.from_status, to_status: e.to_status, detail: e.detail })
            .collect(),
        rule_set_name,
        id: m.id,
        profile_a: m.profile_a,
        profile_b: m.profile_b,
        source_profile: m.source_profile,
        rule_set_id: m.rule_set_id,
        rule_set_version: m.rule_set_version,
        status: m.status,
        hold_reason: m.hold_reason,
        a_response: m.a_response,
        b_response: m.b_response,
        outcome: m.outcome,
        override_reason: m.override_reason,
        weight_overrides: m.weight_overrides,
        hidden: m.hidden,
        created_at: m.created_at,
        updated_at: m.updated_at,
    })
}

fn check_weights(w: &BTreeMap<String, f64>) -> Result<(), String> {
    if w.len() > 30 {
        return Err("Too many weight adjustments".into());
    }
    for (k, v) in w {
        if k.trim().is_empty() || !v.is_finite() || !(0.0..=10.0).contains(v) {
            return Err(format!("The weight for '{k}' must be between 0 and 10"));
        }
    }
    Ok(())
}

/// Score a pair with per-match weight adjustments applied on top of the rule set.
async fn scorecard_json(
    pool: &SqlitePool,
    set: &RuleSet,
    a_id: &str,
    b_id: &str,
    weights: &BTreeMap<String, f64>,
) -> Result<Json, String> {
    let a = MmRepository::get_profile(pool, a_id).await.map_err(err)?.ok_or("First profile not found")?;
    let b = MmRepository::get_profile(pool, b_id).await.map_err(err)?.ok_or("Second profile not found")?;
    let a_prefs = MmRepository::get_preferences(pool, a_id).await.map_err(err)?;
    let b_prefs = MmRepository::get_preferences(pool, b_id).await.map_err(err)?;
    let mut set = set.clone();
    for (k, v) in weights {
        set.group_weights.insert(k.clone(), *v);
    }
    let card = score_pair(&set, &a.profile, &b.profile, &a_prefs, &b_prefs)?;
    serde_json::to_value(card).map_err(err)
}

async fn require_match(pool: &SqlitePool, id: &str) -> Result<MatchRow, String> {
    MatchRepo::get(pool, id).await.map_err(err)?.ok_or_else(|| "Match not found".to_string())
}

/// Find the record for a pair of people, if one exists.
#[tauri::command]
pub async fn mm_find_match_record(state: State<'_, AppState>, profile_a: String, profile_b: String) -> Result<Option<String>, String> {
    Ok(MatchRepo::find_by_pair(state.db_manager.pool(), &profile_a, &profile_b).await.map_err(err)?.map(|m| m.id))
}

/// Start tracking a pair. The pair is scored now and that score is kept as the first snapshot (the record of why
/// it was recommended). Eligible pairs start as `recommended`; pairs the rules exclude start as `identified`.
/// If the pair is already tracked, the existing record is returned.
#[tauri::command]
pub async fn mm_create_match(
    state: State<'_, AppState>,
    profile_a: String,
    profile_b: String,
    rule_set_id: String,
    source_profile: Option<String>,
) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    if profile_a == profile_b {
        return Err("A match needs two different people".into());
    }
    if let Some(existing) = MatchRepo::find_by_pair(pool, &profile_a, &profile_b).await.map_err(err)? {
        return detail(pool, &existing.id).await;
    }
    for id in [&profile_a, &profile_b] {
        let p = MmRepository::get_profile(pool, id).await.map_err(err)?.ok_or("Profile not found")?;
        if p.status != "active" {
            return Err("Deactivated profiles cannot be matched".into());
        }
    }
    let (_, set, _) = MmRepository::get_rule_set(pool, &rule_set_id, None).await.map_err(err)?.ok_or("Rule set not found")?;
    let (a, b) = super::match_repo::pair_key(&profile_a, &profile_b);
    let card = scorecard_json(pool, &set, &a, &b, &BTreeMap::new()).await?;
    let status = if eligible_of(&Some(card.clone())) { MatchStatus::Recommended } else { MatchStatus::Identified };
    let id = MatchRepo::create(pool, &a, &b, source_profile.as_deref(), &rule_set_id, set.version, status, &card).await.map_err(err)?;
    MmRepository::audit(pool, "create_match", "match", &id, Some(status.as_str())).await;
    detail(pool, &id).await
}

#[derive(Serialize)]
pub struct MatchSummary {
    pub id: String,
    pub name_a: Option<String>,
    pub name_b: Option<String>,
    pub status: MatchStatus,
    pub on_hold: bool,
    pub hidden: bool,
    pub eligible: bool,
    /// Confidence-adjusted score from the latest snapshot.
    pub score: Option<f64>,
    pub outcome: Option<Outcome>,
    pub updated_at: String,
}

/// List tracked matches. By default finished matches and hidden ones are left out.
#[tauri::command]
pub async fn mm_list_matches(
    state: State<'_, AppState>,
    status: Option<MatchStatus>,
    profile_id: Option<String>,
    include_finished: Option<bool>,
    include_hidden: Option<bool>,
) -> Result<Vec<MatchSummary>, String> {
    let pool = state.db_manager.pool();
    let names: BTreeMap<String, Option<String>> = MmRepository::list_profiles(pool, true)
        .await
        .map_err(err)?
        .iter()
        .map(|sp| (sp.profile.id.clone(), profile_name(sp)))
        .collect();
    let rows = MatchRepo::list(pool).await.map_err(err)?;
    Ok(rows
        .into_iter()
        .filter(|(m, _)| status.map_or(true, |s| m.status == s))
        .filter(|(m, _)| profile_id.as_ref().map_or(true, |p| &m.profile_a == p || &m.profile_b == p))
        .filter(|(m, _)| include_finished.unwrap_or(status.is_some()) || !m.status.is_terminal())
        .filter(|(m, _)| include_hidden.unwrap_or(false) || !m.hidden)
        .map(|(m, card)| MatchSummary {
            name_a: names.get(&m.profile_a).cloned().flatten(),
            name_b: names.get(&m.profile_b).cloned().flatten(),
            eligible: eligible_of(&card),
            score: card.as_ref().and_then(|c| c.get("ranking_score")).and_then(|s| s.as_f64()),
            id: m.id,
            status: m.status,
            on_hold: m.hold_reason.is_some(),
            hidden: m.hidden,
            outcome: m.outcome,
            updated_at: m.updated_at,
        })
        .collect())
}

/// Number of visible matches per status (for the pipeline tabs).
#[tauri::command]
pub async fn mm_match_counts(state: State<'_, AppState>) -> Result<BTreeMap<String, i64>, String> {
    MatchRepo::counts(state.db_manager.pool()).await.map_err(err)
}

#[tauri::command]
pub async fn mm_get_match(state: State<'_, AppState>, id: String) -> Result<MatchDetail, String> {
    detail(state.db_manager.pool(), &id).await
}

/// Move a match along its lifecycle. Approving or recommending a pair the rules exclude needs `override_reason`;
/// closing needs an outcome (given here or recorded earlier).
#[tauri::command]
pub async fn mm_transition_match(
    state: State<'_, AppState>,
    id: String,
    to_status: MatchStatus,
    reason: Option<String>,
    override_reason: Option<String>,
    outcome: Option<Outcome>,
) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id).await?;
    let snap = MatchRepo::latest_snapshot(pool, &id).await.map_err(err)?;
    let ctx = TransitionContext {
        current: m.status,
        eligible: eligible_of(&snap.map(|s| s.scorecard)),
        a: m.a_response,
        b: m.b_response,
        outcome: m.outcome,
    };
    let req = TransitionRequest { reason, override_reason, outcome };
    let plan = validate_transition(&ctx, to_status, &req)?;
    let override_text = if plan.override_used { req.override_reason.as_ref().map(|s| s.trim().to_string()) } else { None };
    let detail_text = match (&plan.reason, &override_text) {
        (Some(r), Some(o)) => Some(format!("{r}\nOverride: {o}")),
        (Some(r), None) => Some(r.clone()),
        (None, Some(o)) => Some(format!("Override: {o}")),
        (None, None) => None,
    };
    MatchRepo::record_transition(pool, &id, m.status, to_status, plan.outcome, override_text.as_deref(), detail_text.as_deref()).await.map_err(err)?;
    // The global audit log keeps the move, never the free text.
    MmRepository::audit(pool, "match_transition", "match", &id, Some(&format!("{} -> {}{}", m.status.as_str(), to_status.as_str(), if plan.override_used { " (override)" } else { "" }))).await;
    detail(pool, &id).await
}

/// Record whether a person is interested in the introduction. Two "yes" answers advance the match; a "no" ends it.
#[tauri::command]
pub async fn mm_set_response(state: State<'_, AppState>, id: String, side: String, response: Interest) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id).await?;
    if !can_record_responses(m.status) {
        return Err("Responses can only be recorded while an introduction is in progress".into());
    }
    let (a, b) = match side.as_str() {
        "a" => (response, m.b_response),
        "b" => (m.a_response, response),
        _ => return Err("side must be 'a' or 'b'".into()),
    };
    let next = apply_responses(m.status, a, b);
    MatchRepo::record_responses(pool, &id, m.status, a, b, next, &side).await.map_err(err)?;
    MmRepository::audit(pool, "match_response", "match", &id, next.map(|s| s.as_str())).await;
    detail(pool, &id).await
}

/// Record what happened (first conversation, first meeting, relationship formed, ...). This is the label later learning uses.
#[tauri::command]
pub async fn mm_record_outcome(state: State<'_, AppState>, id: String, outcome: Outcome) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id).await?;
    if !can_record_outcome(m.status) {
        return Err("An outcome can be recorded once contact has been exchanged, or when a person declined".into());
    }
    MatchRepo::set_outcome(pool, &id, outcome).await.map_err(err)?;
    MmRepository::audit(pool, "match_outcome", "match", &id, Some(outcome.as_str())).await;
    detail(pool, &id).await
}

/// Mark that more information is needed (with what is needed), or clear the hold with `reason = None`.
#[tauri::command]
pub async fn mm_set_match_hold(state: State<'_, AppState>, id: String, reason: Option<String>) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id).await?;
    let reason = reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    validate_text("The request", &reason)?;
    if m.status.is_terminal() && reason.is_some() {
        return Err("This match is finished".into());
    }
    MatchRepo::set_hold(pool, &id, reason.as_deref()).await.map_err(err)?;
    MmRepository::audit(pool, if reason.is_some() { "match_info_requested" } else { "match_hold_cleared" }, "match", &id, None).await;
    detail(pool, &id).await
}

#[tauri::command]
pub async fn mm_add_match_note(state: State<'_, AppState>, id: String, text: String) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    require_match(pool, &id).await?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("A note cannot be empty".into());
    }
    validate_text("The note", &Some(text.clone()))?;
    MatchRepo::add_note(pool, &id, &text).await.map_err(err)?;
    MmRepository::audit(pool, "match_note", "match", &id, None).await;
    detail(pool, &id).await
}

/// Hide a match from the default lists (it is kept, and can be shown again).
#[tauri::command]
pub async fn mm_set_match_hidden(state: State<'_, AppState>, id: String, hidden: bool) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    require_match(pool, &id).await?;
    MatchRepo::set_hidden(pool, &id, hidden).await.map_err(err)?;
    MmRepository::audit(pool, if hidden { "match_hidden" } else { "match_unhidden" }, "match", &id, None).await;
    detail(pool, &id).await
}

/// Re-score a match with the current profile data and store a new snapshot (the old ones are kept). With
/// `use_latest_rule_set` the match moves to the rule set's newest version. `weights` (dimension -> 0..10)
/// replaces this match's own weight adjustments; an empty map removes them.
#[tauri::command]
pub async fn mm_rescore_match(
    state: State<'_, AppState>,
    id: String,
    use_latest_rule_set: Option<bool>,
    weights: Option<BTreeMap<String, f64>>,
) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id).await?;
    if m.status.is_terminal() {
        return Err("This match is finished".into());
    }
    let version = if use_latest_rule_set.unwrap_or(false) { None } else { Some(m.rule_set_version) };
    let (meta, set, _) = MmRepository::get_rule_set(pool, &m.rule_set_id, version).await.map_err(err)?.ok_or("Rule set version not found")?;
    let new_weights = match &weights {
        Some(w) => {
            check_weights(w)?;
            w.clone()
        }
        None => m.weight_overrides.clone(),
    };
    let card = scorecard_json(pool, &set, &m.profile_a, &m.profile_b, &new_weights).await?;
    let trigger = if weights.is_some() { "weights_adjusted" } else { "rescore" };
    let version_used = if use_latest_rule_set.unwrap_or(false) { meta.current_version } else { m.rule_set_version };
    MatchRepo::add_snapshot(pool, &id, &m.rule_set_id, version_used, trigger, &card, weights.as_ref()).await.map_err(err)?;
    MmRepository::audit(pool, "match_rescore", "match", &id, Some(trigger)).await;
    detail(pool, &id).await
}
