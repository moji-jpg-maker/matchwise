use matchmaking_core::expr::{CmpOp, Expr, Operand, Side, Tri};
use matchmaking_core::profile::{Provenance, Value};
use matchmaking_core::*;

fn p(id: &str, kv: &[(&str, Value)]) -> Profile {
    let mut x = Profile::new(id);
    for (k, v) in kv { x.set(k, v.clone(), Provenance::User); }
    x
}
fn num(n: f64) -> Value { Value::Num(n) }
fn txt(s: &str) -> Value { Value::Text(s.into()) }
fn f(of: Side, k: &str) -> Operand { Operand::Field { of, key: k.into() } }
fn lit(v: Value) -> Operand { Operand::Lit { value: v } }
fn eq(l: Operand, r: Operand) -> Expr { Expr::Cmp { left: l, cmp: CmpOp::Eq, right: r } }

/// The plan's example: IF candidate.age BETWEEN 25 AND 30 THEN partner age between own age and own age + 8 (must).
fn age_window_rule() -> Rule {
    let mut r = Rule::new("age_window", "partner 0-8 years older when candidate is 25-30", RuleKind::Hard, 1.0,
        Expr::Between { value: f(Side::B, "age"), lo: f(Side::A, "age"),
            hi: Operand::Offset { base: Box::new(f(Side::A, "age")), by: 8.0 } });
    r.when = Some(Expr::Between { value: f(Side::A, "age"), lo: lit(num(25.0)), hi: lit(num(30.0)) });
    r.scope = RuleScope::Directional;
    r
}

#[test]
fn conditional_rule_is_skipped_not_passed() {
    // soft rule worth 3 that only applies when both are highly religious
    let mut r = Rule::new("religion", "same religion when both highly religious", RuleKind::Soft, 3.0, eq(f(Side::A, "religion"), f(Side::B, "religion")));
    r.when = Some(Expr::And { args: vec![
        eq(f(Side::A, "religiosity"), lit(txt("high"))),
        eq(f(Side::B, "religiosity"), lit(txt("high"))),
    ]});
    let city = Rule::new("city", "same city", RuleKind::Soft, 1.0, eq(f(Side::A, "city"), f(Side::B, "city")));
    let set = RuleSet::new("t", 1, vec![r, city]);

    // not both highly religious: the religion rule is skipped, so the score is decided by the city rule alone
    let a = p("a", &[("city", txt("Shiraz")), ("religiosity", txt("low")), ("religion", txt("X"))]);
    let b = p("b", &[("city", txt("Shiraz")), ("religiosity", txt("high")), ("religion", txt("Y"))]);
    let o = evaluate_ruleset_mutual(&set, &a, &b);
    assert_eq!(o.soft_score, Some(100.0));
    assert!(!o.results.iter().find(|r| r.rule_id == "religion").unwrap().applicable);

    // both high but different religions: the rule applies and fails (weight 3 of 4)
    let a2 = p("a", &[("city", txt("Shiraz")), ("religiosity", txt("high")), ("religion", txt("X"))]);
    let o = evaluate_ruleset_mutual(&set, &a2, &b);
    assert_eq!(o.soft_score, Some(25.0));

    // religiosity unknown on one side: cannot tell whether the rule applies, reported as unknown
    let c = p("c", &[("city", txt("Shiraz"))]);
    let o = evaluate_ruleset_mutual(&set, &a2, &c);
    assert_eq!(o.unknown_soft, vec!["religion".to_string()]);
    assert_eq!(o.soft_score, Some(100.0));
}

