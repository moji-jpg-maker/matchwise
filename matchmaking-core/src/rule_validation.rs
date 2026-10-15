//! Static checks for rule sets, run before a rule set is saved and live in the editor.
//! A rule set that passes cannot reference unknown fields, compare incompatible types, or be
//! so deeply nested that it becomes unreadable or slow.

use crate::expr::{CmpOp, Expr, Operand};
use crate::field::{FieldKind, FieldRegistry};
use crate::locale::{localize_digits, Locale};
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
    fn name(self, locale: Locale) -> &'static str {
        match self {
            Ty::Num => locale.pick("a number", "عدد"),
            Ty::Bool => locale.pick("yes/no", "بله/خیر"),
            Ty::Text => locale.pick("text", "متن"),
            Ty::List => locale.pick("a list", "فهرست"),
        }
    }
}

fn operand_ty(op: &Operand, reg: &FieldRegistry, locale: Locale) -> Result<Ty, String> {
    match op {
        Operand::Field { key, .. } => match reg.get(key).map(|d| &d.kind) {
            None => Err(match locale {
                Locale::En => format!("unknown field '{key}'"),
                Locale::Fa => format!("فیلد ناشناخته «{key}»"),
            }),
            Some(FieldKind::Number) => Ok(Ty::Num),
            Some(FieldKind::Bool) => Ok(Ty::Bool),
            Some(FieldKind::Text) | Some(FieldKind::Choice(_)) => Ok(Ty::Text),
            Some(FieldKind::MultiChoice(_)) => Ok(Ty::List),
            Some(FieldKind::Records(_)) => Err(match locale {
                Locale::En => format!("'{key}' holds repeating records and cannot be used in rules"),
                Locale::Fa => format!("«{key}» شامل رکوردهای تکرارشونده است و در قوانین قابل استفاده نیست"),
            }),
        },
        Operand::Lit { value } => match value {
            Value::Num(n) if n.is_finite() => Ok(Ty::Num),
            Value::Num(_) => Err(locale.pick("number must be finite", "عدد باید متناهی باشد").into()),
            Value::Bool(_) => Ok(Ty::Bool),
            Value::Text(_) => Ok(Ty::Text),
            Value::List(_) => Ok(Ty::List),
            Value::Records(_) => Err(locale.pick("records cannot be used as a literal", "رکوردها را نمی‌توان به‌صورت مقدار ثابت به کار برد").into()),
        },
        Operand::Offset { base, by } => {
            if !by.is_finite() {
                return Err(locale.pick("offset must be a finite number", "مقدار انحراف باید عددی متناهی باشد").into());
            }
            match operand_ty(base, reg, locale)? {
                Ty::Num => Ok(Ty::Num),
                other => Err(match locale {
                    Locale::En => format!("an offset needs a number, found {}", other.name(locale)),
                    Locale::Fa => format!("انحراف به عدد نیاز دارد، اما {} یافت شد", other.name(locale)),
                }),
            }
        }
    }
}

/// If `field_side` is a choice field and `lit_side` a text/list literal, every literal must be an allowed option.
fn check_options(field_side: &Operand, lit_side: &Operand, reg: &FieldRegistry, locale: Locale) -> Result<(), String> {
    let (Operand::Field { key, .. }, Operand::Lit { value }) = (field_side, lit_side) else { return Ok(()) };
    let Some(def) = reg.get(key) else { return Ok(()) };
    let (FieldKind::Choice(opts) | FieldKind::MultiChoice(opts)) = &def.kind else { return Ok(()) };
    let lits: Vec<&String> = match value {
        Value::Text(t) => vec![t],
        Value::List(l) => l.iter().collect(),
        _ => vec![],
    };
    match lits.into_iter().find(|l| !opts.contains(l)) {
        Some(bad) => Err(match locale {
            Locale::En => format!("'{bad}' is not an option of '{}' (allowed: {})", def.label_for(locale), opts.join(", ")),
            Locale::Fa => format!("«{bad}» جزو گزینه‌های «{}» نیست (مجاز: {})", def.label_for(locale), opts.join("، ")),
        }),
        None => Ok(()),
    }
}

