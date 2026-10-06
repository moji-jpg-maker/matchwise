//! Starter field set from the product plan. Matchmakers can add, edit, or hide fields at runtime;
//! this is only the seed used for a new organization.

use crate::expr::{CmpOp, Expr, Operand, Side};
use crate::field::{FieldDef, FieldKind, FieldRegistry};
use crate::profile::Value;
use crate::rules::{Rule, RuleKind, RuleScope, RuleSet};

fn f(key: &str, label: &str, kind: FieldKind, sensitive: bool, required: bool) -> FieldDef {
    FieldDef { key: key.into(), label: label.into(), kind, sensitive, required }
}

fn choice(opts: &[&str]) -> FieldKind {
    FieldKind::Choice(opts.iter().map(|s| s.to_string()).collect())
}

pub fn default_registry() -> FieldRegistry {
    use FieldKind::*;
    let mut r = FieldRegistry::new();
    for d in [
        f("full_name", "Full name", Text, true, true),
        f("gender", "Gender", choice(&["female", "male"]), false, true),
        f("age", "Age", Number, false, true),
        f("date_of_birth", "Date of birth (YYYY-MM-DD)", Text, true, false),
        f("height_cm", "Height (cm)", Number, false, false),
        f("weight_kg", "Weight (kg)", Number, true, false),
        f("birthplace", "Birthplace", Text, false, false),
        f("city", "Current city", Text, false, true),
        f("province", "Province / region", Text, false, false),
        f("family_origin", "Family origin", Text, false, false),
        f("education", "Education level", choice(&["secondary", "diploma", "bachelor", "master", "doctorate", "other"]), false, true),
        f("occupation", "Occupation", Text, false, true),
        f("employment_status", "Employment status", choice(&["employed", "self_employed", "student", "unemployed", "retired"]), false, false),
        f("financial_situation", "Financial situation", Text, true, false),
        f("religion", "Religion", Text, true, false),
        f("religiosity", "Religious observance", choice(&["low", "moderate", "high"]), true, false),
        f("marital_status", "Marriage history", choice(&["never_married", "divorced", "widowed", "separated"]), false, true),
        f("has_children", "Has children", Bool, false, true),
        f("children_count", "Number of children", Number, false, false),
        f("children", "Children (details)", Records(vec![
            f("gender", "Gender", choice(&["female", "male"]), false, false),
            f("age", "Age", Number, false, false),
            f("custody", "Custody", choice(&["with_me", "shared", "with_other_parent", "other"]), false, false),
            f("living_arrangement", "Currently living", choice(&["with_me", "with_other_parent", "boarding_or_school", "independent", "other"]), false, false),
            f("notes", "Other circumstances", Text, false, false),
        ]), true, false),
        f("accepts_children", "Accepts a partner's children", Bool, false, false),
        f("siblings_count", "Number of siblings", Number, false, false),
        f("father_occupation", "Father's occupation", Text, false, false),
        f("mother_occupation", "Mother's occupation", Text, false, false),
        f("smoking", "Smoking", choice(&["never", "occasionally", "regularly"]), false, false),
        f("health_notes", "Health information", Text, true, false),
        f("lifestyle_notes", "Lifestyle", Text, false, false),
        f("about", "About / free-text profile", Text, false, false),
        f("telegram_username", "Telegram username", Text, true, false),
        f("phone", "Primary phone", Text, true, false),
    ] {
        r.register(d);
    }
    r
}

fn fld(of: Side, key: &str) -> Operand {
    Operand::Field { of, key: key.into() }
}

fn lit(v: Value) -> Operand {
    Operand::Lit { value: v }
}

fn eq(l: Operand, r: Operand) -> Expr {
    Expr::Cmp { left: l, cmp: CmpOp::Eq, right: r }
}

/// Starter rule set shown to a new organization. It demonstrates the rule features (conditional rules,
/// directional rules, groups, an example that is switched off) and is meant to be edited.
pub fn default_ruleset() -> RuleSet {
    let mut kids = Rule::new(
        "kids",
        "If one person has children, the partner must accept children",
        RuleKind::Hard,
        1.0,
        eq(fld(Side::B, "accepts_children"), lit(Value::Bool(true))),
    );
    kids.when = Some(eq(fld(Side::A, "has_children"), lit(Value::Bool(true))));
    kids.scope = RuleScope::Directional;
    kids.group = Some("children".into());
    kids.priority = 100;

    let mut age_gap = Rule::new(
        "age_gap",
        "Ages within 8 years of each other",
        RuleKind::Soft,
        2.0,
        Expr::Between {
            value: fld(Side::B, "age"),
            lo: Operand::Offset { base: Box::new(fld(Side::A, "age")), by: -8.0 },
            hi: Operand::Offset { base: Box::new(fld(Side::A, "age")), by: 8.0 },
        },
    );
    age_gap.group = Some("age_life_stage".into());
    age_gap.priority = 20;

    let mut city = Rule::new("same_city", "Live in the same city", RuleKind::Soft, 2.0, eq(fld(Side::A, "city"), fld(Side::B, "city")));
    city.group = Some("geography".into());
    city.priority = 10;

    let mut religion = Rule::new(
        "religion_aligned",
        "Same religion when both are highly observant",
        RuleKind::Soft,
        3.0,
        eq(fld(Side::A, "religion"), fld(Side::B, "religion")),
    );
    religion.when = Some(Expr::And {
        args: vec![
            eq(fld(Side::A, "religiosity"), lit(Value::Text("high".into()))),
            eq(fld(Side::B, "religiosity"), lit(Value::Text("high".into()))),
        ],
    });
    religion.group = Some("religion".into());
    religion.priority = 30;

    // Example from the product plan, disabled by default because it excludes many pairs.
    let mut window = Rule::new(
        "age_window_example",
        "Example: when a candidate is 25-30, the partner must be 0-8 years older",
        RuleKind::Hard,
        1.0,
        Expr::Between {
            value: fld(Side::B, "age"),
            lo: fld(Side::A, "age"),
            hi: Operand::Offset { base: Box::new(fld(Side::A, "age")), by: 8.0 },
        },
    );
    window.when = Some(Expr::Between { value: fld(Side::A, "age"), lo: lit(Value::Num(25.0)), hi: lit(Value::Num(30.0)) });
    window.scope = RuleScope::Directional;
    window.group = Some("age_life_stage".into());
    window.enabled = false;

    let mut observance = Rule::new(
        "religiosity_close",
        "Similar level of religious observance",
        RuleKind::Soft,
        2.0,
        eq(fld(Side::A, "religiosity"), fld(Side::B, "religiosity")),
    );
    observance.group = Some("religion".into());
    observance.priority = 25;

    let mut smoking = Rule::new("smoking_match", "Same smoking habits", RuleKind::Soft, 1.0, eq(fld(Side::A, "smoking"), fld(Side::B, "smoking")));
    smoking.group = Some("lifestyle".into());
    smoking.priority = 5;

    let mut education = Rule::new("education_match", "Same education level", RuleKind::Soft, 1.0, eq(fld(Side::A, "education"), fld(Side::B, "education")));
    education.group = Some("financial_practical".into());

    RuleSet::new("Default program", 1, vec![kids, age_gap, city, religion, observance, smoking, education, window])
}
