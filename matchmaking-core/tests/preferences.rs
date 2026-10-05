use matchmaking_core::expr::Tri;
use matchmaking_core::profile::{Provenance, Value};
use matchmaking_core::*;
use std::collections::BTreeMap;

fn p(id: &str, kv: &[(&str, Value)]) -> Profile {
    let mut x = Profile::new(id);
    for (k, v) in kv {
        x.set(k, v.clone(), Provenance::User);
    }
    x
}
fn num(n: f64) -> Value { Value::Num(n) }
fn txt(s: &str) -> Value { Value::Text(s.into()) }

fn cond(field: &str, op: ConditionOp, v: Option<Value>, v2: Option<Value>) -> Condition {
    Condition { field: field.into(), op, value: v, value2: v2, value_rel: None, value2_rel: None }
}
fn rel(field: &str, offset: f64) -> Option<RelativeValue> { Some(RelativeValue { field: field.into(), offset }) }
fn pref(id: &str, c: Condition, s: Strength, imp: u8) -> Preference {
    Preference { id: id.into(), condition: c, strength: s, importance: imp, note: None }
}

/// "IF candidate.age BETWEEN 25 AND 30 THEN partner age between own age + 0 and own age + 8" from the plan.
fn age_window() -> Preference {
    let mut c = cond("age", ConditionOp::Between, None, None);
    c.value_rel = rel("age", 0.0);
    c.value2_rel = rel("age", 8.0);
    pref("age", c, Strength::Required, 3)
}

#[test]
fn relative_age_window_from_the_plan() {
    let owner = p("o", &[("age", num(28.0))]);
    let prefs = [age_window()];
    let eval = |age: Option<f64>| {
        let cand = match age { Some(a) => p("c", &[("age", num(a))]), None => p("c", &[]) };
        evaluate_preferences(&prefs, &owner, &cand).unwrap()
    };
    assert!(eval(Some(30.0)).eligible);
    assert!(eval(Some(36.0)).eligible); // 28 + 8, inclusive
    assert!(!eval(Some(38.0)).eligible);
    assert!(!eval(Some(25.0)).eligible); // younger than 28 + 0
    let unknown = eval(None);
    assert!(unknown.eligible, "missing data does not exclude");
    assert_eq!(unknown.needs_info, vec!["age".to_string()]);
}

#[test]
fn deal_breaker_excludes_only_when_it_matches() {
    let owner = p("o", &[]);
    let prefs = [pref("smk", cond("smoking", ConditionOp::Eq, Some(txt("regularly")), None), Strength::DealBreaker, 5)];
    let run = |s: Option<&str>| {
        let cand = match s { Some(v) => p("c", &[("smoking", txt(v))]), None => p("c", &[]) };
        evaluate_preferences(&prefs, &owner, &cand).unwrap()
    };
    assert!(!run(Some("regularly")).eligible);
    assert!(run(Some("never")).eligible);
    let u = run(None);
    assert!(u.eligible && u.needs_info == vec!["smk".to_string()]);
}

#[test]
fn importance_and_flexibility_weight_the_score() {
    let owner = p("o", &[]);
    let prefs = [
        pref("city", cond("city", ConditionOp::Eq, Some(txt("Shiraz")), None), Strength::Preferred, 5), // weight 5
        pref("edu", cond("education", ConditionOp::Eq, Some(txt("master")), None), Strength::Preferred, 1), // weight 1
        pref("rel", cond("religiosity", ConditionOp::Eq, Some(txt("high")), None), Strength::Flexible, 4), // weight 2
    ];
    // city matches (5), education fails (1), religiosity fails (2): 5 / 8 = 62.5
    let cand = p("c", &[("city", txt("shiraz")), ("education", txt("bachelor")), ("religiosity", txt("low"))]);
    let o = evaluate_preferences(&prefs, &owner, &cand).unwrap();
    assert!(o.eligible);
    assert_eq!(o.soft_score, Some(62.5));
    // unknown soft preferences are excluded from the score and reported
    let cand = p("c", &[("city", txt("Shiraz"))]);
    let o = evaluate_preferences(&prefs, &owner, &cand).unwrap();
    assert_eq!(o.soft_score, Some(100.0));
    assert_eq!(o.unknown_soft.len(), 2);
}

#[test]
fn mutual_evaluation_checks_both_directions() {
    let a = p("a", &[("age", num(30.0)), ("has_children", Value::Bool(true))]);
    let b = p("b", &[("age", num(34.0)), ("accepts_children", Value::Bool(false))]);
    // a requires the partner to be 0..8 years older; b refuses a partner with children
    let a_prefs = [age_window()];
    let b_prefs = [pref("kids", cond("has_children", ConditionOp::Eq, Some(Value::Bool(true)), None), Strength::DealBreaker, 5)];
    let m = evaluate_mutual(&a_prefs, &a, &b_prefs, &b).unwrap();
    assert!(m.a_wants_b.eligible);
    assert!(!m.b_wants_a.eligible);
    assert!(!m.eligible);
}