fn check_expr(e: &Expr, reg: &FieldRegistry, locale: Locale, depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if *nodes > MAX_NODES {
        return Err(match locale {
            Locale::En => format!("too many conditions in one rule (max {MAX_NODES})"),
            Locale::Fa => format!("شرط‌های یک قانون بیش از حد است (حداکثر {})", localize_digits(locale, &MAX_NODES.to_string())),
        });
    }
    if depth > MAX_DEPTH {
        return Err(match locale {
            Locale::En => format!("conditions are nested too deeply (max {MAX_DEPTH} levels)"),
            Locale::Fa => format!("شرط‌ها بیش از حد تودرتو هستند (حداکثر {} سطح)", localize_digits(locale, &MAX_DEPTH.to_string())),
        });
    }
    match e {
        Expr::And { args } | Expr::Or { args } => {
            if args.is_empty() {
                return Err(locale.pick("an AND/OR group needs at least one condition", "گروه «و/یا» دست‌کم به یک شرط نیاز دارد").into());
            }
            args.iter().try_for_each(|a| check_expr(a, reg, locale, depth + 1, nodes))
        }
        Expr::Not { arg } => check_expr(arg, reg, locale, depth + 1, nodes),
        Expr::If { when, then } => {
            check_expr(when, reg, locale, depth + 1, nodes)?;
            check_expr(then, reg, locale, depth + 1, nodes)
        }
        Expr::Exists { value } => match value {
            Operand::Field { .. } => operand_ty(value, reg, locale).map(|_| ()),
            _ => Err(locale.pick("'is filled in' needs a field", "«پر شده است» به یک فیلد نیاز دارد").into()),
        },
        Expr::Between { value, lo, hi } => {
            let parts = [
                (locale.pick("value", "مقدار"), value),
                (locale.pick("lower bound", "کران پایین"), lo),
                (locale.pick("upper bound", "کران بالا"), hi),
            ];
            for (n, o) in parts {
                let t = operand_ty(o, reg, locale).map_err(|m| format!("{n}: {m}"))?;
                if t != Ty::Num {
                    return Err(match locale {
                        Locale::En => format!("'between' needs numbers; the {n} is {}", t.name(locale)),
                        Locale::Fa => format!("«بین» به عدد نیاز دارد؛ {n} از نوع {} است", t.name(locale)),
                    });
                }
            }
            Ok(())
        }
        Expr::Cmp { left, cmp, right } => {
            let lt = operand_ty(left, reg, locale).map_err(|m| format!("{}: {m}", locale.pick("left side", "سمت چپ")))?;
            let rt = operand_ty(right, reg, locale).map_err(|m| format!("{}: {m}", locale.pick("right side", "سمت راست")))?;
            match cmp {
                CmpOp::Lt | CmpOp::Le | CmpOp::Gt | CmpOp::Ge => {
                    if lt != Ty::Num || rt != Ty::Num {
                        return Err(match locale {
                            Locale::En => format!("ordering comparisons need numbers, found {} and {}", lt.name(locale), rt.name(locale)),
                            Locale::Fa => format!("مقایسه‌های ترتیبی به عدد نیاز دارند، اما {} و {} یافت شد", lt.name(locale), rt.name(locale)),
                        });
                    }
                }
                CmpOp::Eq | CmpOp::Ne => {
                    if lt != rt {
                        return Err(match locale {
                            Locale::En => format!("cannot compare {} with {}", lt.name(locale), rt.name(locale)),
                            Locale::Fa => format!("نمی‌توان {} را با {} مقایسه کرد", lt.name(locale), rt.name(locale)),
                        });
                    }
                    if lt == Ty::List {
                        return Err(locale.pick("lists cannot be compared with = or ≠; use 'includes any of'", "فهرست‌ها را نمی‌توان با = یا ≠ مقایسه کرد؛ از «شامل هر یک از» استفاده کنید").into());
                    }
                }
                CmpOp::In => {
                    let ok = matches!((lt, rt), (Ty::Text, Ty::Text) | (Ty::Text, Ty::List) | (Ty::List, Ty::Text) | (Ty::List, Ty::List));
                    if !ok {
                        return Err(match locale {
                            Locale::En => format!("'is one of' needs text or lists, found {} and {}", lt.name(locale), rt.name(locale)),
                            Locale::Fa => format!("«یکی از» به متن یا فهرست نیاز دارد، اما {} و {} یافت شد", lt.name(locale), rt.name(locale)),
                        });
                    }
                }
            }
            check_options(left, right, reg, locale)?;
            check_options(right, left, reg, locale)
        }
    }
}

