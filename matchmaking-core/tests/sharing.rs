use matchmaking_core::profile::{Provenance, Value};
use matchmaking_core::*;
use std::collections::BTreeMap;

fn p(kv: &[(&str, Value)]) -> Profile {
    let mut x = Profile::new("x");
    for (k, v) in kv { x.set(k, v.clone(), Provenance::User); }
    x
}
fn f(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

#[test]
fn card_contains_only_listed_fields_and_first_name() {
    let reg = default_registry();
    let prof = p(&[
        ("full_name", Value::Text("Sara Karimi".into())), ("age", Value::Num(29.0)), ("city", Value::Text("Shiraz".into())),
        ("marital_status", Value::Text("never_married".into())), ("has_children", Value::Bool(false)),
        ("religion", Value::Text("X".into())), ("health_notes", Value::Text("private".into())), ("phone", Value::Text("+98 900".into())),
        ("height_cm", Value::Num(165.5)),
    ]);
    let card = build_introduction_card(&prof, &reg, &f(&["age", "city", "marital_status", "has_children", "height_cm", "education"]), true);
    assert_eq!(card.title, "Sara");
    let lines: BTreeMap<_, _> = card.lines.iter().cloned().collect();
    assert_eq!(lines.get("Age").map(String::as_str), Some("29"), "whole numbers print without .0");
    assert_eq!(lines.get("Marriage history").map(String::as_str), Some("never married"));
    assert_eq!(lines.get("Has children").map(String::as_str), Some("no"));
    assert_eq!(lines.get("Height (cm)").map(String::as_str), Some("165.5"));
    assert!(!lines.contains_key("Education level"), "missing values are skipped, not shown as blanks");
    assert!(!card.lines.iter().any(|(_, v)| v.contains("private") || v.contains("+98") || v == "X"));
    assert_eq!(card.lines.len(), 5);
}

#[test]
fn contact_details_and_names_can_never_be_shared() {
    let reg = default_registry();
    let prof = p(&[("full_name", Value::Text("Sara Karimi".into())), ("phone", Value::Text("+98 900".into())), ("telegram_username", Value::Text("sara".into())), ("date_of_birth", Value::Text("1995-01-01".into()))]);
    // even if a matchmaker lists them, they stay off the card
    let card = build_introduction_card(&prof, &reg, &f(&["phone", "telegram_username", "full_name", "date_of_birth"]), true);
    assert!(card.lines.is_empty());
    assert_eq!(card.title, "Sara");
    // first names can be switched off
    assert_eq!(build_introduction_card(&prof, &reg, &[], false).title, "Someone");
    // no name recorded
    assert_eq!(first_name(&p(&[]), true), "Someone");
}

#[test]
fn defaults_exclude_every_sensitive_field_and_records() {
    let reg = default_registry();
    for k in DEFAULT_INTRODUCTION_FIELDS {
        let def = reg.get(k).unwrap_or_else(|| panic!("{k} is not a default field"));
        assert!(!def.sensitive, "{k} is sensitive and must not be shared by default");
        assert!(!matches!(def.kind, FieldKind::Records(_)));
        assert!(!NEVER_SHARED.contains(k));
    }
    // children details are a records field: skipped even when listed
    let prof = p(&[("children", Value::Records(vec![BTreeMap::from([("age".to_string(), Value::Num(4.0))])]))]);
    assert!(build_introduction_card(&prof, &reg, &f(&["children"]), true).lines.is_empty());
}

#[test]
fn preferences_read_naturally() {
    let reg = default_registry();
    let c = |field: &str, op: ConditionOp, v: Option<Value>, v2: Option<Value>| Condition { field: field.into(), op, value: v, value2: v2, value_rel: None, value2_rel: None };
    let pr = |cond: Condition, s: Strength| Preference { id: "p".into(), condition: cond, strength: s, importance: 3, note: None };
    assert_eq!(describe_preference(&pr(c("smoking", ConditionOp::In, Some(Value::List(vec!["never".into(), "occasionally".into()])), None), Strength::DealBreaker), &reg), "Smoking is never or occasionally (deal-breaker)");
    assert_eq!(describe_preference(&pr(c("age", ConditionOp::Between, Some(Value::Num(27.0)), Some(Value::Num(34.0))), Strength::Required), &reg), "Age between 27 and 34 (must have)");
    let mut rel = c("age", ConditionOp::Between, None, None);
    rel.value_rel = Some(RelativeValue { field: "age".into(), offset: 0.0 });
    rel.value2_rel = Some(RelativeValue { field: "age".into(), offset: 8.0 });
    assert_eq!(describe_preference(&pr(rel, Strength::Preferred), &reg), "Age between own age and own age + 8 (preferred)");
    assert_eq!(describe_preference(&pr(c("has_children", ConditionOp::Eq, Some(Value::Bool(false)), None), Strength::Flexible), &reg), "Has children is no (nice to have)");
}
