use super::match_repo::{MatchRepo, MatchRow};
use super::repository::MmRepository;
use super::scoring_config_repo::ScoringConfigRepo;
use crate::state::AppState;
use matchmaking_core::{
    apply_responses, can_record_outcome, can_record_responses, score_pair, validate_text_for, validate_transition_for, Interest, MatchStatus,
    Outcome, RuleSet, ScoringConfig, TransitionContext, TransitionRequest,
    describe_preference_for, localize_scorecard_json, FindingSource, Locale,
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
    /// Which scoring config judged the latest snapshot, and which version of it. `None` when the snapshot predates
    /// scoring configs: those numbers were never stored, so the view shows nothing rather than a guess.
    pub scoring_config_id: Option<String>,
    pub scoring_config_name: Option<String>,
    pub scoring_config_version: Option<u32>,
    /// The numbers the latest snapshot was judged by, for the match view. Blank for pre-config snapshots.
    pub scoring_config: Option<ScoringConfig>,
    pub status: MatchStatus,
    /// Moves the state machine allows from here (the UI offers exactly these).
    pub allowed_next: Vec<MatchStatus>,
    /// The latest snapshot says the rules allow this pair; if false, recommending or approving needs an override reason.
    pub eligible: bool,
    pub can_record_responses: bool,
    pub can_record_outcome: bool,
    /// Whether each person can currently receive Telegram messages (linked, agreed to the notice, notifications on).
    pub reachable_a: bool,
    pub reachable_b: bool,
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

async fn localized_scorecard(pool: &SqlitePool, card: &mut Json, a_id: &str, b_id: &str, locale: Locale) -> Result<(), String> {
    if locale == Locale::En {
        return Ok(());
    }
    let registry = MmRepository::registry(pool).await.map_err(err)?;
    let mut texts: BTreeMap<(String, String), String> = BTreeMap::new();
    for (who, profile_id) in [("a", a_id), ("b", b_id)] {
        for preference in MmRepository::get_preferences(pool, profile_id).await.map_err(err)? {
            if preference.note.as_deref().map_or(true, |note| note.trim().is_empty()) {
                texts.insert((who.to_string(), preference.id.clone()), describe_preference_for(&preference, &registry, locale));
            }
        }
    }
    localize_scorecard_json(card, locale, &|source, id| {
        let who = if source == FindingSource::PreferenceA { "a" } else { "b" };
        texts.get(&(who.to_string(), id.to_string())).cloned()
    });
    Ok(())
}

fn locale_of(raw: &Option<String>) -> Locale {
    Locale::parse_or_default(raw.as_deref().unwrap_or_default())
}

async fn detail(pool: &SqlitePool, id: &str, locale: Locale) -> Result<MatchDetail, String> {
    let m = MatchRepo::get(pool, id).await.map_err(err)?.ok_or(locale.pick("Match not found", "تطبیق پیدا نشد"))?;
    let mut snap = MatchRepo::latest_snapshot(pool, id).await.map_err(err)?;
    if let Some(snapshot) = snap.as_mut() {
        localized_scorecard(pool, &mut snapshot.scorecard, &m.profile_a, &m.profile_b, locale).await?;
    }
    let rule_set_name = MmRepository::get_rule_set_name(pool, &m.rule_set_id).await.map_err(err)?;
    // The config that judged the newest snapshot, resolved from the snapshot's own record rather than from the rule
    // set: a stored scorecard names the version it was judged by, and that is what the view must report.
    let config_id = snap.as_ref().and_then(|s| s.scoring_config_id.clone());
    let config_version = snap.as_ref().and_then(|s| s.scoring_config_version);
    let scoring_config = match (&config_id, config_version) {
        (Some(cid), Some(cv)) => ScoringConfigRepo::get(pool, cid, Some(cv)).await.map_err(err)?.map(|(meta, def, _)| (meta.name, def)),
        _ => None,
    };
    let (scoring_config_name, scoring_config_def) = match scoring_config {
        Some((name, def)) => (Some(name), Some(def)),
        None => (None, None),
    };
    let notes = MatchRepo::notes(pool, id).await.map_err(err)?;
    let events = MatchRepo::events(pool, id).await.map_err(err)?;
    let (reachable_a, reachable_b) = crate::telegram::notify::reachable_sides(pool, &m).await?;
    Ok(MatchDetail {
        reachable_a,
        reachable_b,
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
        scoring_config_id: config_id,
        scoring_config_name,
        scoring_config_version: config_version,
        scoring_config: scoring_config_def,
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

fn check_weights(w: &BTreeMap<String, f64>, locale: Locale) -> Result<(), String> {
    if w.len() > 30 {
        return Err(locale.pick("Too many weight adjustments", "تعداد تنظیم‌های وزن بیش از حد است").into());
    }
    for (k, v) in w {
        if k.trim().is_empty() || !v.is_finite() || !(0.0..=10.0).contains(v) {
            return Err(match locale {
                Locale::En => format!("The weight for '{k}' must be between 0 and 10"),
                Locale::Fa => format!("وزن «{k}» باید بین ۰ و ۱۰ باشد"),
            });
        }
    }
    Ok(())
}

/// Score a pair with per-match weight adjustments applied on top of the rule set.
pub(crate) async fn compute_scorecard(
    pool: &SqlitePool,
    set: &RuleSet,
    config: &ScoringConfig,
    a_id: &str,
    b_id: &str,
    weights: &BTreeMap<String, f64>,
) -> Result<matchmaking_core::ScoreCard, String> {
    let a = MmRepository::get_profile(pool, a_id).await.map_err(err)?.ok_or("First profile not found")?;
    let b = MmRepository::get_profile(pool, b_id).await.map_err(err)?.ok_or("Second profile not found")?;
    let a_prefs = MmRepository::get_preferences(pool, a_id).await.map_err(err)?;
    let b_prefs = MmRepository::get_preferences(pool, b_id).await.map_err(err)?;
    let mut set = set.clone();
    for (k, v) in weights {
        set.group_weights.insert(k.clone(), *v);
    }
    score_pair(&set, config, &a.profile, &b.profile, &a_prefs, &b_prefs)
}

async fn scorecard_json(
    pool: &SqlitePool,
    set: &RuleSet,
    config: &ScoringConfig,
    a_id: &str,
    b_id: &str,
    weights: &BTreeMap<String, f64>,
) -> Result<Json, String> {
    serde_json::to_value(compute_scorecard(pool, set, config, a_id, b_id, weights).await?).map_err(err)
}

async fn require_match(pool: &SqlitePool, id: &str, locale: Locale) -> Result<MatchRow, String> {
    MatchRepo::get(pool, id).await.map_err(err)?.ok_or_else(|| locale.pick("Match not found", "تطبیق پیدا نشد").to_string())
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
    locale: Option<String>,
) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    if profile_a == profile_b {
        return Err(locale_of(&locale).pick("A match needs two different people", "تطبیق به دو نفر متفاوت نیاز دارد").into());
    }
    if let Some(existing) = MatchRepo::find_by_pair(pool, &profile_a, &profile_b).await.map_err(err)? {
        return detail(pool, &existing.id, locale_of(&locale)).await;
    }
    for id in [&profile_a, &profile_b] {
        let p = MmRepository::get_profile(pool, id).await.map_err(err)?.ok_or(locale_of(&locale).pick("Profile not found", "پروفایل پیدا نشد"))?;
        if p.status != "active" {
            return Err(locale_of(&locale).pick("Deactivated profiles cannot be matched", "پروفایل‌های غیرفعال را نمی‌توان تطبیق داد").into());
        }
    }
    let (_, set, _) = MmRepository::get_rule_set(pool, &rule_set_id, None).await.map_err(err)?.ok_or(locale_of(&locale).pick("Rule set not found", "مجموعه قوانین پیدا نشد"))?;
    let config = ScoringConfigRepo::effective_for_rule_set(pool, &rule_set_id).await.map_err(err)?;
    let (a, b) = super::match_repo::pair_key(&profile_a, &profile_b);
    let card = scorecard_json(pool, &set, &config.definition, &a, &b, &BTreeMap::new()).await?;
    let status = if eligible_of(&Some(card.clone())) { MatchStatus::Recommended } else { MatchStatus::Identified };
    let id = MatchRepo::create(
        pool,
        &a,
        &b,
        source_profile.as_deref(),
        &rule_set_id,
        set.version,
        config.id.as_deref(),
        config.version,
        status,
        &card,
    )
    .await
    .map_err(err)?;
    MmRepository::audit(pool, "create_match", "match", &id, Some(status.as_str())).await;
    detail(pool, &id, locale_of(&locale)).await
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
pub async fn mm_get_match(state: State<'_, AppState>, id: String, locale: Option<String>) -> Result<MatchDetail, String> {
    detail(state.db_manager.pool(), &id, locale_of(&locale)).await
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
    locale: Option<String>,
) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id, locale_of(&locale)).await?;
    let snap = MatchRepo::latest_snapshot(pool, &id).await.map_err(err)?;
    let ctx = TransitionContext {
        current: m.status,
        eligible: eligible_of(&snap.map(|s| s.scorecard)),
        a: m.a_response,
        b: m.b_response,
        outcome: m.outcome,
    };
    let req = TransitionRequest { reason, override_reason, outcome };
    let plan = validate_transition_for(&ctx, to_status, &req, locale_of(&locale))?;
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
    crate::telegram::notify::after_status_change(pool, &id, m.status, to_status).await;
    detail(pool, &id, locale_of(&locale)).await
}

/// Record one person's answer and let it move the match. Used by the app and by the Telegram bot, so both
/// follow the same rules and trigger the same notifications. Returns the status before and after.
pub(crate) async fn set_response_inner(pool: &SqlitePool, id: &str, side: &str, response: Interest, locale: Locale) -> Result<(MatchStatus, MatchStatus), String> {
    let m = require_match(pool, id, locale).await?;
    if !can_record_responses(m.status) {
        return Err(locale
            .pick(
                "Responses can only be recorded while an introduction is in progress",
                "پاسخ‌ها را فقط در زمانی می‌توان ثبت کرد که معرفی در جریان است",
            )
            .into());
    }
    let (a, b) = match side {
        "a" => (response, m.b_response),
        "b" => (m.a_response, response),
        _ => return Err(locale.pick("side must be 'a' or 'b'", "سمت باید «a» یا «b» باشد").into()),
    };
    let next = apply_responses(m.status, a, b);
    MatchRepo::record_responses(pool, id, m.status, a, b, next, side).await.map_err(err)?;
    MmRepository::audit(pool, "match_response", "match", id, next.map(|s| s.as_str())).await;
    let new_status = next.unwrap_or(m.status);
    if new_status != m.status {
        crate::telegram::notify::after_status_change(pool, id, m.status, new_status).await;
    }
    Ok((m.status, new_status))
}

/// Record whether a person is interested in the introduction. Two "yes" answers advance the match; a "no" ends it.
#[tauri::command]
pub async fn mm_set_response(state: State<'_, AppState>, id: String, side: String, response: Interest, locale: Option<String>) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    set_response_inner(pool, &id, &side, response, locale_of(&locale)).await?;
    detail(pool, &id, locale_of(&locale)).await
}

/// Record what happened (first conversation, first meeting, relationship formed, ...). This is the label later learning uses.
#[tauri::command]
pub async fn mm_record_outcome(state: State<'_, AppState>, id: String, outcome: Outcome, locale: Option<String>) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id, locale_of(&locale)).await?;
    if !can_record_outcome(m.status) {
        return Err(locale_of(&locale)
            .pick(
                "An outcome can be recorded once contact has been exchanged, or when a person declined",
                "نتیجه را می‌توان پس از تبادل اطلاعات تماس یا هنگام انصراف یکی از طرفین ثبت کرد",
            )
            .into());
    }
    MatchRepo::set_outcome(pool, &id, outcome).await.map_err(err)?;
    MmRepository::audit(pool, "match_outcome", "match", &id, Some(outcome.as_str())).await;
    detail(pool, &id, locale_of(&locale)).await
}

/// Mark that more information is needed (with what is needed), or clear the hold with `reason = None`.
#[tauri::command]
pub async fn mm_set_match_hold(state: State<'_, AppState>, id: String, reason: Option<String>, locale: Option<String>) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id, locale_of(&locale)).await?;
    let reason = reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
    validate_text_for(locale_of(&locale).pick("The request", "درخواست"), &reason, locale_of(&locale))?;
    if m.status.is_terminal() && reason.is_some() {
        return Err(locale_of(&locale).pick("This match is finished", "این تطبیق پایان یافته است").into());
    }
    MatchRepo::set_hold(pool, &id, reason.as_deref()).await.map_err(err)?;
    MmRepository::audit(pool, if reason.is_some() { "match_info_requested" } else { "match_hold_cleared" }, "match", &id, None).await;
    detail(pool, &id, locale_of(&locale)).await
}

#[tauri::command]
pub async fn mm_add_match_note(state: State<'_, AppState>, id: String, text: String, locale: Option<String>) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    require_match(pool, &id, locale_of(&locale)).await?;
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(locale_of(&locale).pick("A note cannot be empty", "یادداشت نمی‌تواند خالی باشد").into());
    }
    validate_text_for(locale_of(&locale).pick("The note", "یادداشت"), &Some(text.clone()), locale_of(&locale))?;
    MatchRepo::add_note(pool, &id, &text).await.map_err(err)?;
    MmRepository::audit(pool, "match_note", "match", &id, None).await;
    detail(pool, &id, locale_of(&locale)).await
}

/// Hide a match from the default lists (it is kept, and can be shown again).
#[tauri::command]
pub async fn mm_set_match_hidden(state: State<'_, AppState>, id: String, hidden: bool, locale: Option<String>) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    require_match(pool, &id, locale_of(&locale)).await?;
    MatchRepo::set_hidden(pool, &id, hidden).await.map_err(err)?;
    MmRepository::audit(pool, if hidden { "match_hidden" } else { "match_unhidden" }, "match", &id, None).await;
    detail(pool, &id, locale_of(&locale)).await
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
    locale: Option<String>,
) -> Result<MatchDetail, String> {
    let pool = state.db_manager.pool();
    let m = require_match(pool, &id, locale_of(&locale)).await?;
    if m.status.is_terminal() {
        return Err(locale_of(&locale).pick("This match is finished", "این تطبیق پایان یافته است").into());
    }
    let version = if use_latest_rule_set.unwrap_or(false) { None } else { Some(m.rule_set_version) };
    let (meta, set, _) = MmRepository::get_rule_set(pool, &m.rule_set_id, version).await.map_err(err)?.ok_or(locale_of(&locale).pick("Rule set version not found", "نسخه مجموعه قوانین پیدا نشد"))?;
    let config = ScoringConfigRepo::effective_for_rule_set(pool, &m.rule_set_id).await.map_err(err)?;
    let new_weights = match &weights {
        Some(w) => {
            check_weights(w, locale_of(&locale))?;
            w.clone()
        }
        None => m.weight_overrides.clone(),
    };
    let card = scorecard_json(pool, &set, &config.definition, &m.profile_a, &m.profile_b, &new_weights).await?;
    let trigger = if weights.is_some() { "weights_adjusted" } else { "rescore" };
    let version_used = if use_latest_rule_set.unwrap_or(false) { meta.current_version } else { m.rule_set_version };
    MatchRepo::add_snapshot(
        pool,
        &id,
        &m.rule_set_id,
        version_used,
        config.id.as_deref(),
        config.version,
        trigger,
        &card,
        weights.as_ref(),
    )
    .await
    .map_err(err)?;
    MmRepository::audit(pool, "match_rescore", "match", &id, Some(trigger)).await;
    detail(pool, &id, locale_of(&locale)).await
}