#[test]
fn children_records_validate() {
    let reg = default_registry();
    let child = |kv: &[(&str, Value)]| Value::Records(vec![kv.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>()]);
    assert!(reg.validate_value("children", &child(&[("gender", txt("male")), ("age", num(6.0)), ("custody", txt("with_me"))])).is_ok());
    assert!(reg.validate_value("children", &Value::List(vec![])).is_ok(), "empty array means no children");
    assert!(reg.validate_value("children", &child(&[("custody", txt("nonsense"))])).is_err());
    assert!(reg.validate_value("children", &child(&[("shoe_size", num(30.0))])).is_err());
    assert!(reg.validate_value("children", &child(&[("age", txt("six"))])).is_err());
    assert!(reg.validate_value("children", &txt("x")).is_err());
}

#[test]
fn records_json_roundtrip_is_unambiguous() {
    let v: Value = serde_json::from_str(r#"[{"gender":"male","age":6}]"#).unwrap();
    assert!(matches!(v, Value::Records(ref r) if r.len() == 1));
    let v: Value = serde_json::from_str(r#"["a","b"]"#).unwrap();
    assert!(matches!(v, Value::List(_)));
    let mut prof = Profile::new("x");
    prof.set("children", Value::Records(vec![BTreeMap::from([("age".to_string(), num(4.0))])]), Provenance::User);
    let s = serde_json::to_string(&prof).unwrap();
    let back: Profile = serde_json::from_str(&s).unwrap();
    assert_eq!(back.get("children"), prof.get("children"));
    // the default registry (with the nested records field) round-trips too
    let reg = default_registry();
    let back: FieldRegistry = serde_json::from_str(&serde_json::to_string(&reg).unwrap()).unwrap();
    assert_eq!(back.defs().count(), reg.defs().count());
}

#[test]
fn preference_validation() {
    let reg = default_registry();
    let ok = |pf: &Preference| validate_preference(pf, &reg);
    assert!(ok(&age_window()).is_ok());
    assert!(ok(&pref("x", cond("nope", ConditionOp::Eq, Some(txt("a")), None), Strength::Required, 3)).is_err()); // unknown field
    assert!(ok(&pref("x", cond("children", ConditionOp::Exists, None, None), Strength::Required, 3)).is_err()); // records unsupported
    assert!(ok(&pref("x", cond("age", ConditionOp::Between, Some(num(20.0)), None), Strength::Required, 3)).is_err()); // missing upper bound
    assert!(ok(&pref("x", cond("age", ConditionOp::In, Some(Value::List(vec!["1".into()])), None), Strength::Required, 3)).is_err()); // op not allowed
    assert!(ok(&pref("x", cond("smoking", ConditionOp::In, Some(Value::List(vec!["never".into(), "bogus".into()])), None), Strength::Preferred, 3)).is_err());
    assert!(ok(&pref("x", cond("smoking", ConditionOp::In, Some(Value::List(vec!["never".into()])), None), Strength::Preferred, 0)).is_err()); // importance
    assert!(ok(&pref("x", cond("smoking", ConditionOp::In, Some(Value::List(vec!["never".into()])), None), Strength::Preferred, 6)).is_err());
    let mut bad_rel = cond("city", ConditionOp::Eq, None, None);
    bad_rel.value_rel = rel("age", 1.0);
    assert!(ok(&pref("x", bad_rel, Strength::Required, 3)).is_err()); // relative on a text field
    let mut rel_to_text = cond("age", ConditionOp::Ge, None, None);
    rel_to_text.value_rel = rel("city", 1.0);
    assert!(ok(&pref("x", rel_to_text, Strength::Required, 3)).is_err()); // relative to non-numeric
}

#[test]
fn relative_values_are_rejected_in_plain_search() {
    let mut c = cond("age", ConditionOp::Ge, None, None);
    c.value_rel = rel("age", 0.0);
    assert!(conditions_to_expr(&[c]).is_err());
}

#[test]
fn preference_json_roundtrip() {
    let pf = age_window();
    let s = serde_json::to_string(&pf).unwrap();
    assert!(s.contains("\"required\"") && s.contains("value_rel"));
    let back: Preference = serde_json::from_str(&s).unwrap();
    assert_eq!(back, pf);
    // importance defaults to 3 when omitted by a client
    let minimal: Preference = serde_json::from_str(r#"{"condition":{"field":"city","op":"eq","value":"Shiraz"},"strength":"preferred"}"#).unwrap();
    assert_eq!(minimal.importance, 3);
    let _ = Tri::True;
}
