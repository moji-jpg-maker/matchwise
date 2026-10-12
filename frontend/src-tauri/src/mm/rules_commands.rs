use super::commands::{summary, ProfileSummary};
use super::repository::{MmRepository, RuleSetRow};
use crate::state::AppState;
use matchmaking_core::{
    dimension_catalog, evaluate_match, score_pair, validate_ruleset, Direction, DimensionDef, Finding, HardStatus, MatchEvaluation,
    RuleIssue, RuleSet, ScoreCard,
};
use serde::Serialize;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
pub struct RuleSetSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub current_version: u32,
    pub archived: bool,
    pub rule_count: usize,
    pub updated_at: String,
}

#[derive(Serialize)]
pub struct VersionInfo {
    pub version: u32,
    pub created_at: String,
    pub note: Option<String>,
}

#[derive(Serialize)]
pub struct RuleSetView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub archived: bool,
    pub current_version: u32,
    /// The version whose definition is shown (may be older than current).
    pub version: u32,
    pub definition: RuleSet,
    pub versions: Vec<VersionInfo>,
}

fn view(meta: RuleSetRow, def: RuleSet, versions: Vec<super::repository::VersionRow>) -> RuleSetView {
    RuleSetView {
        id: meta.id,
        name: meta.name,
        description: meta.description,
        archived: meta.archived,
        current_version: meta.current_version,
        version: def.version,
        definition: def,
        versions: versions.into_iter().map(|v| VersionInfo { version: v.version, created_at: v.created_at, note: v.note }).collect(),
    }
}

#[tauri::command]
pub async fn mm_list_rule_sets(state: State<'_, AppState>, include_archived: Option<bool>) -> Result<Vec<RuleSetSummary>, String> {
    let pool = state.db_manager.pool();
    MmRepository::ensure_default_rule_set(pool).await.map_err(err)?;
    let rows = MmRepository::list_rule_sets(pool, include_archived.unwrap_or(false)).await.map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|(r, n)| RuleSetSummary {
            id: r.id,
            name: r.name,
            description: r.description,
            current_version: r.current_version,
            archived: r.archived,
            rule_count: n,
            updated_at: r.updated_at,
        })
        .collect())
}

#[tauri::command]
pub async fn mm_get_rule_set(state: State<'_, AppState>, id: String, version: Option<u32>) -> Result<RuleSetView, String> {
    let (meta, def, versions) = MmRepository::get_rule_set(state.db_manager.pool(), &id, version)
        .await
        .map_err(err)?
        .ok_or("Rule set or version not found")?;
    Ok(view(meta, def, versions))
}

/// Live validation for the editor: nothing is saved.
#[tauri::command]
pub async fn mm_validate_rule_set(state: State<'_, AppState>, definition: RuleSet) -> Result<Vec<RuleIssue>, String> {
    let reg = MmRepository::registry(state.db_manager.pool()).await.map_err(err)?;
    Ok(validate_ruleset(&definition, &reg))
}

/// Create a rule set (`id` = None) or save a new version of an existing one. Refuses to save anything
/// that fails validation, so a stored rule set can always be evaluated.
#[tauri::command]
pub async fn mm_save_rule_set(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    description: Option<String>,
    definition: RuleSet,
    note: Option<String>,
) -> Result<RuleSetView, String> {
    let pool = state.db_manager.pool();
    let name = name.trim().to_string();
    let description = description.unwrap_or_default();
    let mut definition = definition;
    definition.name = name.clone();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let issues = validate_ruleset(&definition, &reg);
    if !issues.is_empty() {
        let shown: Vec<String> = issues
            .iter()
            .take(5)
            .map(|i| match &i.rule_id {
                Some(r) => format!("{r}: {}", i.message),
                None => i.message.clone(),
            })
            .collect();
        return Err(format!("Not saved. {}{}", shown.join("; "), if issues.len() > 5 { format!(" (+{} more)", issues.len() - 5) } else { String::new() }));
    }
    if MmRepository::rule_set_name_taken(pool, &name, id.as_deref()).await.map_err(err)? {
        return Err(format!("A rule set named '{name}' already exists"));
    }
    let note = note.filter(|n| !n.trim().is_empty());
    let (saved_id, version) = match id {
        Some(existing) => {
            let v = MmRepository::save_rule_set_version(pool, &existing, &name, &description, &definition, note.as_deref()).await.map_err(err)?;
            (existing, v)
        }
        None => (MmRepository::create_rule_set(pool, &name, &description, &definition, note.as_deref()).await.map_err(err)?, 1),
    };
    MmRepository::audit(pool, "save_rule_set", "rule_set", &saved_id, Some(&format!("version {version}"))).await;
    let (meta, def, versions) = MmRepository::get_rule_set(pool, &saved_id, None).await.map_err(err)?.ok_or("Rule set vanished")?;
    Ok(view(meta, def, versions))
}

