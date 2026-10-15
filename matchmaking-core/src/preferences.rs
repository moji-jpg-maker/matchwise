//! Partner preferences: what a person (or their matchmaker) wants in a partner.
//!
//! A preference is a [`Condition`] about the *partner's* profile plus how strongly it matters.
//! Preferences compile to ordinary rules and run through the same engine, so they get the same
//! three-valued logic: missing data on the candidate is reported, never guessed.

use crate::expr::{Expr, Side};
use crate::field::{FieldKind, FieldRegistry};
use crate::locale::Locale;
use crate::profile::{Profile, Value};
use crate::rules::{evaluate_pair, MatchOutcome, Rule, RuleKind, RuleSet};
use crate::search::{condition_to_expr, Condition, ConditionOp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    /// The partner must satisfy the condition; otherwise the pair is excluded.
    Required,
    /// The condition describes something unacceptable; the pair is excluded if the partner matches it.
    DealBreaker,
    /// Counts towards the score with weight = importance.
    Preferred,
    /// Negotiable: counts towards the score with half weight.
    Flexible,
}

fn default_importance() -> u8 {
    3
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preference {
    /// Stable id (the app assigns one when empty).
    #[serde(default)]
    pub id: String,
    /// Condition on the partner's profile.
    pub condition: Condition,
    pub strength: Strength,
    /// Relative importance 1 (minor) to 5 (very important); used for `Preferred` and `Flexible`.
    #[serde(default = "default_importance")]
    pub importance: u8,
    /// Free-text note shown to the matchmaker.
    #[serde(default)]
    pub note: Option<String>,
}

impl Preference {
    /// Compile into a rule that reads the partner as `Side::B` and the owner as `Side::A`.
    pub fn to_rule(&self) -> Result<Rule, String> {
        let expr = condition_to_expr(&self.condition, Side::B, Some(Side::A))?;
        let description = self
            .note
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("partner {} {:?}", self.condition.field, self.condition.op));
        let (kind, expr, weight) = match self.strength {
            Strength::Required => (RuleKind::Hard, expr, 1.0),
            Strength::DealBreaker => (RuleKind::Hard, Expr::Not { arg: Box::new(expr) }, 1.0),
            Strength::Preferred => (RuleKind::Soft, expr, self.importance as f64),
            Strength::Flexible => (RuleKind::Soft, expr, self.importance as f64 * 0.5),
        };
        Ok(Rule::new(&self.id, &description, kind, weight, expr))
    }
}

fn kind_supports(kind: &FieldKind, op: ConditionOp) -> bool {
    use ConditionOp::*;
    match kind {
        FieldKind::Number => matches!(op, Eq | Ne | Lt | Le | Gt | Ge | Between | Exists),
        FieldKind::Text => matches!(op, Eq | Ne | Exists),
        FieldKind::Bool => matches!(op, Eq | Exists),
        FieldKind::Choice(_) => matches!(op, Eq | Ne | In | Exists),
        FieldKind::MultiChoice(_) => matches!(op, In | Exists),
        FieldKind::Records(_) => false,
    }
}

/// Check one preference against the field registry (before it is saved).
pub fn validate_preference(p: &Preference, registry: &FieldRegistry) -> Result<(), String> {
    validate_preference_for(p, registry, Locale::En)
}

