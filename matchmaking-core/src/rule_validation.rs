//! Static checks for rule sets, run before a rule set is saved and live in the editor.
//! A rule set that passes cannot reference unknown fields, compare incompatible types, or be
//! so deeply nested that it becomes unreadable or slow.

use crate::expr::{CmpOp, Expr, Operand};
use crate::field::{FieldKind, FieldRegistry};
use crate::profile::Value;
use crate::rules::RuleSet;
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_RULES: usize = 200;
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 100;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RuleIssue {
    /// None for problems with the rule set itself.
    pub rule_id: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Num,
    Bool,
    Text,
    List,
}

impl Ty {
    fn name(self) -> &'static str {
        match self {
            Ty::Num => "a number",
            Ty::Bool => "yes/no",
            Ty::Text => "text",
            Ty::List => "a list",
        }
    }
}

fn operand_ty(op: &Operand, reg: &FieldRegistry) -> Result<Ty, String> {
    match op {
        Operand::Field { key, .. } => match reg.get(key).map(|d| &d.kind) {
            None => Err(format!("unknown field '{key}'")),
            Some(FieldKind::Number) => Ok(Ty::Num),
            Some(FieldKind::Bool) => Ok(Ty::Bool),
            Some(FieldKind::Text) | Some(FieldKind::Choice(_)) => Ok(Ty::Text),
            Some(FieldKind::MultiChoice(_)) => Ok(Ty::List),
            Some(FieldKind::Records(_)) => Err(format!("'{key}' holds repeating records and cannot be used in rules")),
        },
        Operand::Lit { value } => match value {
            Value::Num(n) if n.is_finite() => Ok(Ty::Num),
            Value::Num(_) => Err("number must be finite".into()),
            Value::Bool(_) => Ok(Ty::Bool),
            Value::Text(_) => Ok(Ty::Text),
            Value::List(_) => Ok(Ty::List),
            Value::Records(_) => Err("records cannot be used as a literal".into()),
        },
        Operand::Offset { base, by } => {
            if !by.is_finite() {
                return Err("offset must be a finite number".into());
            }
            match operand_ty(base, reg)? {
                Ty::Num => Ok(Ty::Num),
                other => Err(format!("an offset needs a number, found {}", other.name())),
            }
        }
    }
}

/// If `field_side` is a choice field and `lit_side` a text/list literal, every literal must be an allowed option.
fn check_options(field_side: &Operand, lit_side: &Operand, reg: &FieldRegistry) -> Result<(), String> {
    let (Operand::Field { key, .. }, Operand::Lit { value }) = (field_side, lit_side) else { return Ok(()) };
    let Some(def) = reg.get(key) else { return Ok(()) };
    let (FieldKind::Choice(opts) | FieldKind::MultiChoice(opts)) = &def.kind else { return Ok(()) };
    let lits: Vec<&String> = match value {
        Value::Text(t) => vec![t],
        Value::List(l) => l.iter().collect(),
        _ => vec![],
    };
    match lits.into_iter().find(|l| !opts.contains(l)) {
        Some(bad) => Err(format!("'{bad}' is not an option of '{}' (allowed: {})", def.label, opts.join(", "))),
        None => Ok(()),
    }
}

