//! Deterministic candidate search. Filters are the same expression trees the rule engine uses,
//! so a saved search can later become a rule and vice versa.

use crate::expr::{evaluate, CmpOp, Expr, Operand, Side, Tri};
use crate::profile::{Profile, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Field value is one of the given list, or (for multi-choice fields) contains the given value.
    In,
    Between,
    Exists,
}

/// A value defined relative to the preference owner's own field, e.g. "own age + 8".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RelativeValue {
    pub field: String,
    #[serde(default)]
    pub offset: f64,
}

/// One row of the search form, e.g. `age between 27 and 34`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    pub field: String,
    pub op: ConditionOp,
    #[serde(default)]
    pub value: Option<Value>,
    /// Upper bound for `between`.
    #[serde(default)]
    pub value2: Option<Value>,
    /// If set, replaces `value` with "<owner's field> + offset". Only valid in partner preferences.
    #[serde(default)]
    pub value_rel: Option<RelativeValue>,
    /// If set, replaces `value2` (upper bound) likewise.
    #[serde(default)]
    pub value2_rel: Option<RelativeValue>,
}

fn lit_operand(v: &Value) -> Operand {
    Operand::Lit { value: v.clone() }
}

/// Build a filter expression from form conditions (all must hold). Relative values are not allowed here
/// because a plain search has no "owner" profile to be relative to.
pub fn conditions_to_expr(conditions: &[Condition]) -> Result<Expr, String> {
    let mut args = Vec::with_capacity(conditions.len());
    for c in conditions {
        args.push(condition_to_expr(c, Side::A, None)?);
    }
    Ok(Expr::And { args })
}

/// Build the expression for one condition. `subject` is the profile whose field is tested; `owner` is the
/// profile that relative values refer to (the preference owner), if any.
pub fn condition_to_expr(c: &Condition, subject: Side, owner: Option<Side>) -> Result<Expr, String> {
    let subj = Operand::Field { of: subject, key: c.field.clone() };
    let operand = |lit: &Option<Value>, rel: &Option<RelativeValue>| -> Result<Operand, String> {
        if let Some(r) = rel {
            let own = owner.ok_or_else(|| format!("'{}': relative values are only valid in partner preferences", c.field))?;
            return Ok(Operand::Offset {
                base: Box::new(Operand::Field { of: own, key: r.field.clone() }),
                by: r.offset,
            });
        }
        lit.as_ref()
            .map(lit_operand)
            .ok_or_else(|| format!("condition on '{}' needs a value", c.field))
    };
    Ok(match c.op {
        ConditionOp::Exists => Expr::Exists { value: subj },
        ConditionOp::Between => Expr::Between {
            value: subj,
            lo: operand(&c.value, &c.value_rel)?,
            hi: operand(&c.value2, &c.value2_rel)?,
        },
        ConditionOp::In => {
            // `in` with a list means "any of"; build an OR of single-value tests so it works for
            // both single-choice and multi-choice fields.
            match c.value.clone().ok_or_else(|| format!("condition on '{}' needs a value", c.field))? {
                Value::List(items) => Expr::Or {
                    args: items
                        .into_iter()
                        .map(|i| Expr::Cmp { left: subj.clone(), cmp: CmpOp::In, right: lit_operand(&Value::Text(i)) })
                        .collect(),
                },
                v => Expr::Cmp { left: subj, cmp: CmpOp::In, right: lit_operand(&v) },
            }
        }
        op => {
            let cmp = match op {
                ConditionOp::Eq => CmpOp::Eq,
                ConditionOp::Ne => CmpOp::Ne,
                ConditionOp::Lt => CmpOp::Lt,
                ConditionOp::Le => CmpOp::Le,
                ConditionOp::Gt => CmpOp::Gt,
                ConditionOp::Ge => CmpOp::Ge,
                _ => unreachable!(),
            };
            Expr::Cmp { left: subj, cmp, right: operand(&c.value, &c.value_rel)? }
        }
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    pub profile_id: String,
    /// `True` = definitely matches; `Unknown` = some filtered field is missing on the profile.
    pub result: Tri,
}

/// Run a filter over profiles. Profiles that definitely fail are dropped; profiles with missing
/// data are returned as `Unknown` only when `include_unknown` is set, so a matchmaker can choose
/// between strict results and "might match, ask for more information".
pub fn search(profiles: &[Profile], filter: &Expr, include_unknown: bool) -> Vec<SearchHit> {
    profiles
        .iter()
        .filter_map(|p| {
            let (r, _) = evaluate(filter, p, None);
            match r {
                Tri::True => Some(SearchHit { profile_id: p.id.clone(), result: r }),
                Tri::Unknown if include_unknown => Some(SearchHit { profile_id: p.id.clone(), result: r }),
                _ => None,
            }
        })
        .collect()
}
