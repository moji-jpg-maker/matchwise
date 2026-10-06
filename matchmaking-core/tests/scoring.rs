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
fn same(field: &str) -> Expr { eq(f(Side::A, field), f(Side::B, field)) }
fn soft(id: &str, group: &str, weight: f64, field: &str) -> Rule {
    let mut r = Rule::new(id, &format!("same {field}"), RuleKind::Soft, weight, same(field));
    r.group = Some(group.into());
    r
}
fn dim<'a>(c: &'a ScoreCard, key: &str) -> &'a DimensionScore { c.dimensions.iter().find(|d| d.key == key).unwrap() }
fn score(set: &RuleSet, a: &Profile, b: &Profile) -> ScoreCard { score_pair(set, a, b, &[], &[]).unwrap() }

#[test]
fn dimensions_scores_and_statuses() {
    let mut hard_kids = Rule::new("kids", "partner accepts children", RuleKind::Hard, 1.0, eq(f(Side::B, "accepts_children"), lit(Value::Bool(true))));
    hard_kids.when = Some(eq(f(Side::A, "has_children"), lit(Value::Bool(true))));
    hard_kids.scope = RuleScope::Directional;
    hard_kids.group = Some("children".into());
    let set = RuleSet::new("t", 1, vec![
        soft("city", "geography", 2.0, "city"),
        soft("edu", "financial_practical", 1.0, "education"),
        soft("smk", "lifestyle", 1.0, "smoking"),
        soft("rel", "religion", 1.0, "religiosity"),
        hard_kids,
    ]);
    let a = p("a", &[("city", txt("Shiraz")), ("education", txt("master")), ("smoking", txt("never")), ("has_children", Value::Bool(false))]);
    let b = p("b", &[("city", txt("Shiraz")), ("education", txt("bachelor")), ("smoking", txt("never")), ("has_children", Value::Bool(false))]);
    let c = score(&set, &a, &b);

    assert_eq!(dim(&c, "geography").score, Some(100.0));
    assert_eq!(dim(&c, "geography").status, DimStatus::Strong);
    assert_eq!(dim(&c, "financial_practical").score, Some(0.0));
    assert_eq!(dim(&c, "financial_practical").status, DimStatus::Concern);
    assert_eq!(dim(&c, "lifestyle").status, DimStatus::Strong);
    // religiosity unknown on both sides: rule exists but cannot be judged
    assert_eq!(dim(&c, "religion").status, DimStatus::Unknown);
    // children rule configured but skipped (neither has children): not applicable, not "strong"
    assert_eq!(dim(&c, "children").status, DimStatus::NotApplicable);
    // nobody configured personality/values/communication: reported as not assessed, never silently scored
    for k in ["personality", "values", "communication", "relationship_expectations", "family", "preferences"] {
        assert_eq!(dim(&c, k).status, DimStatus::NotAssessed, "{k}");
        assert_eq!(dim(&c, k).score, None);
    }
    assert!(c.eligible && c.hard_constraints.status == HardStatus::Pass);
    // overall = coverage-weighted mean of dimension scores: geography 100 (w1,cov1), education 0, lifestyle 100 => 66.7
    assert!((c.overall.unwrap() - 200.0 / 3.0).abs() < 1e-9);
    // dimensions come back in catalog order
    let keys: Vec<&str> = c.dimensions.iter().map(|d| d.key.as_str()).collect();
    assert_eq!(keys[0], "age_life_stage");
    assert_eq!(keys.last().copied(), Some("preferences"));
}

#[test]
fn clear_status_when_only_must_haves_hold() {
    let mut r = Rule::new("kids", "accepts children", RuleKind::Hard, 1.0, eq(f(Side::B, "accepts_children"), lit(Value::Bool(true))));
    r.group = Some("children".into());
    let set = RuleSet::new("t", 1, vec![r]);
    let c = score(&set, &p("a", &[]), &p("b", &[("accepts_children", Value::Bool(true))]));
    assert_eq!(dim(&c, "children").status, DimStatus::Clear);
    assert_eq!(c.hard_constraints.passed, 1);
    assert_eq!(c.overall, None);
}

#[test]
fn aliases_resolve_and_unknown_groups_become_other() {
    assert_eq!(resolve_dimension(Some("Location")), "geography");
    assert_eq!(resolve_dimension(Some(" AGE ")), "age_life_stage");
    assert_eq!(resolve_dimension(Some("education")), "financial_practical");
    assert_eq!(resolve_dimension(Some("zzz")), "other");
    assert_eq!(resolve_dimension(None), "other");
    let set = RuleSet::new("t", 1, vec![soft("x", "zzz", 1.0, "city")]);
    let c = score(&set, &p("a", &[("city", txt("X"))]), &p("b", &[("city", txt("X"))]));
    assert_eq!(dim(&c, "other").score, Some(100.0));
    // "other" is only listed when something uses it
    let c = score(&RuleSet::new("t", 1, vec![]), &p("a", &[]), &p("b", &[]));
    assert!(c.dimensions.iter().all(|d| d.key != "other"));
}

