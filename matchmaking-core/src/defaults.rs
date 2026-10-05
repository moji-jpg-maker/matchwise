//! Starter field set from the product plan. Matchmakers can add, edit, or hide fields at runtime;
//! this is only the seed used for a new organization.

use crate::field::{FieldDef, FieldKind, FieldRegistry};

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