#[test]
fn directional_rule_runs_both_ways() {
    let set = RuleSet::new("t", 1, vec![age_window_rule()]);
    let ok = evaluate_ruleset_mutual(&set, &p("a", &[("age", num(28.0))]), &p("b", &[("age", num(33.0))]));
    assert!(ok.eligible); // a->b fine; b (33) is outside 25-30 so b->a is skipped
    assert_eq!(ok.results.len(), 2);

    let bad = evaluate_ruleset_mutual(&set, &p("a", &[("age", num(28.0))]), &p("b", &[("age", num(40.0))]));
    assert!(!bad.eligible);
    let failed = bad.results.iter().find(|r| r.result == Tri::False && r.applicable).unwrap();
    assert_eq!(failed.direction, Direction::AToB);

    // a is 28 and b is 26: b is younger than a, so a->b fails even though b->a would pass
    let younger = evaluate_ruleset_mutual(&set, &p("a", &[("age", num(28.0))]), &p("b", &[("age", num(26.0))]));
    assert!(!younger.eligible);

    // unknown partner age: a->b cannot be decided, and b->a cannot even tell whether its condition
    // (b is 25-30) applies; both are reported, labelled with their direction, and neither excludes the pair
    let unk = evaluate_ruleset_mutual(&set, &p("a", &[("age", num(28.0))]), &p("b", &[]));
    assert!(unk.eligible);
    assert_eq!(unk.needs_info, vec!["age_window:a_to_b".to_string(), "age_window:b_to_a".to_string()]);
}

#[test]
fn group_weights_priority_disabled_and_threshold() {
    let mut city = Rule::new("city", "same city", RuleKind::Soft, 1.0, eq(f(Side::A, "city"), f(Side::B, "city")));
    city.group = Some("location".into());
    city.priority = 5;
    let mut edu = Rule::new("edu", "same education", RuleKind::Soft, 1.0, eq(f(Side::A, "education"), f(Side::B, "education")));
    edu.priority = 10;
    let mut off = Rule::new("off", "disabled rule that would always fail", RuleKind::Hard, 1.0, eq(lit(num(1.0)), lit(num(2.0))));
    off.enabled = false;
    let mut set = RuleSet::new("t", 1, vec![city, edu, off]);

    let a = p("a", &[("city", txt("Shiraz")), ("education", txt("master"))]);
    let b = p("b", &[("city", txt("Shiraz")), ("education", txt("bachelor"))]);
    let o = evaluate_ruleset_mutual(&set, &a, &b);
    assert!(o.eligible, "disabled rules are ignored");
    assert_eq!(o.soft_score, Some(50.0));
    assert_eq!(o.results[0].rule_id, "edu", "highest priority first");

    set.group_weights.insert("location".into(), 3.0); // city now counts 3x
    assert_eq!(evaluate_ruleset_mutual(&set, &a, &b).soft_score, Some(75.0));

    set.min_score = Some(80.0);
    assert_eq!(evaluate_ruleset_mutual(&set, &a, &b).meets_threshold, Some(false));
}

#[test]
fn full_match_combines_rules_and_preferences() {
    let a = p("a", &[("age", num(30.0)), ("city", txt("Shiraz")), ("smoking", txt("never"))]);
    let b = p("b", &[("age", num(31.0)), ("city", txt("Shiraz")), ("smoking", txt("regularly"))]);
    let city = Rule::new("city", "same city", RuleKind::Soft, 1.0, eq(f(Side::A, "city"), f(Side::B, "city")));
    let set = RuleSet::new("t", 1, vec![city]);
    let no_smokers = Preference {
        id: "smk".into(),
        condition: Condition { field: "smoking".into(), op: ConditionOp::Eq, value: Some(txt("regularly")), value2: None, value_rel: None, value2_rel: None },
        strength: Strength::DealBreaker, importance: 5, note: None,
    };
    let m = evaluate_match(&set, &a, &b, &[no_smokers.clone()], &[]).unwrap();
    assert!(!m.eligible, "a's deal-breaker excludes b");
    assert!(!m.a_preferences.eligible && m.b_preferences.eligible && m.rules.eligible);
    // b has no preferences, so b's side has no score; combined score = mean of rules (100) only
    assert_eq!(m.score, Some(100.0));

    let m = evaluate_match(&set, &a, &p("c", &[("age", num(31.0)), ("city", txt("Tehran"))]), &[no_smokers], &[]).unwrap();
    assert!(m.eligible && m.needs_info, "smoking unknown: eligible but flagged");
    assert_eq!(m.score, Some(0.0));
}

