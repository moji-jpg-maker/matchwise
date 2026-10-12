//! Multi-dimension compatibility scorecard.
//!
//! A scorecard answers "how compatible, in which respects, and how sure are we?" rather than producing one
//! opaque percentage. Rules are assigned to dimensions through their `group` (a dimension key or alias);
//! both people's partner preferences form the `preferences` dimension; must-have rules and preferences form the
//! separate hard-constraints verdict.
//!
//! Dimensions nobody has configured rules for (for example personality and communication, which need
//! questionnaire data) are reported as `not_assessed` instead of being silently scored.
//!
//! Scores:
//! * `overall`: coverage-weighted mean of the dimension scores (dimension weight = rule-set `group_weights`, default 1).
//! * `confidence`: how much of the picture is backed by data (0..=1).
//! * `ranking_score`: `confidence * overall + (1 - confidence) * prior` (prior defaults to 50). A perfect score
//!   resting on little information is pulled towards neutral, so better-documented candidates rank higher.
//!   The rule-set threshold (`min_score`) applies to this score.

use crate::expr::{Expr, Operand, Side, Tri};
use crate::preferences::{evaluate_preferences, preferences_to_ruleset, Preference};
use crate::profile::Profile;
use crate::rules::{evaluate_ruleset_mutual, Direction, Rule, RuleKind, RuleResult, RuleSet};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct DimensionDef {
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    #[serde(skip)]
    pub aliases: &'static [&'static str],
}

pub const PREFERENCES: &str = "preferences";
pub const OTHER: &str = "other";

/// The default dimensions, in display order (from the product plan).
pub fn dimension_catalog() -> Vec<DimensionDef> {
    vec![
        DimensionDef { key: "age_life_stage", label: "Age / life stage", description: "Age gap and where each person is in life", aliases: &["age", "life_stage"] },
        DimensionDef { key: "values", label: "Values", description: "Shared core values (needs questionnaire data)", aliases: &[] },
        DimensionDef { key: "personality", label: "Personality", description: "Personality fit (needs questionnaire data)", aliases: &[] },
        DimensionDef { key: "family", label: "Family", description: "Family background and expectations", aliases: &["family_expectations"] },
        DimensionDef { key: "children", label: "Children", description: "Existing children and views on having children", aliases: &["kids"] },
        DimensionDef { key: "religion", label: "Religion", description: "Religion and level of observance", aliases: &["religious"] },
        DimensionDef { key: "lifestyle", label: "Lifestyle", description: "Daily habits: smoking, health, routines", aliases: &["habits"] },
        DimensionDef { key: "communication", label: "Communication", description: "Communication and conflict style (needs questionnaire data)", aliases: &[] },
        DimensionDef { key: "relationship_expectations", label: "Relationship expectations", description: "What each wants from the relationship", aliases: &["expectations"] },
        DimensionDef { key: "geography", label: "Geographic compatibility", description: "Where they live and are willing to live", aliases: &["location", "geo", "city"] },
        DimensionDef { key: "financial_practical", label: "Financial / practical", description: "Education, work, finances", aliases: &["financial", "practical", "education", "work"] },
        DimensionDef { key: PREFERENCES, label: "Personal preferences", description: "How well each fits the other's stated partner preferences", aliases: &["preference"] },
    ]
}

