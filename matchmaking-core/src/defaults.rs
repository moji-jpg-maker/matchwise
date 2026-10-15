//! Starter field set from the product plan. Matchmakers can add, edit, or hide fields at runtime;
//! this is only the seed used for a new organization.

use crate::expr::{CmpOp, Expr, Operand, Side};
use crate::field::{FieldDef, FieldKind, FieldRegistry};
use crate::profile::Value;
use crate::rules::{Rule, RuleKind, RuleScope, RuleSet};

fn f(key: &str, label: &str, label_fa: &str, kind: FieldKind, sensitive: bool, required: bool) -> FieldDef {
    FieldDef { key: key.into(), label: label.into(), label_fa: Some(label_fa.into()), kind, sensitive, required }
}

fn choice(opts: &[&str]) -> FieldKind {
    FieldKind::Choice(opts.iter().map(|s| s.to_string()).collect())
}

pub fn default_registry() -> FieldRegistry {
    use FieldKind::*;
    let mut r = FieldRegistry::new();
    for d in [
        f("full_name", "Full name", "نام و نام خانوادگی", Text, true, true),
        f("gender", "Gender", "جنسیت", choice(&["female", "male"]), false, true),
        f("age", "Age", "سن", Number, false, true),
        f("date_of_birth", "Date of birth (YYYY-MM-DD)", "تاریخ تولد (سال-ماه-روز)", Text, true, false),
        f("height_cm", "Height (cm)", "قد (سانتی‌متر)", Number, false, false),
        f("weight_kg", "Weight (kg)", "وزن (کیلوگرم)", Number, true, false),
        f("birthplace", "Birthplace", "محل تولد", Text, false, false),
        f("city", "Current city", "شهر محل سکونت", Text, false, true),
        f("province", "Province / region", "استان / منطقه", Text, false, false),
        f("family_origin", "Family origin", "اصالت خانوادگی", Text, false, false),
        f("education", "Education level", "سطح تحصیلات", choice(&["secondary", "diploma", "bachelor", "master", "doctorate", "other"]), false, true),
        f("occupation", "Occupation", "شغل", Text, false, true),
        f("employment_status", "Employment status", "وضعیت اشتغال", choice(&["employed", "self_employed", "student", "unemployed", "retired"]), false, false),
        f("financial_situation", "Financial situation", "وضعیت مالی", Text, true, false),
        f("religion", "Religion", "دین", Text, true, false),
        f("religiosity", "Religious observance", "میزان پایبندی دینی", choice(&["low", "moderate", "high"]), true, false),
        f("marital_status", "Marriage history", "سابقه ازدواج", choice(&["never_married", "divorced", "widowed", "separated"]), false, true),
        f("has_children", "Has children", "فرزند دارد", Bool, false, true),
        f("children_count", "Number of children", "تعداد فرزندان", Number, false, false),
        f("children", "Children (details)", "فرزندان (جزئیات)", Records(vec![
            f("gender", "Gender", "جنسیت", choice(&["female", "male"]), false, false),
            f("age", "Age", "سن", Number, false, false),
            f("custody", "Custody", "حضانت", choice(&["with_me", "shared", "with_other_parent", "other"]), false, false),
            f("living_arrangement", "Currently living", "محل زندگی فعلی", choice(&["with_me", "with_other_parent", "boarding_or_school", "independent", "other"]), false, false),
            f("notes", "Other circumstances", "سایر شرایط", Text, false, false),
        ]), true, false),
        f("accepts_children", "Accepts a partner's children", "فرزندان همسر را می‌پذیرد", Bool, false, false),
        f("siblings_count", "Number of siblings", "تعداد خواهر و برادر", Number, false, false),
        f("father_occupation", "Father's occupation", "شغل پدر", Text, false, false),
        f("mother_occupation", "Mother's occupation", "شغل مادر", Text, false, false),
        f("smoking", "Smoking", "مصرف دخانیات", choice(&["never", "occasionally", "regularly"]), false, false),
        f("health_notes", "Health information", "اطلاعات سلامت", Text, true, false),
        f("lifestyle_notes", "Lifestyle", "سبک زندگی", Text, false, false),
        f("about", "About / free-text profile", "درباره / توضیحات آزاد", Text, false, false),
        f("telegram_username", "Telegram username", "نام کاربری تلگرام", Text, true, false),
        f("phone", "Primary phone", "شماره تلفن اصلی", Text, true, false),
        f("big5_extraversion", "Extraversion (Mini-IPIP, 4-20)", "برون‌گرایی (Mini-IPIP، ۴ تا ۲۰)", Number, true, false),
        f("big5_agreeableness", "Agreeableness (Mini-IPIP, 4-20)", "توافق‌پذیری (Mini-IPIP، ۴ تا ۲۰)", Number, true, false),
        f("big5_conscientiousness", "Conscientiousness (Mini-IPIP, 4-20)", "وظیفه‌شناسی (Mini-IPIP، ۴ تا ۲۰)", Number, true, false),
        f("big5_neuroticism", "Neuroticism (Mini-IPIP, 4-20)", "روان‌رنجوری (Mini-IPIP، ۴ تا ۲۰)", Number, true, false),
        f("big5_openness", "Intellect / imagination (Mini-IPIP, 4-20)", "هوش و خیال‌پردازی (Mini-IPIP، ۴ تا ۲۰)", Number, true, false),
    ] {
        r.register(d);
    }
    r
}