#[test]
fn old_json_without_new_fields_still_loads() {
    let json = r#"{"name":"old","version":2,"rules":[{"id":"r","description":"d","kind":"soft","expr":{"op":"exists","value":{"field":{"of":"a","key":"age"}}}}]}"#;
    let set: RuleSet = serde_json::from_str(json).unwrap();
    let r = &set.rules[0];
    assert!(r.enabled && r.when.is_none() && r.priority == 0 && r.scope == RuleScope::Pair && r.weight == 1.0);
    assert!(set.group_weights.is_empty() && set.min_score.is_none());
    // and a full rule round-trips
    let mut full = age_window_rule();
    full.group = Some("age".into());
    let back: Rule = serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
    assert_eq!(back, full);
}

#[test]
fn validation_catches_bad_rules() {
    let reg = default_registry();
    let issues = |rules: Vec<Rule>| validate_ruleset(&RuleSet::new("t", 1, rules), &reg);
    let rule = |id: &str, e: Expr| Rule::new(id, "d", RuleKind::Soft, 1.0, e);

    assert!(issues(vec![age_window_rule()]).is_empty(), "the plan example is valid");

    let msgs = |rules: Vec<Rule>| issues(rules).into_iter().map(|i| i.message).collect::<Vec<_>>().join(" | ");
    assert!(msgs(vec![rule("a", eq(f(Side::A, "nope"), lit(num(1.0))))]).contains("unknown field 'nope'"));
    assert!(msgs(vec![rule("a", eq(f(Side::A, "age"), lit(txt("x"))))]).contains("cannot compare"));
    assert!(msgs(vec![rule("a", Expr::Cmp { left: f(Side::A, "city"), cmp: CmpOp::Gt, right: lit(num(1.0)) })]).contains("need numbers"));
    assert!(msgs(vec![rule("a", eq(f(Side::A, "smoking"), lit(txt("sometimes"))))]).contains("not an option"));
    assert!(msgs(vec![rule("a", eq(f(Side::A, "children"), lit(num(1.0))))]).contains("repeating records"));
    assert!(msgs(vec![rule("a", Expr::And { args: vec![] })]).contains("at least one"));
    assert!(msgs(vec![rule("a", Expr::Exists { value: lit(num(1.0)) })]).contains("needs a field"));
    assert!(msgs(vec![rule("a", Expr::Between { value: f(Side::B, "city"), lo: lit(num(1.0)), hi: lit(num(2.0)) })]).contains("between"));
    // duplicate id, empty description, bad weight, bad threshold
    let dup = issues(vec![rule("a", Expr::Exists { value: f(Side::A, "age") }), rule("a", Expr::Exists { value: f(Side::A, "age") })]);
    assert!(dup.iter().any(|i| i.message == "Duplicate rule id"));
    let mut r = rule("w", Expr::Exists { value: f(Side::A, "age") });
    r.weight = -1.0;
    r.description = " ".into();
    let m = msgs(vec![r]);
    assert!(m.contains("Weight") && m.contains("Describe"));
    let mut set = RuleSet::new("t", 1, vec![]);
    set.min_score = Some(150.0);
    assert_eq!(validate_ruleset(&set, &reg).len(), 1);
    // depth limit
    let mut deep = Expr::Exists { value: f(Side::A, "age") };
    for _ in 0..10 { deep = Expr::Not { arg: Box::new(deep) }; }
    assert!(msgs(vec![rule("d", deep)]).contains("nested too deeply"));
}

