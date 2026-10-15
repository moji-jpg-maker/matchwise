use super::commands::{summary, ProfileSummary};
use super::repository::{MmRepository, RuleSetRow};
use super::scoring_config_repo::ScoringConfigRepo;
use crate::state::AppState;
use matchmaking_core::{
    dimension_catalog, evaluate_match, score_pair, validate_ruleset_for, Direction, DimensionDef, Finding, HardStatus, MatchEvaluation,
    Locale, RuleIssue, RuleSet, ScoreCard,
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
    /// The scoring config this rule set reads its scorecards with, if it has been given one.
    pub scoring_config_id: Option<String>,
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
    pub scoring_config_id: Option<String>,
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
        scoring_config_id: meta.scoring_config_id,
        version: def.version,
        definition: def,
        versions: versions.into_iter().map(|v| VersionInfo { version: v.version, created_at: v.created_at, note: v.note }).collect(),
    }
}

#[tauri::command]
pub async fn mm_list_rule_sets(state: State<'_, AppState>, include_archived: Option<bool>) -> Result<Vec<RuleSetSummary>, String> {
    let pool = state.db_manager.pool();
    MmRepository::ensure_default_rule_set(pool).await.map_err(err)?;
    // Every rule set needs a config to read its scorecards with; this also ports the prior a rule set used to hold.
    ScoringConfigRepo::ensure_scoring_configs(pool).await.map_err(err)?;
    let rows = MmRepository::list_rule_sets(pool, include_archived.unwrap_or(false)).await.map_err(err)?;
    Ok(rows
        .into_iter()
        .map(|(r, n)| RuleSetSummary {
            id: r.id,
            name: r.name,
            description: r.description,
            current_version: r.current_version,
            archived: r.archived,
            scoring_config_id: r.scoring_config_id,
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
pub async fn mm_validate_rule_set(state: State<'_, AppState>, definition: RuleSet, locale: Option<String>) -> Result<Vec<RuleIssue>, String> {
    let reg = MmRepository::registry(state.db_manager.pool()).await.map_err(err)?;
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    Ok(validate_ruleset_for(&definition, &reg, locale))
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
    scoring_config_id: Option<String>,
    note: Option<String>,
    locale: Option<String>,
) -> Result<RuleSetView, String> {
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    let pool = state.db_manager.pool();
    let name = name.trim().to_string();
    let description = description.unwrap_or_default();
    let mut definition = definition;
    definition.name = name.clone();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let issues = validate_ruleset_for(&definition, &reg, locale);
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
    // The config is a property of the rule set, not of a version: switching it re-reads existing results under the
    // new numbers rather than rewriting history, which is why it is stored on the rule set itself.
    if let Some(config_id) = &scoring_config_id {
        if ScoringConfigRepo::name(pool, config_id).await.map_err(err)?.is_none() {
            return Err("The chosen scoring config no longer exists".into());
        }
        ScoringConfigRepo::set_rule_set_config(pool, &saved_id, Some(config_id)).await.map_err(err)?;
        MmRepository::audit(pool, "rule_set_scoring_config", "rule_set", &saved_id, Some(config_id)).await;
    }
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
pub async fn mm_dimension_catalog(locale: Option<String>) -> Result<Vec<DimensionDef>, String> {
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    Ok(dimension_catalog().into_iter().map(|def| def.localized(locale)).collect())
}

#[tauri::command]
pub async fn mm_score_pair(
    state: State<'_, AppState>,
    rule_set_id: String,
    profile_a: String,
    profile_b: String,
    version: Option<u32>,
    locale: Option<String>,
) -> Result<MatchView, String> {
    let pool = state.db_manager.pool();
    let (_, set, _) = MmRepository::get_rule_set(pool, &rule_set_id, version).await.map_err(err)?.ok_or("Rule set not found")?;
    let a = MmRepository::get_profile(pool, &profile_a).await.map_err(err)?.ok_or("First profile not found")?;
    let b = MmRepository::get_profile(pool, &profile_b).await.map_err(err)?.ok_or("Second profile not found")?;
    let a_prefs = MmRepository::get_preferences(pool, &profile_a).await.map_err(err)?;
    let b_prefs = MmRepository::get_preferences(pool, &profile_b).await.map_err(err)?;
    let config = ScoringConfigRepo::config_for_rule_set(pool, &rule_set_id).await.map_err(err)?;
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    let mut scorecard = score_pair(&set, &config, &a.profile, &b.profile, &a_prefs, &b_prefs)?;
    let registry = MmRepository::registry(pool).await.map_err(err)?;
    let texts = |prefs: &[matchmaking_core::Preference]| -> std::collections::BTreeMap<String, String> {
        prefs
            .iter()
            .filter(|p| p.note.as_deref().map_or(true, |n| n.trim().is_empty()))
            .map(|p| (p.id.clone(), matchmaking_core::describe_preference_for(p, &registry, locale)))
            .collect()
    };
    let (a_texts, b_texts) = (texts(&a_prefs), texts(&b_prefs));
    scorecard.localize(locale, &|source, id| match source {
        matchmaking_core::FindingSource::PreferenceA => a_texts.get(id).cloned(),
        _ => b_texts.get(id).cloned(),
    });
    let mut evaluation = evaluate_match(&set, &a.profile, &b.profile, &a_prefs, &b_prefs)?;
    evaluation.localize(locale);
    Ok(MatchView {
        scorecard,
        evaluation,
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
) -> Result<MatchSearch, String> {
    let (candidates, total) =
        find_matches_with_total(state.db_manager.pool(), &profile_id, &rule_set_id, limit, include_ineligible.unwrap_or(false)).await?;
    Ok(MatchSearch { candidates, total })
}

#[derive(Serialize)]
pub struct MatchSearch {
    pub candidates: Vec<MatchCandidate>,
    pub total: usize,
}

/// The body of `mm_find_matches`, split out so it can be exercised without a Tauri `State`. Nothing about the result
/// changes: candidates are scored, filtered, ordered and cut here, and the exposure log records exactly that list.
pub async fn find_matches(
    pool: &sqlx::SqlitePool,
    profile_id: &str,
    rule_set_id: &str,
    limit: Option<usize>,
    include_ineligible: bool,
) -> Result<Vec<MatchCandidate>, String> {
    find_matches_with_total(pool, profile_id, rule_set_id, limit, include_ineligible).await.map(|(out, _)| out)
}

/// [`find_matches`] plus how many candidates matched before the limit cut the list.
pub async fn find_matches_with_total(
    pool: &sqlx::SqlitePool,
    profile_id: &str,
    rule_set_id: &str,
    limit: Option<usize>,
    include_ineligible: bool,
) -> Result<(Vec<MatchCandidate>, usize), String> {
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let (_, set, _) = MmRepository::get_rule_set(pool, &rule_set_id, None).await.map_err(err)?.ok_or("Rule set not found")?;
    let config = ScoringConfigRepo::effective_for_rule_set(pool, &rule_set_id).await.map_err(err)?;
    let me = MmRepository::get_profile(pool, &profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    let all = MmRepository::list_profiles(pool, false).await.map_err(err)?;
    let prefs = MmRepository::all_preferences(pool).await.map_err(err)?;
    let none = vec![];
    let my_prefs = prefs.get(profile_id).unwrap_or(&none);

    let mut out = vec![];
    for cand in all.iter().filter(|c| c.profile.id != profile_id) {
        let cand_prefs = prefs.get(&cand.profile.id).unwrap_or(&none);
        let card = score_pair(&set, &config.definition, &me.profile, &cand.profile, my_prefs, cand_prefs)?;
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
    if !include_ineligible {
        out.retain(|c| c.eligible);
    }
    out.sort_by(|x, y| {
        y.eligible
            .cmp(&x.eligible)
            .then(y.score.unwrap_or(-1.0).partial_cmp(&x.score.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal))
            .then(y.confidence.unwrap_or(-1.0).partial_cmp(&x.confidence.unwrap_or(-1.0)).unwrap_or(std::cmp::Ordering::Equal))
            .then(x.unknown_count.cmp(&y.unknown_count))
    });
    let total = out.len();
    out.truncate(limit.unwrap_or(50).min(500));
    // Log what was actually shown: one row per returned candidate, in the order returned. Candidates filtered out or
    // cut by the limit are not logged -- they were never on screen, and a browsing history that claims otherwise
    // would misrepresent the choices the matchmaker was given.
    let search_id = uuid::Uuid::new_v4().to_string();
    let exposures: Vec<super::exposure_repo::ExposureRow> = out
        .iter()
        .enumerate()
        .map(|(rank, c)| super::exposure_repo::ExposureRow {
            candidate_id: c.summary.id.clone(),
            rank: rank as i64,
            shown: true,
            score: c.score,
            confidence: c.confidence,
            eligible: Some(c.eligible),
        })
        .collect();
    super::exposure_repo::ExposureRepo::record(
        pool,
        &search_id,
        profile_id,
        rule_set_id,
        set.version,
        config.id.as_deref(),
        config.version,
        &exposures,
    )
    .await
    .map_err(err)?;
    Ok((out, total))
}