/// Like [`validate_preference`], with messages in `locale`.
pub fn validate_preference_for(p: &Preference, registry: &FieldRegistry, locale: Locale) -> Result<(), String> {
    let c = &p.condition;
    let def = registry.get(&c.field).ok_or_else(|| match locale {
        Locale::En => format!("unknown field '{}'", c.field),
        Locale::Fa => format!("فیلد ناشناخته «{}»", c.field),
    })?;
    let label = def.label_for(locale);
    if !(1..=5).contains(&p.importance) {
        return Err(match locale {
            Locale::En => format!("'{label}': importance must be between 1 and 5"),
            Locale::Fa => format!("«{label}»: اهمیت باید بین ۱ تا ۵ باشد"),
        });
    }
    if !kind_supports(&def.kind, c.op) {
        return Err(match locale {
            Locale::En => format!("'{label}': this field does not support the '{:?}' condition", c.op).to_lowercase(),
            Locale::Fa => format!("«{label}»: این فیلد از شرط «{:?}» پشتیبانی نمی‌کند", c.op).to_lowercase(),
        });
    }
    // Relative values: numeric operators only, and they must point at a numeric field.
    for rel in [&c.value_rel, &c.value2_rel].into_iter().flatten() {
        if !matches!(def.kind, FieldKind::Number) {
            return Err(match locale {
                Locale::En => format!("'{label}': relative values only work on numeric fields"),
                Locale::Fa => format!("«{label}»: مقدار نسبی فقط برای فیلدهای عددی کار می‌کند"),
            });
        }
        match registry.get(&rel.field).map(|d| &d.kind) {
            Some(FieldKind::Number) => {}
            _ => return Err(match locale {
                Locale::En => format!("'{label}': relative to '{}', which is not a numeric field", rel.field),
                Locale::Fa => format!("«{label}»: نسبت به «{}» که فیلد عددی نیست", rel.field),
            }),
        }
        if !rel.offset.is_finite() {
            return Err(match locale {
                Locale::En => format!("'{label}': offset must be a finite number"),
                Locale::Fa => format!("«{label}»: اختلاف باید عدد متناهی باشد"),
            });
        }
    }
    let lit_ok = |v: &Option<Value>, rel: &Option<crate::search::RelativeValue>| -> Result<(), String> {
        if rel.is_some() {
            return Ok(());
        }
        let v = v.as_ref().ok_or_else(|| match locale {
            Locale::En => format!("'{label}': a value is required"),
            Locale::Fa => format!("«{label}»: مقدار لازم است"),
        })?;
        match (&def.kind, c.op, v) {
            (FieldKind::Choice(opts) | FieldKind::MultiChoice(opts), ConditionOp::In, Value::List(items)) => {
                if items.is_empty() {
                    return Err(match locale {
                        Locale::En => format!("'{label}': pick at least one option"),
                        Locale::Fa => format!("«{label}»: دست‌کم یک گزینه انتخاب کنید"),
                    });
                }
                match items.iter().find(|i| !opts.contains(i)) {
                    Some(bad) => Err(format!("«{label}»: {}", FieldKind::Choice(opts.clone()).validate_for(&Value::Text(bad.clone()), locale).unwrap_err())),
                    None => Ok(()),
                }
            }
            (FieldKind::Choice(opts) | FieldKind::MultiChoice(opts), ConditionOp::In, Value::Text(t)) => {
                if opts.contains(t) { Ok(()) } else { Err(format!("«{label}»: {}", FieldKind::Choice(opts.clone()).validate_for(&Value::Text(t.clone()), locale).unwrap_err())) }
            }
            (kind, _, v) => kind.validate_for(v, locale).map_err(|e| format!("«{label}»: {e}")),
        }
    };
    match c.op {
        ConditionOp::Exists => {}
        ConditionOp::Between => {
            lit_ok(&c.value, &c.value_rel)?;
            lit_ok(&c.value2, &c.value2_rel)?;
        }
        _ => lit_ok(&c.value, &c.value_rel)?,
    }
    Ok(())
}

/// Compile preferences into a rule set (preferences first become rules, then run through the rule engine).
pub fn preferences_to_ruleset(prefs: &[Preference], version: u32) -> Result<RuleSet, String> {
    let rules = prefs.iter().map(Preference::to_rule).collect::<Result<Vec<_>, _>>()?;
    Ok(RuleSet::new("partner_preferences", version, rules))
}