fn fld(of: Side, key: &str) -> Operand {
    Operand::Field { of, key: key.into() }
}

#[cfg(test)]
mod locale_tests {
    use super::*;
    use crate::locale::Locale;

    #[test]
    fn every_default_field_has_a_persian_label() {
        for def in default_registry().defs() {
            let persian = def.label_for(Locale::Fa);
            assert_ne!(persian, def.label, "{} has no Persian label", def.key);
            if let FieldKind::Records(subs) = &def.kind {
                for sub in subs {
                    assert_ne!(sub.label_for(Locale::Fa), sub.label, "{}.{} has no Persian label", def.key, sub.key);
                }
            }
        }
    }

    #[test]
    fn custom_field_falls_back_to_its_only_label() {
        let def = FieldDef { key: "x".into(), label: "Hobby".into(), label_fa: None, kind: FieldKind::Text, sensitive: false, required: false };
        assert_eq!(def.label_for(Locale::Fa), "Hobby");
        let json = r#"{"key":"x","label":"Hobby","kind":"text"}"#;
        assert!(serde_json::from_str::<FieldDef>(json).is_ok());
    }
}

fn lit(v: Value) -> Operand {
    Operand::Lit { value: v }
}

fn eq(l: Operand, r: Operand) -> Expr {
    Expr::Cmp { left: l, cmp: CmpOp::Eq, right: r }
}

/// Persian wording of the starter rules' descriptions, by rule id. `None` for rules the matchmaker wrote.
pub fn default_rule_description_fa(rule_id: &str) -> Option<&'static str> {
    Some(match rule_id {
        "kids" => "اگر یکی از دو نفر فرزند دارد، شریک او باید فرزند را بپذیرد",
        "age_gap" => "اختلاف سنی حداکثر ۸ سال",
        "same_city" => "ساکن یک شهر باشند",
        "religion_aligned" => "هم‌دین بودن، وقتی هر دو بسیار مقید هستند",
        "age_window_example" => "نمونه: وقتی متقاضی ۲۵ تا ۳۰ ساله است، شریک باید ۰ تا ۸ سال بزرگ‌تر باشد",
        "religiosity_close" => "سطح مشابه پایبندی دینی",
        "smoking_match" => "عادت‌های مشابه درباره سیگار",
        "education_match" => "سطح تحصیلات یکسان",
        "personality_similar_conscientiousness_example" => "نمونه (شواهد ضعیف): وظیفه‌شناسی مشابه (حداکثر ۴ امتیاز اختلاف)",
        "personality_partner_calm_example" => "نمونه (اثر کوچک): شریک روان‌رنجوری بالا ندارد (۱۲ یا کمتر)",
        _ => return None,
    })
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

    // Personality rules ship switched off. Research finds that how similar two people's trait scores are predicts
    // satisfaction very little (see the Mini-IPIP evidence record), so the matchmaker decides whether to use them.
    let mut similar_c = Rule::new(
        "personality_similar_conscientiousness_example",
        "Example (weak evidence): similar conscientiousness (within 4 points)",
        RuleKind::Soft,
        1.0,
        Expr::Between {
            value: fld(Side::B, "big5_conscientiousness"),
            lo: Operand::Offset { base: Box::new(fld(Side::A, "big5_conscientiousness")), by: -4.0 },
            hi: Operand::Offset { base: Box::new(fld(Side::A, "big5_conscientiousness")), by: 4.0 },
        },
    );
    similar_c.group = Some("personality".into());
    similar_c.enabled = false;

    let mut calm = Rule::new(
        "personality_partner_calm_example",
        "Example (small effect): partner is not high in neuroticism (12 or below)",
        RuleKind::Soft,
        1.0,
        Expr::Cmp { left: fld(Side::B, "big5_neuroticism"), cmp: CmpOp::Le, right: lit(Value::Num(12.0)) },
    );
    calm.group = Some("personality".into());
    calm.scope = RuleScope::Directional;
    calm.enabled = false;

    RuleSet::new("Default program", 1, vec![kids, age_gap, city, religion, observance, smoking, education, window, similar_c, calm])
}
