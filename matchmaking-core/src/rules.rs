//! Versioned rule sets: hard constraints plus weighted soft preferences.
//! The deterministic result is the source of truth; AI/ML layers may only rank among
//! pairs that survive the hard rules.

use crate::expr::{evaluate, Expr, Tri, Trace};
use crate::profile::Profile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// Must hold; a False result excludes the pair, Unknown flags missing info.
    Hard,
    /// Contributes `weight` to the score when true.
    Soft,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub description: String,
    pub kind: RuleKind,
    #[serde(default = "one")]
    pub weight: f64,
    pub expr: Expr,
}

fn one() -> f64 { 1.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub name: String,
    /// Bump on every edit so each recommendation can record which version produced it.
    pub version: u32,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleResult {
    pub rule_id: String,
    pub kind: RuleKind,
    pub result: Tri,
    pub trace: Trace,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchOutcome {
    pub rule_set: String,
    pub rule_set_version: u32,
    /// False if any hard rule is False.
    pub eligible: bool,
    /// Hard rules that could not be decided because data is missing.
    pub needs_info: Vec<String>,
    /// 0..=100 over soft rules whose result is known; None if there are no decidable soft rules.
    pub soft_score: Option<f64>,
    /// Soft rules that could not be evaluated, i.e. unknown information.
    pub unknown_soft: Vec<String>,
    pub results: Vec<RuleResult>,
}

/// Evaluate a rule set for the ordered pair (a, b): a's perspective on partner b.
/// Run it in both directions for mutual compatibility.
pub fn evaluate_pair(set: &RuleSet, a: &Profile, b: &Profile) -> MatchOutcome {
    let mut eligible = true;
    let mut needs_info = vec![];
    let mut unknown_soft = vec![];
    let (mut got, mut possible) = (0.0, 0.0);
    let mut results = vec![];

    for r in &set.rules {
        let (res, trace) = evaluate(&r.expr, a, Some(b));
        match (r.kind, res) {
            (RuleKind::Hard, Tri::False) => eligible = false,
            (RuleKind::Hard, Tri::Unknown) => needs_info.push(r.id.clone()),
            (RuleKind::Soft, Tri::Unknown) => unknown_soft.push(r.id.clone()),
            (RuleKind::Soft, Tri::True) => { got += r.weight; possible += r.weight; }
            (RuleKind::Soft, Tri::False) => { possible += r.weight; }
            _ => {}
        }
        results.push(RuleResult { rule_id: r.id.clone(), kind: r.kind, result: res, trace });
    }

    MatchOutcome {
        rule_set: set.name.clone(),
        rule_set_version: set.version,
        eligible,
        needs_info,
        soft_score: if possible > 0.0 { Some(100.0 * got / possible) } else { None },
        unknown_soft,
        results,
    }
}
