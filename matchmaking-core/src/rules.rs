//! Versioned rule sets: hard constraints plus weighted soft preferences.
//! The deterministic result is the source of truth; AI/ML layers may only rank among
//! pairs that survive the hard rules.
//!
//! Rules are data (JSON), edited through the admin UI. Features:
//! * `Hard` rules exclude a pair; `Soft` rules add weighted score.
//! * `when`: a rule only applies if its condition holds (conditional rules). A false condition
//!   skips the rule entirely (it neither passes nor fails and does not affect the score).
//! * `scope`: `Pair` rules are symmetric and run once; `Directional` rules read "A's partner is B"
//!   and run in both directions (A->B and B->A).
//! * `group` and the set's `group_weights` let a matchmaker re-weight whole families of rules.
//! * `priority` orders results (highest first) so the most important findings are shown first.
//! * `min_score` is a threshold on the combined soft score.

use crate::expr::{evaluate, Expr, Tri, Trace};
use crate::profile::Profile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// Must hold; a False result excludes the pair, Unknown flags missing info.
    Hard,
    /// Contributes `weight` to the score when true.
    Soft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    /// Symmetric rule, evaluated once with (A, B).
    #[default]
    Pair,
    /// "A's partner is B": evaluated as (A, B) and again as (B, A).
    Directional,
}

fn one() -> f64 { 1.0 }
fn yes() -> bool { true }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub description: String,
    pub kind: RuleKind,
    #[serde(default = "one")]
    pub weight: f64,
    pub expr: Expr,
    /// Rule applies only when this holds (None = always).
    #[serde(default)]
    pub when: Option<Expr>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub scope: RuleScope,
    #[serde(default = "yes")]
    pub enabled: bool,
}

