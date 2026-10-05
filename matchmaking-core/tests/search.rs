use matchmaking_core::profile::Provenance;
use matchmaking_core::*;

fn person(id: &str, gender: &str, age: f64, city: &str, kids: Option<bool>, langs: &[&str]) -> Profile {
    let mut p = Profile::new(id);
    p.set("gender", Value::Text(gender.into()), Provenance::User);
    p.set("age", Value::Num(age), Provenance::User);
    p.set("city", Value::Text(city.into()), Provenance::User);
    if let Some(k) = kids { p.set("has_children", Value::Bool(k), Provenance::User); }
    if !langs.is_empty() { p.set("hobbies", Value::List(langs.iter().map(|s| s.to_string()).collect()), Provenance::User); }
    p
}

fn pool() -> Vec<Profile> {
    vec![
        person("a", "female", 28.0, "Shiraz", Some(false), &["hiking", "books"]),
        person("b", "female", 36.0, "Shiraz", Some(false), &[]),
        person("c", "female", 30.0, "Tehran", Some(false), &[]),
        person("d", "female", 31.0, "Shiraz", None, &[]), // has_children unknown
        person("e", "male", 30.0, "Shiraz", Some(false), &[]),
    ]
}

fn cond(field: &str, op: ConditionOp, v: Option<Value>, v2: Option<Value>) -> Condition {
    Condition { field: field.into(), op, value: v, value2: v2, value_rel: None, value2_rel: None }
}

fn ids(h: &[SearchHit]) -> Vec<&str> { h.iter().map(|x| x.profile_id.as_str()).collect() }

#[test]
fn plan_example_search() {
    // Female, age 27-34, city Shiraz, no children (example from the plan)
    let f = conditions_to_expr(&[
        cond("gender", ConditionOp::Eq, Some(Value::Text("female".into())), None),
        cond("age", ConditionOp::Between, Some(Value::Num(27.0)), Some(Value::Num(34.0))),
        cond("city", ConditionOp::Eq, Some(Value::Text("shiraz".into())), None), // case-insensitive
        cond("has_children", ConditionOp::Eq, Some(Value::Bool(false)), None),
    ]).unwrap();
    assert_eq!(ids(&search(&pool(), &f, false)), vec!["a"]);
    // lenient mode also surfaces d (children status unknown) but flags it Unknown
    let lenient = search(&pool(), &f, true);
    assert_eq!(ids(&lenient), vec!["a", "d"]);
    assert_eq!(lenient[1].result, expr::Tri::Unknown);
}

#[test]
fn in_list_and_multichoice_contains() {
    let f = conditions_to_expr(&[cond("city", ConditionOp::In, Some(Value::List(vec!["Tehran".into(), "Isfahan".into()])), None)]).unwrap();
    assert_eq!(ids(&search(&pool(), &f, false)), vec!["c"]);
    let f = conditions_to_expr(&[cond("hobbies", ConditionOp::In, Some(Value::Text("hiking".into())), None)]).unwrap();
    assert_eq!(ids(&search(&pool(), &f, false)), vec!["a"]);
}

#[test]
fn exists_and_missing_value_errors() {
    let f = conditions_to_expr(&[cond("has_children", ConditionOp::Exists, None, None)]).unwrap();
    assert_eq!(search(&pool(), &f, false).len(), 4);
    assert!(conditions_to_expr(&[cond("age", ConditionOp::Gt, None, None)]).is_err());
}

#[test]
fn conditions_roundtrip_json() {
    let c = vec![cond("age", ConditionOp::Between, Some(Value::Num(27.0)), Some(Value::Num(34.0)))];
    let s = serde_json::to_string(&c).unwrap();
    assert!(s.contains("\"between\""));
    let back: Vec<Condition> = serde_json::from_str(&s).unwrap();
    assert_eq!(back, c);
}

#[test]
fn validation_and_completeness() {
    let reg = default_registry();
    let mut p = Profile::new("x");
    p.set("age", Value::Text("thirty".into()), Provenance::User); // wrong type
    p.set("gender", Value::Text("other".into()), Provenance::User); // not an allowed choice
    p.set("favourite_colour", Value::Text("teal".into()), Provenance::User); // unregistered
    let issues = p.validate(&reg);
    assert_eq!(issues.len(), 3);
    assert!(p.completeness(&reg).unwrap() < 0.5);
    assert!(p.missing_required(&reg).contains(&"city".to_string()));
    // sensitive flags drive redaction/visibility
    assert!(reg.is_sensitive("religion") && !reg.is_sensitive("city"));
    assert!(reg.is_sensitive("not_registered")); // unknown => sensitive by default
}

#[test]
fn registry_keeps_curated_order_and_replaces_in_place() {
    let reg = default_registry();
    let keys: Vec<&str> = reg.keys().map(|s| s.as_str()).collect();
    assert_eq!(keys[0], "full_name");
    assert_eq!(keys[1], "gender");
    assert!(keys.contains(&"children"));
    let mut reg = reg;
    let mut def = reg.get("city").unwrap().clone();
    def.label = "Town".into();
    reg.register(def);
    assert_eq!(reg.get("city").unwrap().label, "Town");
    assert_eq!(reg.keys().filter(|k| k.as_str() == "city").count(), 1);
}