fn check_expr(e: &Expr, reg: &FieldRegistry, depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if *nodes > MAX_NODES {
        return Err(format!("too many conditions in one rule (max {MAX_NODES})"));
    }
    if depth > MAX_DEPTH {
        return Err(format!("conditions are nested too deeply (max {MAX_DEPTH} levels)"));
    }
    match e {
        Expr::And { args } | Expr::Or { args } => {
            if args.is_empty() {
                return Err("an AND/OR group needs at least one condition".into());
            }
            args.iter().try_for_each(|a| check_expr(a, reg, depth + 1, nodes))
        }
        Expr::Not { arg } => check_expr(arg, reg, depth + 1, nodes),
        Expr::If { when, then } => {
            check_expr(when, reg, depth + 1, nodes)?;
            check_expr(then, reg, depth + 1, nodes)
        }
        Expr::Exists { value } => match value {
            Operand::Field { .. } => operand_ty(value, reg).map(|_| ()),
            _ => Err("'is filled in' needs a field".into()),
        },
        Expr::Between { value, lo, hi } => {
            for (n, o) in [("value", value), ("lower bound", lo), ("upper bound", hi)] {
                let t = operand_ty(o, reg).map_err(|m| format!("{n}: {m}"))?;
                if t != Ty::Num {
                    return Err(format!("'between' needs numbers; the {n} is {}", t.name()));
                }
            }
            Ok(())
        }
        Expr::Cmp { left, cmp, right } => {
            let lt = operand_ty(left, reg).map_err(|m| format!("left side: {m}"))?;
            let rt = operand_ty(right, reg).map_err(|m| format!("right side: {m}"))?;
            match cmp {
                CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => {
                    if lt != Ty::Num || rt != Ty::Num {
                        return Err(format!("ordering comparisons need numbers, found {} and {}", lt.name(), rt.name()));
                    }
                }
                CmpOp::Eq | CmpOp::Ne => {
                    if lt != rt {
                        return Err(format!("cannot compare {} with {}", lt.name(), rt.name()));
                    }
                    if lt == Ty::List {
                        return Err("lists cannot be compared with = or ≠; use 'includes any of'".into());
                    }
                }
                CmpOp::In => {
                    let ok = matches!((lt, rt), (Ty::Text, Ty::Text) | (Ty::Text, Ty::List) | (Ty::List, Ty::Text) | (Ty::List, Ty::List));
                    if !ok {
                        return Err(format!("'is one of' needs text or lists, found {} and {}", lt.name(), rt.name()));
                    }
                }
            }
            check_options(left, right, reg)?;
            check_options(right, left, reg)
        }
    }
}

pub fn validate_ruleset(set: &RuleSet, reg: &FieldRegistry) -> Vec<RuleIssue> {
    let mut issues = vec![];
    let mut top = |m: String| issues.push(RuleIssue { rule_id: None, message: m });
    if set.name.trim().is_empty() || set.name.len() > 100 {
        top("The rule set needs a name (max 100 characters)".into());
    }
    if set.rules.len() > MAX_RULES {
        top(format!("Too many rules (max {MAX_RULES})"));
    }
    if let Some(m) = set.min_score {
        if !(m.is_finite() && (0.0..=100.0).contains(&m)) {
            top("The minimum score must be between 0 and 100".into());
        }
    }
    if let Some(m) = set.prior_score {
        if !(m.is_finite() && (0.0..=100.0).contains(&m)) {
            top("The neutral baseline score must be between 0 and 100".into());
        }
    }
    for (g, w) in &set.group_weights {
        if g.trim().is_empty() || !w.is_finite() || !(0.0..=10.0).contains(w) {
            top(format!("Group weight for '{g}' must be between 0 and 10"));
        }
    }
    let mut seen = BTreeSet::new();
    for r in &set.rules {
        let mut add = |m: String| issues.push(RuleIssue { rule_id: Some(r.id.clone()), message: m });
        if r.id.trim().is_empty() || r.id.len() > 64 {
            add("Each rule needs an id (max 64 characters)".into());
        } else if !seen.insert(r.id.clone()) {
            add("Duplicate rule id".into());
        }
        if r.description.trim().is_empty() {
            add("Describe the rule so matchmakers can read results".into());
        }
        if !(r.weight.is_finite() && (0.0..=100.0).contains(&r.weight)) {
            add("Weight must be between 0 and 100".into());
        }
        if !(-1000..=1000).contains(&r.priority) {
            add("Priority must be between -1000 and 1000".into());
        }
        if r.group.as_ref().map_or(false, |g| g.trim().is_empty() || g.len() > 50) {
            add("Group name must be 1-50 characters".into());
        }
        let mut nodes = 0;
        if let Err(m) = check_expr(&r.expr, reg, 0, &mut nodes) {
            add(format!("Condition: {m}"));
        }
        if let Some(w) = &r.when {
            let mut nodes = 0;
            if let Err(m) = check_expr(w, reg, 0, &mut nodes) {
                add(format!("'Applies when': {m}"));
            }
        }
    }
    issues
}