/// Map a rule `group` (key or alias, case-insensitive) to a canonical dimension key; anything else is `other`.
pub fn resolve_dimension(group: Option<&str>) -> String {
    let Some(g) = group.map(|g| g.trim().to_lowercase()).filter(|g| !g.is_empty()) else { return OTHER.into() };
    for d in dimension_catalog() {
        if d.key == g || d.aliases.iter().any(|a| *a == g) {
            return d.key.into();
        }
    }
    OTHER.into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DimStatus {
    /// Score 75 or more (and no violated must-have).
    Strong,
    /// Score 50 to 74.
    Mixed,
    /// Score below 50, or a violated must-have.
    Concern,
    /// No soft score; every applicable must-have holds.
    Clear,
    /// Rules exist but the data needed to judge them is missing.
    Unknown,
    /// Rules exist but none applies to this pair.
    NotApplicable,
    /// No rules or preferences measure this dimension at all.
    NotAssessed,
}

#[derive(Debug, Clone, Serialize)]
pub struct DimensionScore {
    pub key: String,
    pub label: String,
    pub description: String,
    pub score: Option<f64>,
    pub coverage: Option<f64>,
    pub status: DimStatus,
    pub weight: f64,
    /// Rules (or preferences) that applied to this pair in this dimension.
    pub checks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSource {
    Rule,
    /// A's preferences about B.
    PreferenceA,
    /// B's preferences about A.
    PreferenceB,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MissingField {
    /// "a" or "b": which person of the pair lacks the value.
    pub who: String,
    pub field: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub source: FindingSource,
    pub id: String,
    pub description: String,
    pub dimension: String,
    pub direction: Direction,
    pub kind: RuleKind,
    pub result: Tri,
    pub weight: f64,
    pub priority: i32,
    /// Profile fields whose absence prevents a decision (only for unknown results).
    pub missing: Vec<MissingField>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HardStatus {
    Pass,
    Fail,
    NeedsInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct HardConstraints {
    pub status: HardStatus,
    /// Must-haves that hold.
    pub passed: usize,
    /// Must-haves that fail or deal-breakers that match: possible deal-breakers.
    pub violations: Vec<Finding>,
    /// Must-haves that cannot be decided because information is missing.
    pub undecided: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScoreCard {
    pub rule_set: String,
    pub rule_set_version: u32,
    pub eligible: bool,
    pub hard_constraints: HardConstraints,
    pub dimensions: Vec<DimensionScore>,
    pub overall: Option<f64>,
    pub confidence: Option<f64>,
    pub ranking_score: Option<f64>,
    pub meets_threshold: Option<bool>,
    pub strengths: Vec<Finding>,
    pub concerns: Vec<Finding>,
    pub unknowns: Vec<Finding>,
}

const MAX_LIST: usize = 5;
const MAX_UNKNOWNS: usize = 15;

fn operand_fields(o: &Operand, out: &mut Vec<(Side, String)>) {
    match o {
        Operand::Field { of, key } => out.push((*of, key.clone())),
        Operand::Lit { .. } => {}
        Operand::Offset { base, .. } => operand_fields(base, out),
    }
}

fn expr_fields(e: &Expr, out: &mut Vec<(Side, String)>) {
    match e {
        Expr::And { args } | Expr::Or { args } => args.iter().for_each(|a| expr_fields(a, out)),
        Expr::Not { arg } => expr_fields(arg, out),
        Expr::If { when, then } => {
            expr_fields(when, out);
            expr_fields(then, out);
        }
        Expr::Exists { value } => operand_fields(value, out),
        Expr::Between { value, lo, hi } => {
            for o in [value, lo, hi] {
                operand_fields(o, out);
            }
        }
        Expr::Cmp { left, right, .. } => {
            operand_fields(left, out);
            operand_fields(right, out);
        }
    }
}

/// Fields a rule reads that are absent on the profile that plays that side. `swapped` means the rule was
/// evaluated with (B, A), so its `Side::A` is the pair's person "b".
fn missing_fields(rule: &Rule, eval_a: &Profile, eval_b: &Profile, swapped: bool) -> Vec<MissingField> {
    let mut used = vec![];
    expr_fields(&rule.expr, &mut used);
    if let Some(w) = &rule.when {
        expr_fields(w, &mut used);
    }
    let mut out: Vec<MissingField> = vec![];
    for (side, key) in used {
        let profile = eval_a_or_b(side, eval_a, eval_b);
        let who = match (side, swapped) {
            (Side::A, false) | (Side::B, true) => "a",
            (Side::B, false) | (Side::A, true) => "b",
        };
        if profile.get(&key).is_none() {
            let m = MissingField { who: who.into(), field: key };
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

fn eval_a_or_b<'a>(side: Side, a: &'a Profile, b: &'a Profile) -> &'a Profile {
    match side {
        Side::A => a,
        Side::B => b,
    }
}

#[derive(Default)]
struct Acc {
    configured: bool,
    got: f64,
    possible: f64,
    unknown_w: f64,
    hard_pass: usize,
    violations: usize,
    undecided: usize,
    checks: usize,
}

impl Acc {
    fn ingest(&mut self, r: &RuleResult) {
        if !r.applicable {
            return;
        }
        self.checks += 1;
        match (r.kind, r.result) {
            (RuleKind::Soft, Tri::True) => {
                self.got += r.weight;
                self.possible += r.weight;
            }
            (RuleKind::Soft, Tri::False) => self.possible += r.weight,
            (RuleKind::Soft, Tri::Unknown) => self.unknown_w += r.weight,
            (RuleKind::Hard, Tri::True) => self.hard_pass += 1,
            (RuleKind::Hard, Tri::False) => self.violations += 1,
            (RuleKind::Hard, Tri::Unknown) => self.undecided += 1,
        }
    }

    fn score(&self) -> Option<f64> {
        if self.possible > 0.0 { Some(100.0 * self.got / self.possible) } else { None }
    }

    /// Share of what could be checked that actually was. Each must-have counts 1; soft rules count their weight.
    fn coverage(&self) -> Option<f64> {
        let known = self.possible + (self.hard_pass + self.violations) as f64;
        let total = known + self.unknown_w + self.undecided as f64;
        if total > 0.0 { Some(known / total) } else { None }
    }

    fn status(&self) -> DimStatus {
        if !self.configured {
            return DimStatus::NotAssessed;
        }
        if self.checks == 0 {
            return DimStatus::NotApplicable;
        }
        if self.violations > 0 {
            return DimStatus::Concern;
        }
        match self.score() {
            Some(s) if s >= 75.0 => DimStatus::Strong,
            Some(s) if s >= 50.0 => DimStatus::Mixed,
            Some(_) => DimStatus::Concern,
            None if self.undecided > 0 || self.unknown_w > 0.0 => DimStatus::Unknown,
            None => DimStatus::Clear,
        }
    }
}

fn finding(
    source: FindingSource,
    r: &RuleResult,
    dimension: &str,
    rule: Option<&Rule>,
    eval: (&Profile, &Profile),
    swapped: bool,
) -> Finding {
    let missing = if r.result == Tri::Unknown {
        rule.map(|rule| missing_fields(rule, eval.0, eval.1, swapped)).unwrap_or_default()
    } else {
        vec![]
    };
    Finding {
        source,
        id: r.rule_id.clone(),
        description: r.description.clone(),
        dimension: dimension.to_string(),
        direction: r.direction,
        kind: r.kind,
        result: r.result,
        weight: r.weight,
        priority: r.priority,
        missing,
    }
}

pub fn score_pair(
    set: &RuleSet,
    a: &Profile,
    b: &Profile,
    a_prefs: &[Preference],
    b_prefs: &[Preference],
) -> Result<ScoreCard, String> {
    let rules_out = evaluate_ruleset_mutual(set, a, b);
    let a_out = evaluate_preferences(a_prefs, a, b)?;
    let b_out = evaluate_preferences(b_prefs, b, a)?;
    let a_rules = preferences_to_ruleset(a_prefs, 1)?;
    let b_rules = preferences_to_ruleset(b_prefs, 1)?;

    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    for r in set.rules.iter().filter(|r| r.enabled) {
        acc.entry(resolve_dimension(r.group.as_deref())).or_default().configured = true;
    }
    if !a_prefs.is_empty() || !b_prefs.is_empty() {
        acc.entry(PREFERENCES.into()).or_default().configured = true;
    }

    let mut all: Vec<Finding> = vec![];
    for r in rules_out.results.iter().filter(|r| r.applicable) {
        let rule = set.rules.iter().find(|x| x.id == r.rule_id);
        let dim = resolve_dimension(r.group.as_deref());
        acc.entry(dim.clone()).or_default().ingest(r);
        let swapped = r.direction == Direction::BToA;
        let eval = if swapped { (b, a) } else { (a, b) };
        all.push(finding(FindingSource::Rule, r, &dim, rule, eval, swapped));
    }
    for (src, out, rules, owner, partner, swapped) in [
        (FindingSource::PreferenceA, &a_out, &a_rules, a, b, false),
        (FindingSource::PreferenceB, &b_out, &b_rules, b, a, true),
    ] {
        for r in out.results.iter().filter(|r| r.applicable) {
            let rule = rules.rules.iter().find(|x| x.id == r.rule_id);
            acc.entry(PREFERENCES.into()).or_default().ingest(r);
            all.push(finding(src, r, PREFERENCES, rule, (owner, partner), swapped));
        }
    }

    // ---- hard constraints
    let violations: Vec<Finding> = all.iter().filter(|f| f.kind == RuleKind::Hard && f.result == Tri::False).cloned().collect();
    let undecided: Vec<Finding> = all.iter().filter(|f| f.kind == RuleKind::Hard && f.result == Tri::Unknown).cloned().collect();
    let passed = all.iter().filter(|f| f.kind == RuleKind::Hard && f.result == Tri::True).count();
    let hard_status = if !violations.is_empty() {
        HardStatus::Fail
    } else if !undecided.is_empty() {
        HardStatus::NeedsInfo
    } else {
        HardStatus::Pass
    };

    // ---- dimensions, in catalog order (then "other" if configured)
    let weights: BTreeMap<String, f64> = set.group_weights.iter().map(|(g, w)| (resolve_dimension(Some(g)), *w)).collect();
    let mut dims = vec![];
    let mut catalog: Vec<(String, String, String)> =
        dimension_catalog().into_iter().map(|d| (d.key.to_string(), d.label.to_string(), d.description.to_string())).collect();
    if acc.get(OTHER).map_or(false, |a| a.configured) {
        catalog.push((OTHER.into(), "Other".into(), "Rules without a dimension".into()));
    }
    for (key, label, description) in catalog {
        let empty = Acc::default();
        let a = acc.get(&key).unwrap_or(&empty);
        dims.push(DimensionScore {
            weight: weights.get(&key).copied().unwrap_or(1.0),
            score: a.score(),
            coverage: a.coverage(),
            status: a.status(),
            checks: a.checks,
            key,
            label,
            description,
        });
    }

    // ---- overall, confidence, ranking score
    let scored: Vec<&DimensionScore> = dims.iter().filter(|d| d.score.is_some() && d.weight > 0.0).collect();
    let denom: f64 = scored.iter().map(|d| d.weight * d.coverage.unwrap_or(0.0)).sum();
    let overall = if denom > 0.0 { Some(scored.iter().map(|d| d.weight * d.coverage.unwrap_or(0.0) * d.score.unwrap_or(0.0)).sum::<f64>() / denom) } else { None };
    let judged: Vec<&DimensionScore> = dims
        .iter()
        .filter(|d| !matches!(d.status, DimStatus::NotAssessed | DimStatus::NotApplicable) && d.weight > 0.0)
        .collect();
    let wsum: f64 = judged.iter().map(|d| d.weight).sum();
    let confidence = if wsum > 0.0 { Some(judged.iter().map(|d| d.weight * d.coverage.unwrap_or(0.0)).sum::<f64>() / wsum) } else { None };
    let prior = set.prior_score.unwrap_or(50.0);
    let ranking_score = overall.map(|o| {
        let c = confidence.unwrap_or(0.0);
        c * o + (1.0 - c) * prior
    });
    let meets_threshold = match (set.min_score, ranking_score) {
        (Some(min), Some(s)) => Some(s >= min),
        _ => None,
    };

    // ---- findings lists
    let by_weight = |x: &Finding, y: &Finding| y.weight.partial_cmp(&x.weight).unwrap_or(std::cmp::Ordering::Equal).then(y.priority.cmp(&x.priority));
    let mut strengths: Vec<Finding> = all.iter().filter(|f| f.kind == RuleKind::Soft && f.result == Tri::True).cloned().collect();
    strengths.sort_by(by_weight);
    strengths.truncate(MAX_LIST);
    let mut concerns: Vec<Finding> = all.iter().filter(|f| f.kind == RuleKind::Soft && f.result == Tri::False).cloned().collect();
    concerns.sort_by(by_weight);
    concerns.truncate(MAX_LIST);
    let mut unknowns: Vec<Finding> = all.iter().filter(|f| f.result == Tri::Unknown).cloned().collect();
    unknowns.sort_by(|x, y| (y.kind == RuleKind::Hard).cmp(&(x.kind == RuleKind::Hard)).then_with(|| by_weight(x, y)));
    unknowns.truncate(MAX_UNKNOWNS);

    Ok(ScoreCard {
        rule_set: set.name.clone(),
        rule_set_version: set.version,
        eligible: violations.is_empty() && meets_threshold != Some(false),
        hard_constraints: HardConstraints { status: hard_status, passed, violations, undecided },
        dimensions: dims,
        overall,
        confidence,
        ranking_score,
        meets_threshold,
        strengths,
        concerns,
        unknowns,
    })
}