pub fn validate_ruleset(set: &RuleSet, reg: &FieldRegistry) -> Vec<RuleIssue> {
    validate_ruleset_for(set, reg, Locale::En)
}

pub fn validate_ruleset_for(set: &RuleSet, reg: &FieldRegistry, locale: Locale) -> Vec<RuleIssue> {
    let mut issues = vec![];
    let mut top = |m: String| issues.push(RuleIssue { rule_id: None, message: m });
    if set.name.trim().is_empty() || set.name.len() > 100 {
        top(locale.pick("The rule set needs a name (max 100 characters)", "مجموعه قوانین به نام نیاز دارد (حداکثر ۱۰۰ نویسه)").into());
    }
    if set.rules.len() > MAX_RULES {
        top(match locale {
            Locale::En => format!("Too many rules (max {MAX_RULES})"),
            Locale::Fa => format!("تعداد قوانین بیش از حد است (حداکثر {})", localize_digits(locale, &MAX_RULES.to_string())),
        });
    }
    if let Some(m) = set.min_score {
        if !(m.is_finite() && (0.0..=100.0).contains(&m)) {
            top(locale.pick("The minimum score must be between 0 and 100", "حداقل امتیاز باید بین ۰ تا ۱۰۰ باشد").into());
        }
    }
    for (g, w) in &set.group_weights {
        if g.trim().is_empty() || !w.is_finite() || !(0.0..=10.0).contains(w) {
            top(match locale {
                Locale::En => format!("Group weight for '{g}' must be between 0 and 10"),
                Locale::Fa => format!("وزن گروه «{g}» باید بین ۰ تا ۱۰ باشد"),
            });
        }
    }
    let mut seen = BTreeSet::new();
    for r in &set.rules {
        let mut add = |m: String| issues.push(RuleIssue { rule_id: Some(r.id.clone()), message: m });
        if r.id.trim().is_empty() || r.id.len() > 64 {
            add(locale.pick("Each rule needs an id (max 64 characters)", "هر قانون به شناسه نیاز دارد (حداکثر ۶۴ نویسه)").into());
        } else if !seen.insert(r.id.clone()) {
            add(locale.pick("Duplicate rule id", "شناسه قانون تکراری است").into());
        }
        if r.description.trim().is_empty() {
            add(locale.pick("Describe the rule so matchmakers can read results", "برای اینکه همسان‌یاب‌ها بتوانند نتایج را بخوانند، قانون را توصیف کنید").into());
        }
        if !(r.weight.is_finite() && (0.0..=100.0).contains(&r.weight)) {
            add(locale.pick("Weight must be between 0 and 100", "وزن باید بین ۰ تا ۱۰۰ باشد").into());
        }
        if !(-1000..=1000).contains(&r.priority) {
            add(locale.pick("Priority must be between -1000 and 1000", "اولویت باید بین ‎-۱۰۰۰ تا ۱۰۰۰ باشد").into());
        }
        if r.group.as_ref().map_or(false, |g| g.trim().is_empty() || g.len() > 50) {
            add(locale.pick("Group name must be 1-50 characters", "نام گروه باید ۱ تا ۵۰ نویسه باشد").into());
        }
        let mut nodes = 0;
        if let Err(m) = check_expr(&r.expr, reg, locale, 0, &mut nodes) {
            add(format!("{}: {m}", locale.pick("Condition", "شرط")));
        }
        if let Some(w) = &r.when {
            let mut nodes = 0;
            if let Err(m) = check_expr(w, reg, locale, 0, &mut nodes) {
                add(format!("{}: {m}", locale.pick("'Applies when'", "«اعمال شود وقتی»")));
            }
        }
    }
    issues
}
