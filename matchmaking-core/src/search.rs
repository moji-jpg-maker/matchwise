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
}

fn field(key: &str) -> Operand {
    Operand::Field { of: Side::A, key: key.to_string() }
}

fn lit(v: &Value) -> Operand {
    Operand::Lit { value: v.clone() }
}

/// Build a filter expression from form conditions (all must hold).
pub fn conditions_to_expr(conditions: &[Condition]) -> Result<Expr, String> {
    let mut args = Vec::with_capacity(conditions.len());
    for c in conditions {
        let need = |v: &Option<Value>| v.clone().ok_or_else(|| format!("condition on '{}' needs a value", c.field));
        let e = match c.op {
            ConditionOp::Exists => Expr::Exists { value: field(&c.field) },
            ConditionOp::Between => Expr::Between {
                value: field(&c.field),
                lo: lit(&need(&c.value)?),
                hi: lit(&need(&c.value2)?),
            },
            ConditionOp::In => {
                // `in` with a list means "any of"; build an OR of single-value tests so it works for
                // both single-choice and multi-choice fields.
                match need(&c.value)? {
                    Value::List(items) => Expr::Or {
                        args: items
                            .into_iter()
                            .map(|i| Expr::Cmp { left: field(&c.field), cmp: CmpOp::In, right: lit(&Value::Text(i)) })
                            .collect(),
                    },
                    v => Expr::Cmp { left: field(&c.field), cmp: CmpOp::In, right: lit(&v) },
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
                Expr::Cmp { left: field(&c.field), cmp, right: lit(&need(&c.value)?) }
            }
        };
        args.push(e);
    }
    Ok(Expr::And { args })
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