/// Evaluate `owner`'s preferences against a `candidate` partner.
pub fn evaluate_preferences(prefs: &[Preference], owner: &Profile, candidate: &Profile) -> Result<MatchOutcome, String> {
    let set = preferences_to_ruleset(prefs, 1)?;
    Ok(evaluate_pair(&set, owner, candidate))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MutualOutcome {
    pub a_wants_b: MatchOutcome,
    pub b_wants_a: MatchOutcome,
    /// Both directions are eligible (no failed hard preference either way).
    pub eligible: bool,
    /// Any hard preference in either direction could not be decided for lack of data.
    pub needs_info: bool,
    /// Mean of the available directional soft scores (0..=100); None if neither direction has one.
    pub score: Option<f64>,
}

pub fn evaluate_mutual(
    a_prefs: &[Preference],
    a: &Profile,
    b_prefs: &[Preference],
    b: &Profile,
) -> Result<MutualOutcome, String> {
    let a_wants_b = evaluate_preferences(a_prefs, a, b)?;
    let b_wants_a = evaluate_preferences(b_prefs, b, a)?;
    let scores: Vec<f64> = [a_wants_b.soft_score, b_wants_a.soft_score].into_iter().flatten().collect();
    Ok(MutualOutcome {
        eligible: a_wants_b.eligible && b_wants_a.eligible,
        needs_info: !a_wants_b.needs_info.is_empty() || !b_wants_a.needs_info.is_empty(),
        score: if scores.is_empty() { None } else { Some(scores.iter().sum::<f64>() / scores.len() as f64) },
        a_wants_b,
        b_wants_a,
    })
}

/// One-line, human wording for a stored preference (used where the editor is not available, for example in chat).
pub fn describe_preference(p: &Preference, registry: &FieldRegistry) -> String {
    describe_preference_for(p, registry, Locale::En)
}

/// [`describe_preference`] in the reader's language. The Persian wording is a draft for native review.
pub fn describe_preference_for(p: &Preference, registry: &FieldRegistry, locale: Locale) -> String {
    use crate::search::ConditionOp::*;
    let c = &p.condition;
    let label = registry.get(&c.field).map(|d| d.label_for(locale)).unwrap_or(c.field.as_str());
    let or_word = locale.pick(" or ", " یا ");
    let show = |v: &Option<Value>| match v {
        Some(Value::List(l)) => l.iter().map(|s| s.replace('_', " ")).collect::<Vec<_>>().join(or_word),
        Some(v) => crate::sharing::display_value_for(v, locale).unwrap_or_default(),
        None => String::new(),
    };
    let rel = |r: &Option<crate::search::RelativeValue>| {
        r.as_ref().map(|r| {
            let off = if r.offset == 0.0 { String::new() } else if r.offset > 0.0 { format!(" + {}", r.offset) } else { format!(" - {}", r.offset.abs()) };
            let name = registry.get(&r.field).map(|d| d.label_for(locale).to_lowercase()).unwrap_or_else(|| r.field.clone());
            match locale {
                Locale::En => format!("own {name}{off}"),
                Locale::Fa => format!("{name} خودتان{off}"),
            }
        })
    };
    let lo = rel(&c.value_rel).unwrap_or_else(|| show(&c.value));
    let hi = rel(&c.value2_rel).unwrap_or_else(|| show(&c.value2));
    let body = match (locale, c.op) {
        (Locale::En, Eq | In) => format!("{label} is {lo}"),
        (Locale::En, Ne) => format!("{label} is not {lo}"),
        (Locale::En, Lt) => format!("{label} is below {lo}"),
        (Locale::En, Le) => format!("{label} is at most {lo}"),
        (Locale::En, Gt) => format!("{label} is above {lo}"),
        (Locale::En, Ge) => format!("{label} is at least {lo}"),
        (Locale::En, Between) => format!("{label} between {lo} and {hi}"),
        (Locale::En, Exists) => format!("{label} is given"),
        (Locale::Fa, Eq | In) => format!("{label}: {lo}"),
        (Locale::Fa, Ne) => format!("{label}: غیر از {lo}"),
        (Locale::Fa, Lt) => format!("{label}: کمتر از {lo}"),
        (Locale::Fa, Le) => format!("{label}: حداکثر {lo}"),
        (Locale::Fa, Gt) => format!("{label}: بیشتر از {lo}"),
        (Locale::Fa, Ge) => format!("{label}: حداقل {lo}"),
        (Locale::Fa, Between) => format!("{label}: بین {lo} و {hi}"),
        (Locale::Fa, Exists) => format!("{label}: ذکر شده باشد"),
    };
    let body = crate::locale::localize_digits(locale, &body);
    let tag = match p.strength {
        Strength::Required => locale.pick("must have", "الزامی"),
        Strength::DealBreaker => locale.pick("deal-breaker", "غیرقابل‌گذشت"),
        Strength::Preferred => locale.pick("preferred", "ترجیحی"),
        Strength::Flexible => locale.pick("nice to have", "در صورت امکان"),
    };
    format!("{body} ({tag})")
}