#[test]
fn thin_evidence_is_pulled_towards_the_prior_so_documented_candidates_rank_higher() {
    let set = RuleSet::new("t", 1, vec![
        soft("city", "geography", 1.0, "city"),
        soft("edu", "financial_practical", 1.0, "education"),
        soft("smk", "lifestyle", 1.0, "smoking"),
        soft("rel", "religion", 1.0, "religiosity"),
    ]);
    let me = p("me", &[("city", txt("X")), ("education", txt("m")), ("smoking", txt("never")), ("religiosity", txt("low"))]);
    // thin: only the city is known and matches (100 on 1 of 4 checks)
    let thin = p("thin", &[("city", txt("X"))]);
    // rich: everything known, one mismatch (75 on 4 of 4 checks)
    let rich = p("rich", &[("city", txt("X")), ("education", txt("m")), ("smoking", txt("never")), ("religiosity", txt("high"))]);
    let t = score(&set, &me, &thin);
    let r = score(&set, &me, &rich);
    assert_eq!(t.overall, Some(100.0));
    assert_eq!(r.overall, Some(75.0));
    assert_eq!(t.confidence, Some(0.25));
    assert_eq!(r.confidence, Some(1.0));
    assert_eq!(t.ranking_score, Some(0.25 * 100.0 + 0.75 * 50.0)); // 62.5
    assert_eq!(r.ranking_score, Some(75.0));
    assert!(r.ranking_score > t.ranking_score, "the better-documented candidate ranks first");

    // the neutral baseline is configurable
    let mut set2 = set.clone();
    set2.prior_score = Some(0.0);
    assert_eq!(score(&set2, &me, &thin).ranking_score, Some(25.0));
}

#[test]
fn dimension_weights_change_the_overall_score() {
    let mut set = RuleSet::new("t", 1, vec![soft("city", "geography", 1.0, "city"), soft("edu", "financial_practical", 1.0, "education")]);
    let a = p("a", &[("city", txt("X")), ("education", txt("m"))]);
    let b = p("b", &[("city", txt("X")), ("education", txt("b"))]);
    assert_eq!(score(&set, &a, &b).overall, Some(50.0));
    set.group_weights.insert("location".into(), 3.0); // alias of geography
    assert_eq!(score(&set, &a, &b).overall, Some(75.0));
    set.group_weights.insert("education".into(), 0.0); // alias of financial_practical; zero = ignored
    assert_eq!(score(&set, &a, &b).overall, Some(100.0));
}

#[test]
fn deal_breakers_and_missing_fields_are_actionable() {
    // a's deal-breaker: partner smokes regularly; b has not said anything about smoking
    let no_smokers = Preference {
        id: "smk".into(),
        condition: Condition { field: "smoking".into(), op: ConditionOp::Eq, value: Some(txt("regularly")), value2: None, value_rel: None, value2_rel: None },
        strength: Strength::DealBreaker, importance: 5, note: Some("no smokers".into()),
    };
    let set = RuleSet::new("t", 1, vec![]);
    let a = p("a", &[("age", num(30.0))]);
    let unknown = score_pair(&set, &a, &p("b", &[]), &[no_smokers.clone()], &[]).unwrap();
    assert!(unknown.eligible);
    assert_eq!(unknown.hard_constraints.status, HardStatus::NeedsInfo);
    assert_eq!(unknown.hard_constraints.undecided.len(), 1);
    assert_eq!(unknown.unknowns[0].missing, vec![MissingField { who: "b".into(), field: "smoking".into() }]);
    assert_eq!(unknown.unknowns[0].source, FindingSource::PreferenceA);
    assert_eq!(dim(&unknown, "preferences").status, DimStatus::Unknown);

    let smoker = score_pair(&set, &a, &p("b", &[("smoking", txt("regularly"))]), &[no_smokers.clone()], &[]).unwrap();
    assert!(!smoker.eligible);
    assert_eq!(smoker.hard_constraints.status, HardStatus::Fail);
    assert_eq!(smoker.hard_constraints.violations[0].description, "no smokers");
    assert_eq!(dim(&smoker, "preferences").status, DimStatus::Concern);

    // b's preference about a: the missing value is reported against the right person (a), not b
    let wants_age = Preference {
        id: "age".into(),
        condition: Condition { field: "age".into(), op: ConditionOp::Ge, value: Some(num(25.0)), value2: None, value_rel: None, value2_rel: None },
        strength: Strength::Required, importance: 3, note: None,
    };
    let c = score_pair(&set, &p("a", &[]), &p("b", &[]), &[], &[wants_age]).unwrap();
    assert_eq!(c.hard_constraints.undecided[0].source, FindingSource::PreferenceB);
    assert_eq!(c.unknowns[0].missing, vec![MissingField { who: "a".into(), field: "age".into() }]);
}