#[tauri::command]
pub async fn mm_archive_rule_set(state: State<'_, AppState>, id: String, archived: bool) -> Result<(), String> {
    let pool = state.db_manager.pool();
    if MmRepository::set_rule_set_archived(pool, &id, archived).await.map_err(err)? == 0 {
        return Err("Rule set not found".into());
    }
    MmRepository::audit(pool, if archived { "archive_rule_set" } else { "restore_rule_set" }, "rule_set", &id, None).await;
    Ok(())
}

/// Evaluate a pair of people with a rule set plus both people's partner preferences.
#[tauri::command]
pub async fn mm_evaluate_match(
    state: State<'_, AppState>,
    rule_set_id: String,
    profile_a: String,
    profile_b: String,
    version: Option<u32>,
) -> Result<MatchEvaluation, String> {
    let pool = state.db_manager.pool();
    let (_, set, _) = MmRepository::get_rule_set(pool, &rule_set_id, version).await.map_err(err)?.ok_or("Rule set not found")?;
    let a = MmRepository::get_profile(pool, &profile_a).await.map_err(err)?.ok_or("First profile not found")?;
    let b = MmRepository::get_profile(pool, &profile_b).await.map_err(err)?.ok_or("Second profile not found")?;
    let a_prefs = MmRepository::get_preferences(pool, &profile_a).await.map_err(err)?;
    let b_prefs = MmRepository::get_preferences(pool, &profile_b).await.map_err(err)?;
    evaluate_match(&set, &a.profile, &b.profile, &a_prefs, &b_prefs)
}

/// Everything the match view needs: the dimension scorecard plus the underlying rule-by-rule evaluation.
#[derive(Serialize)]
pub struct MatchView {
    pub scorecard: ScoreCard,
    pub evaluation: MatchEvaluation,
}

#[tauri::command]
pub async fn mm_dimension_catalog() -> Result<Vec<DimensionDef>, String> {
    Ok(dimension_catalog())
}

#[tauri::command]
pub async fn mm_score_pair(
    state: State<'_, AppState>,
    rule_set_id: String,
    profile_a: String,
    profile_b: String,
    version: Option<u32>,
) -> Result<MatchView, String> {
    let pool = state.db_manager.pool();
    let (_, set, _) = MmRepository::get_rule_set(pool, &rule_set_id, version).await.map_err(err)?.ok_or("Rule set not found")?;
    let a = MmRepository::get_profile(pool, &profile_a).await.map_err(err)?.ok_or("First profile not found")?;
    let b = MmRepository::get_profile(pool, &profile_b).await.map_err(err)?.ok_or("Second profile not found")?;
    let a_prefs = MmRepository::get_preferences(pool, &profile_a).await.map_err(err)?;
    let b_prefs = MmRepository::get_preferences(pool, &profile_b).await.map_err(err)?;
    Ok(MatchView {
        scorecard: score_pair(&set, &a.profile, &b.profile, &a_prefs, &b_prefs)?,
        evaluation: evaluate_match(&set, &a.profile, &b.profile, &a_prefs, &b_prefs)?,
    })
}