impl Rule {
    /// A rule with default options (always applies, pair scope, enabled, priority 0).
    pub fn new(id: &str, description: &str, kind: RuleKind, weight: f64, expr: Expr) -> Self {
        Rule {
            id: id.into(),
            description: description.into(),
            kind,
            weight,
            expr,
            when: None,
            group: None,
            priority: 0,
            scope: RuleScope::Pair,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleSet {
    pub name: String,
    /// Bump on every edit so each recommendation can record which version produced it.
    pub version: u32,
    pub rules: Vec<Rule>,
    /// Multiplier applied to the weight of soft rules in a group (default 1.0 for unlisted groups).
    #[serde(default)]
    pub group_weights: BTreeMap<String, f64>,
    /// Minimum combined soft score (0..=100) for a pair to be recommended.
    #[serde(default)]
    pub min_score: Option<f64>,
    /// Neutral score (0..=100, default 50) that thin evidence is pulled towards in the ranking score.
    #[serde(default)]
    pub prior_score: Option<f64>,
}

impl RuleSet {
    pub fn new(name: &str, version: u32, rules: Vec<Rule>) -> Self {
        RuleSet { name: name.into(), version, rules, group_weights: BTreeMap::new(), min_score: None, prior_score: None }
    }

    fn effective_weight(&self, r: &Rule) -> f64 {
        let gw = r.group.as_ref().and_then(|g| self.group_weights.get(g)).copied().unwrap_or(1.0);
        r.weight * gw
    }
}

/// Which way a directional rule was evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// A pair-scope rule, or any rule evaluated through [`evaluate_pair`].
    Pair,
    AToB,
    BToA,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    pub rule_id: String,
    pub description: String,
    pub kind: RuleKind,
    pub group: Option<String>,
    pub priority: i32,
    pub direction: Direction,
    /// False when the rule's `when` condition was false: the rule was skipped.
    pub applicable: bool,
    pub result: Tri,
    /// Weight after group scaling (soft rules).
    pub weight: f64,
    pub trace: Trace,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchOutcome {
    pub rule_set: String,
    pub rule_set_version: u32,
    /// False if any hard rule is False.
    pub eligible: bool,
    /// Hard rules that could not be decided because data is missing ("rule_id" or "rule_id:direction").
    pub needs_info: Vec<String>,
    /// 0..=100 over applicable soft rules whose result is known; None if there are none.
    pub soft_score: Option<f64>,
    /// Soft rules that could not be evaluated, i.e. unknown information.
    pub unknown_soft: Vec<String>,
    /// Share (0..=1) of the applicable soft-rule weight that could actually be evaluated; low coverage means
    /// the score rests on little information. None if there is no applicable soft rule.
    pub coverage: Option<f64>,
    /// Some(false) when `min_score` is set and the score is below it; None when not applicable or unknown.
    pub meets_threshold: Option<bool>,
    /// Ordered by priority (highest first), then by position in the rule set.
    pub results: Vec<RuleResult>,
}

fn eval_rule(set: &RuleSet, r: &Rule, a: &Profile, b: &Profile, direction: Direction) -> RuleResult {
    let weight = set.effective_weight(r);
    let make = |applicable: bool, result: Tri, trace: Trace| RuleResult {
        rule_id: r.id.clone(),
        description: r.description.clone(),
        kind: r.kind,
        group: r.group.clone(),
        priority: r.priority,
        direction,
        applicable,
        result,
        weight,
        trace,
    };
    if let Some(when) = &r.when {
        let (w, wt) = evaluate(when, a, Some(b));
        match w {
            Tri::False => return make(false, Tri::True, wt), // skipped: not applicable
            Tri::Unknown => return make(true, Tri::Unknown, wt), // cannot tell whether the rule applies
            Tri::True => {}
        }
    }
    let (res, trace) = evaluate(&r.expr, a, Some(b));
    make(true, res, trace)
}

fn label(r: &RuleResult) -> String {
    match r.direction {
        Direction::Pair => r.rule_id.clone(),
        Direction::AToB => format!("{}:a_to_b", r.rule_id),
        Direction::BToA => format!("{}:b_to_a", r.rule_id),
    }
}

fn aggregate(set: &RuleSet, mut results: Vec<RuleResult>) -> MatchOutcome {
    let mut eligible = true;
    let mut needs_info = vec![];
    let mut unknown_soft = vec![];
    let (mut got, mut possible, mut unknown_weight) = (0.0, 0.0, 0.0);
    for r in results.iter().filter(|r| r.applicable) {
        match (r.kind, r.result) {
            (RuleKind::Hard, Tri::False) => eligible = false,
            (RuleKind::Hard, Tri::Unknown) => needs_info.push(label(r)),
            (RuleKind::Soft, Tri::Unknown) => {
                unknown_soft.push(label(r));
                unknown_weight += r.weight;
            }
            (RuleKind::Soft, Tri::True) => { got += r.weight; possible += r.weight; }
            (RuleKind::Soft, Tri::False) => { possible += r.weight; }
            _ => {}
        }
    }
    results.sort_by(|x, y| y.priority.cmp(&x.priority)); // stable: ties keep rule-set order
    let soft_score = if possible > 0.0 { Some(100.0 * got / possible) } else { None };
    let coverage = if possible + unknown_weight > 0.0 { Some(possible / (possible + unknown_weight)) } else { None };
    let meets_threshold = match (set.min_score, soft_score) {
        (Some(min), Some(s)) => Some(s >= min),
        _ => None,
    };
    MatchOutcome {
        rule_set: set.name.clone(),
        rule_set_version: set.version,
        eligible,
        needs_info,
        soft_score,
        unknown_soft,
        coverage,
        meets_threshold,
        results,
    }
}

/// Evaluate every enabled rule once, from `a`'s perspective on partner `b` (scope is ignored).
/// Used for partner preferences and for single-direction checks.
pub fn evaluate_pair(set: &RuleSet, a: &Profile, b: &Profile) -> MatchOutcome {
    let results = set
        .rules
        .iter()
        .filter(|r| r.enabled)
        .map(|r| eval_rule(set, r, a, b, Direction::Pair))
        .collect();
    aggregate(set, results)
}

/// Evaluate a rule set for an unordered pair: `Pair` rules once, `Directional` rules in both directions.
pub fn evaluate_ruleset_mutual(set: &RuleSet, a: &Profile, b: &Profile) -> MatchOutcome {
    let mut results = vec![];
    for r in set.rules.iter().filter(|r| r.enabled) {
        match r.scope {
            RuleScope::Pair => results.push(eval_rule(set, r, a, b, Direction::Pair)),
            RuleScope::Directional => {
                results.push(eval_rule(set, r, a, b, Direction::AToB));
                results.push(eval_rule(set, r, b, a, Direction::BToA));
            }
        }
    }
    aggregate(set, results)
}
