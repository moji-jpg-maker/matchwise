//! Data-driven expression tree used by both candidate search filters and rules.
//! Uses three-valued logic so missing information yields `Unknown`, never a silent pass/fail.

use crate::profile::{Profile, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    fn not(self) -> Tri {
        match self { Tri::True => Tri::False, Tri::False => Tri::True, Tri::Unknown => Tri::Unknown }
    }
}

/// Which profile an operand reads from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operand {
    Field { of: Side, key: String },
    Lit { value: Value },
    /// `base + by` (numeric), e.g. "partner age <= my age + 8".
    Offset { base: Box<Operand>, by: f64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// right is a list/text; left value is contained in it (or list contains left).
    In,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Expr {
    And { args: Vec<Expr> },
    Or { args: Vec<Expr> },
    Not { arg: Box<Expr> },
    Cmp { left: Operand, cmp: CmpOp, right: Operand },
    Between { value: Operand, lo: Operand, hi: Operand },
    Exists { value: Operand },
    /// Conditional: if `when` is true then `then` must hold; if `when` is false the rule is vacuously true.
    If { when: Box<Expr>, then: Box<Expr> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub node: String,
    pub result: Tri,
    pub children: Vec<Trace>,
}

fn leaf(node: impl Into<String>, result: Tri) -> (Tri, Trace) {
    let node = node.into();
    (result, Trace { node, result, children: vec![] })
}

fn resolve(op: &Operand, a: &Profile, b: Option<&Profile>) -> Option<Value> {
    match op {
        Operand::Lit { value } => Some(value.clone()),
        Operand::Field { of, key } => {
            let p = match of { Side::A => Some(a), Side::B => b }?;
            p.get(key).cloned()
        }
        Operand::Offset { base, by } => match resolve(base, a, b)? {
            Value::Num(n) => Some(Value::Num(n + by)),
            _ => None,
        },
    }
}

fn compare(l: &Value, op: CmpOp, r: &Value) -> Tri {
    use CmpOp::*;
    let b = |x: bool| if x { Tri::True } else { Tri::False };
    match (l, r) {
        (Value::Num(x), Value::Num(y)) => match op {
            Eq => b(x == y), Ne => b(x != y), Lt => b(x < y), Le => b(x <= y),
            Gt => b(x > y), Ge => b(x >= y), In => Tri::Unknown,
        },
        (Value::Text(x), Value::Text(y)) => match op {
            Eq => b(x.eq_ignore_ascii_case(y)), Ne => b(!x.eq_ignore_ascii_case(y)),
            In => b(y.to_lowercase().contains(&x.to_lowercase())),
            _ => Tri::Unknown,
        },
        (Value::Bool(x), Value::Bool(y)) => match op {
            Eq => b(x == y), Ne => b(x != y), _ => Tri::Unknown,
        },
        (Value::Text(x), Value::List(ys)) => match op {
            In => b(ys.iter().any(|y| y.eq_ignore_ascii_case(x))),
            _ => Tri::Unknown,
        },
        (Value::List(xs), Value::List(ys)) => match op {
            // any overlap
            In => b(xs.iter().any(|x| ys.iter().any(|y| y.eq_ignore_ascii_case(x)))),
            _ => Tri::Unknown,
        },
        _ => Tri::Unknown, // type mismatch is treated as unknown, not as a pass or fail
    }
}

/// Evaluate an expression against profile `a` (and optionally partner `b`).
pub fn evaluate(expr: &Expr, a: &Profile, b: Option<&Profile>) -> (Tri, Trace) {
    match expr {
        Expr::And { args } => {
            let rs: Vec<(Tri, Trace)> = args.iter().map(|e| evaluate(e, a, b)).collect();
            let res = if rs.iter().any(|(t, _)| *t == Tri::False) { Tri::False }
                else if rs.iter().any(|(t, _)| *t == Tri::Unknown) { Tri::Unknown }
                else { Tri::True };
            (res, Trace { node: "and".into(), result: res, children: rs.into_iter().map(|(_, t)| t).collect() })
        }
        Expr::Or { args } => {
            let rs: Vec<(Tri, Trace)> = args.iter().map(|e| evaluate(e, a, b)).collect();
            let res = if rs.iter().any(|(t, _)| *t == Tri::True) { Tri::True }
                else if rs.iter().any(|(t, _)| *t == Tri::Unknown) { Tri::Unknown }
                else { Tri::False };
            (res, Trace { node: "or".into(), result: res, children: rs.into_iter().map(|(_, t)| t).collect() })
        }
        Expr::Not { arg } => {
            let (t, tr) = evaluate(arg, a, b);
            let res = t.not();
            (res, Trace { node: "not".into(), result: res, children: vec![tr] })
        }
        Expr::If { when, then } => {
            let (w, wt) = evaluate(when, a, b);
            match w {
                Tri::False => (Tri::True, Trace { node: "if(vacuous)".into(), result: Tri::True, children: vec![wt] }),
                Tri::Unknown => (Tri::Unknown, Trace { node: "if(condition unknown)".into(), result: Tri::Unknown, children: vec![wt] }),
                Tri::True => {
                    let (t, tt) = evaluate(then, a, b);
                    (t, Trace { node: "if".into(), result: t, children: vec![wt, tt] })
                }
            }
        }
        Expr::Exists { value } => {
            let r = if resolve(value, a, b).is_some() { Tri::True } else { Tri::False };
            leaf(format!("exists {:?}", value), r)
        }
        Expr::Cmp { left, cmp, right } => match (resolve(left, a, b), resolve(right, a, b)) {
            (Some(l), Some(r)) => leaf(format!("{:?} {:?} {:?}", left, cmp, right), compare(&l, *cmp, &r)),
            _ => leaf(format!("missing data: {:?} {:?} {:?}", left, cmp, right), Tri::Unknown),
        },
        Expr::Between { value, lo, hi } => {
            match (resolve(value, a, b), resolve(lo, a, b), resolve(hi, a, b)) {
                (Some(v), Some(l), Some(h)) => {
                    let ge = compare(&v, CmpOp::Ge, &l);
                    let le = compare(&v, CmpOp::Le, &h);
                    let res = match (ge, le) {
                        (Tri::True, Tri::True) => Tri::True,
                        (Tri::False, _) | (_, Tri::False) => Tri::False,
                        _ => Tri::Unknown,
                    };
                    leaf(format!("between {:?}", value), res)
                }
                _ => leaf(format!("missing data: between {:?}", value), Tri::Unknown),
            }
        }
    }
}