#[derive(Serialize)]
pub struct MatchCandidate {
    #[serde(flatten)]
    pub summary: ProfileSummary,
    pub eligible: bool,
    /// Some must-have could not be decided because information is missing.
    pub needs_info: bool,
    /// Confidence-adjusted score used for ranking (0..=100).
    pub score: Option<f64>,
    /// Raw coverage-weighted mean of the dimension scores, before the confidence adjustment.
    pub overall: Option<f64>,
    /// How much of the picture is backed by data (0..=1).
    pub confidence: Option<f64>,
    /// Plain-language reasons the pair is excluded (empty when eligible).
    pub blocking: Vec<String>,
    /// Top strengths and concerns, for the list view.
    pub strengths: Vec<String>,
    pub concerns: Vec<String>,
    /// Number of checks that could not be evaluated because information is missing.
    pub unknown_count: usize,
}

fn direction_suffix(d: Direction) -> &'static str {
    match d {
        Direction::Pair => "",
        Direction::AToB => " (this person's side)",
        Direction::BToA => " (candidate's side)",
    }
}

fn describe(f: &Finding) -> String {
    use matchmaking_core::FindingSource::*;
    let who = match f.source {
        Rule => direction_suffix(f.direction).to_string(),
        PreferenceA => " [this person's preference]".to_string(),
        PreferenceB => " [candidate's preference]".to_string(),
    };
    format!("{}{}", f.description, who)
}

/// Rank all active candidates for one person under a rule set by the confidence-adjusted score. Eligible
/// candidates come first. `include_ineligible` also returns excluded candidates with the reasons, so a
/// matchmaker can see why someone was left out.
#[tauri::command]
pub async fn mm_find_matches(
    state: State<'_, AppState>,
    profile_id: String,
    rule_set_id: String,
    limit: Option<usize>,
    include_ineligible: Option<bool>,
) -> Result<Vec<MatchCandidate>, String> {
    let pool = state.db_manager.pool();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let (_, set, _) = MmRepository::get_rule_set(pool, &rule_set_id, None).await.map_err(err)?.ok_or("Rule set not found")?;
    let me = MmRepository::get_profile(pool, &profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    let all = MmRepository::list_profiles(pool, false).await.map_err(err)?;
    let prefs = MmRepository::all_preferences(pool).await.map_err(err)?;
    let none = vec![];
    let my_prefs = prefs.get(&profile_id).unwrap_or(&none);

    let mut out = vec![];
    for cand in all.iter().filter(|c| c.profile.id != profile_id) {
        let cand_prefs = prefs.get(&cand.profile.id).unwrap_or(&none);
        let card = score_pair(&set, &me.profile, &cand.profile, my_prefs, cand_prefs)?;
        let mut blocking: Vec<String> = card.hard_constraints.violations.iter().map(describe).collect();
        if card.meets_threshold == Some(false) {
            blocking.push(format!("Score below the minimum of {}", set.min_score.unwrap_or(0.0)));
        }
        out.push(MatchCandidate {
            summary: summary(cand, &reg),
            eligible: card.eligible,
            needs_info: card.hard_constraints.status == HardStatus::NeedsInfo,
            score: card.ranking_score,
            overall: card.overall,
            confidence: card.confidence,
            blocking,
            strengths: card.strengths.iter().take(2).map(describe).collect(),
            concerns: card.concerns.iter().take(2).map(describe).collect(),
            unknown_count: card.unknowns.len(),
        });
    }
    if !include_ineligible.unwrap_or(false) {
        out.retain(|c| c.eligible);
    }
    out.sort_by(|x, y| {
        y.eligible
            .cmp(&x.eligible)
            .then(y.score.unwrap_or(-1.0).partial_cmp(&x.score.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal))
            .then(y.confidence.unwrap_or(-1.0).partial_cmp(&x.confidence.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal))
            .then(x.unknown_count.cmp(&y.unknown_count))
    });
    out.truncate(limit.unwrap_or(50).min(500));
    Ok(out)
}