#[test]
fn directional_rules_blame_the_right_person_for_missing_data() {
    let mut r = Rule::new("kids", "partner accepts children", RuleKind::Hard, 1.0, eq(f(Side::B, "accepts_children"), lit(Value::Bool(true))));
    r.when = Some(eq(f(Side::A, "has_children"), lit(Value::Bool(true))));
    r.scope = RuleScope::Directional;
    let set = RuleSet::new("t", 1, vec![r]);
    // a has children; b never said whether they accept them: a->b is undecided and b lacks accepts_children.
    // b->a cannot tell whether it applies (b's has_children is missing): reported against b.
    let c = score(&set, &p("a", &[("has_children", Value::Bool(true))]), &p("b", &[]));
    let miss: Vec<(String, String, Direction)> = c.unknowns.iter().flat_map(|u| u.missing.iter().map(move |m| (m.who.clone(), m.field.clone(), u.direction))).collect();
    assert!(miss.contains(&("b".into(), "accepts_children".into(), Direction::AToB)), "{miss:?}");
    assert!(miss.contains(&("b".into(), "has_children".into(), Direction::BToA)), "{miss:?}");
    // b->a also needs a's accepts_children? No: a->b data a already provided; a lacks accepts_children only matters if b->a applies
    assert!(miss.contains(&("a".into(), "accepts_children".into(), Direction::BToA)), "{miss:?}");
}

#[test]
fn strengths_and_concerns_are_ordered_and_capped() {
    let mut rules = vec![];
    let fields = ["city", "education", "smoking", "religiosity", "occupation", "province", "birthplace"];
    for (i, fld) in fields.iter().enumerate() {
        rules.push(soft(&format!("r{i}"), "lifestyle", (i + 1) as f64, fld));
    }
    let set = RuleSet::new("t", 1, rules);
    let all_same: Vec<(&str, Value)> = fields.iter().map(|k| (*k, txt("x"))).collect();
    let none_same_a: Vec<(&str, Value)> = fields.iter().map(|k| (*k, txt("x"))).collect();
    let none_same_b: Vec<(&str, Value)> = fields.iter().map(|k| (*k, txt("y"))).collect();
    let good = score(&set, &p("a", &all_same), &p("b", &all_same));
    assert_eq!(good.strengths.len(), 5, "capped at five");
    assert_eq!(good.strengths[0].id, "r6", "highest weight first");
    assert!(good.concerns.is_empty());
    let bad = score(&set, &p("a", &none_same_a), &p("b", &none_same_b));
    assert_eq!(bad.concerns.len(), 5);
    assert_eq!(bad.concerns[0].id, "r6");
    assert_eq!(bad.overall, Some(0.0));
}

#[test]
fn threshold_uses_the_confidence_adjusted_score() {
    let mut set = RuleSet::new("t", 1, vec![soft("city", "geography", 1.0, "city"), soft("edu", "financial_practical", 1.0, "education")]);
    set.min_score = Some(70.0);
    let a = p("a", &[("city", txt("X")), ("education", txt("m"))]);
    // thin candidate: perfect on city, nothing known about education => ranking score 75 -> passes at 70; at 80 it would not
    let thin = p("b", &[("city", txt("X"))]);
    assert_eq!(score(&set, &a, &thin).meets_threshold, Some(true));
    set.min_score = Some(80.0);
    let c = score(&set, &a, &thin);
    assert_eq!(c.overall, Some(100.0));
    assert_eq!(c.meets_threshold, Some(false));
    assert!(!c.eligible);
}

#[test]
fn scorecard_json_shape_and_starter_rule_set() {
    let set = default_ruleset();
    let a = p("a", &[("age", num(30.0)), ("city", txt("Shiraz")), ("smoking", txt("never")), ("education", txt("master")), ("has_children", Value::Bool(false))]);
    let b = p("b", &[("age", num(34.0)), ("city", txt("Shiraz")), ("smoking", txt("never")), ("education", txt("master")), ("has_children", Value::Bool(false))]);
    let c = score(&set, &a, &b);
    assert!(validate_ruleset(&set, &default_registry()).is_empty());
    assert!(c.eligible);
    assert_eq!(dim(&c, "age_life_stage").score, Some(100.0));
    assert_eq!(dim(&c, "geography").score, Some(100.0));
    assert_eq!(dim(&c, "lifestyle").score, Some(100.0));
    assert_eq!(dim(&c, "religion").status, DimStatus::Unknown, "religiosity missing on both");
    let json = serde_json::to_value(&c).unwrap();
    assert_eq!(json["hard_constraints"]["status"], "pass");
    assert_eq!(json["dimensions"][0]["status"], "strong");
    assert!(json["dimensions"].as_array().unwrap().iter().any(|d| d["status"] == "not_assessed"));
    assert_eq!(json["strengths"][0]["source"], "rule");
    let _ = Tri::True;
}
