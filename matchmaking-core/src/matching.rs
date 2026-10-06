//! Combines a rule set with both people's partner preferences into one evaluation of a pair.
//!
//! The combined score is provisional: it is the mean of the available component scores
//! (rule set, A's preferences about B, B's preferences about A). Multi-dimension scoring
//! (values, lifestyle, family, ...) replaces it in a later milestone.

use crate::preferences::{evaluate_preferences, Preference};
use crate::profile::Profile;
use crate::rules::{evaluate_ruleset_mutual, MatchOutcome, RuleSet};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct MatchEvaluation {
    pub rules: MatchOutcome,
    pub a_preferences: MatchOutcome,
    pub b_preferences: MatchOutcome,
    /// No failed hard rule, no failed must-have or matched deal-breaker, and the score threshold (if any) is met.
    pub eligible: bool,
    /// Some hard rule or must-have could not be decided because information is missing.
    pub needs_info: bool,
    /// 0..=100; None if nothing could be scored.
    pub score: Option<f64>,
    /// Mean of the components' coverage (see `MatchOutcome::coverage`): how much of the score is backed by data.
    pub coverage: Option<f64>,
    /// Authoritative threshold result (the per-component `meets_threshold` fields are not).
    pub meets_threshold: Option<bool>,
}

pub fn evaluate_match(
    set: &RuleSet,
    a: &Profile,
    b: &Profile,
    a_prefs: &[Preference],
    b_prefs: &[Preference],
) -> Result<MatchEvaluation, String> {
    let rules = evaluate_ruleset_mutual(set, a, b);
    let a_preferences = evaluate_preferences(a_prefs, a, b)?;
    let b_preferences = evaluate_preferences(b_prefs, b, a)?;
    let scores: Vec<f64> = [rules.soft_score, a_preferences.soft_score, b_preferences.soft_score].into_iter().flatten().collect();
    let score = if scores.is_empty() { None } else { Some(scores.iter().sum::<f64>() / scores.len() as f64) };
    let covs: Vec<f64> = [rules.coverage, a_preferences.coverage, b_preferences.coverage].into_iter().flatten().collect();
    let coverage = if covs.is_empty() { None } else { Some(covs.iter().sum::<f64>() / covs.len() as f64) };
    let meets_threshold = match (set.min_score, score) {
        (Some(min), Some(s)) => Some(s >= min),
        _ => None,
    };
    Ok(MatchEvaluation {
        eligible: rules.eligible && a_preferences.eligible && b_preferences.eligible && meets_threshold != Some(false),
        needs_info: !rules.needs_info.is_empty() || !a_preferences.needs_info.is_empty() || !b_preferences.needs_info.is_empty(),
        score,
        coverage,
        meets_threshold,
        rules,
        a_preferences,
        b_preferences,
    })
}