#[test]
fn starter_rule_set_is_valid_and_behaves() {
    let reg = default_registry();
    let set = default_ruleset();
    assert!(validate_ruleset(&set, &reg).is_empty());
    // a has children and b does not accept them: the directional hard rule excludes the pair
    let a = p("a", &[("has_children", Value::Bool(true)), ("age", num(30.0)), ("city", txt("Shiraz"))]);
    let b = p("b", &[("accepts_children", Value::Bool(false)), ("age", num(33.0)), ("city", txt("Shiraz"))]);
    let o = evaluate_ruleset_mutual(&set, &a, &b);
    assert!(!o.eligible);
    assert!(o.results.iter().all(|r| r.rule_id != "age_window_example"), "disabled example is not evaluated");
    // with children accepted, only the soft rules decide: age_gap (2) + same_city (2) pass; religion not applicable
    let b2 = p("b", &[("accepts_children", Value::Bool(true)), ("age", num(33.0)), ("city", txt("Shiraz"))]);
    let o = evaluate_ruleset_mutual(&set, &a, &b2);
    assert!(o.eligible);
    assert_eq!(o.soft_score, Some(100.0));
}

#[test]
fn coverage_shows_how_much_of_the_score_is_backed_by_data() {
    let city = Rule::new("city", "same city", RuleKind::Soft, 2.0, eq(f(Side::A, "city"), f(Side::B, "city")));
    let edu = Rule::new("edu", "same education", RuleKind::Soft, 2.0, eq(f(Side::A, "education"), f(Side::B, "education")));
    let set = RuleSet::new("t", 1, vec![city, edu]);
    // everything known
    let full = evaluate_ruleset_mutual(&set, &p("a", &[("city", txt("X")), ("education", txt("m"))]), &p("b", &[("city", txt("X")), ("education", txt("m"))]));
    assert_eq!(full.coverage, Some(1.0));
    // education unknown on one side: score is 100 on the city rule alone, but only half the weight was evaluable
    let part = evaluate_ruleset_mutual(&set, &p("a", &[("city", txt("X")), ("education", txt("m"))]), &p("b", &[("city", txt("X"))]));
    assert_eq!(part.soft_score, Some(100.0));
    assert_eq!(part.coverage, Some(0.5));
    // nothing evaluable
    let none = evaluate_ruleset_mutual(&set, &p("a", &[]), &p("b", &[]));
    assert_eq!((none.soft_score, none.coverage), (None, Some(0.0)));
    // an empty rule set has no coverage figure
    assert_eq!(evaluate_ruleset_mutual(&RuleSet::new("e", 1, vec![]), &p("a", &[]), &p("b", &[])).coverage, None);
}

#[test]
fn json_produced_by_the_editor_deserializes() {
    // Shapes exactly as the TypeScript editor emits them (externally tagged operands, internally tagged expressions).
    let json = r#"{
      "name": "ui", "version": 0, "min_score": null, "group_weights": {"age": 2},
      "rules": [{
        "id": "rule_1", "description": "d", "kind": "soft", "weight": 2, "priority": 5, "scope": "directional",
        "enabled": true, "group": "age",
        "when": {"op":"cmp","left":{"field":{"of":"a","key":"religiosity"}},"cmp":"in","right":{"lit":{"value":["high"]}}},
        "expr": {"op":"and","args":[
          {"op":"between","value":{"field":{"of":"b","key":"age"}},"lo":{"offset":{"base":{"field":{"of":"a","key":"age"}},"by":-8}},"hi":{"offset":{"base":{"field":{"of":"a","key":"age"}},"by":8}}},
          {"op":"not","arg":{"op":"exists","value":{"field":{"of":"b","key":"smoking"}}}},
          {"op":"if","when":{"op":"cmp","left":{"field":{"of":"a","key":"has_children"}},"cmp":"eq","right":{"lit":{"value":true}}},
                     "then":{"op":"cmp","left":{"field":{"of":"b","key":"accepts_children"}},"cmp":"eq","right":{"lit":{"value":true}}}}
        ]}
      }]
    }"#;
    let set: RuleSet = serde_json::from_str(json).unwrap();
    assert_eq!(set.rules[0].scope, RuleScope::Directional);
    assert_eq!(set.group_weights.get("age"), Some(&2.0));
    assert!(validate_ruleset(&set, &default_registry()).is_empty());
}
