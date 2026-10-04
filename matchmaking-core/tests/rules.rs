use matchmaking_core::expr::{CmpOp, Expr, Operand, Side};
use matchmaking_core::profile::{Provenance, Value};
use matchmaking_core::*;

fn p(id: &str, age: f64, kids: bool, accepts: Option<bool>) -> Profile {
    let mut x = Profile::new(id);
    x.set("age", Value::Num(age), Provenance::User);
    x.set("has_children", Value::Bool(kids), Provenance::User);
    if let Some(a) = accepts { x.set("accepts_children", Value::Bool(a), Provenance::User); }
    x
}
fn f(of: Side, k: &str) -> Operand { Operand::Field { of, key: k.into() } }
fn lit_b(v: bool) -> Operand { Operand::Lit { value: Value::Bool(v) } }

fn ruleset() -> RuleSet {
    RuleSet { name: "default".into(), version: 3, rules: vec![
        // From the plan: IF candidate.has_children THEN partner.accepts_children MUST = true
        Rule { id: "kids".into(), description: "partner accepts children".into(), kind: RuleKind::Hard, weight: 1.0,
            expr: Expr::If {
                when: Box::new(Expr::Cmp { left: f(Side::A, "has_children"), cmp: CmpOp::Eq, right: lit_b(true) }),
                then: Box::new(Expr::Cmp { left: f(Side::B, "accepts_children"), cmp: CmpOp::Eq, right: lit_b(true) }),
            } },
        // Partner age within [age, age+8]
        Rule { id: "age".into(), description: "partner age 0..8 above".into(), kind: RuleKind::Soft, weight: 2.0,
            expr: Expr::Between {
                value: f(Side::B, "age"),
                lo: f(Side::A, "age"),
                hi: Operand::Offset { base: Box::new(f(Side::A, "age")), by: 8.0 },
            } },
    ]}
}

#[test]
fn hard_rule_excludes() {
    let o = evaluate_pair(&ruleset(), &p("a", 28.0, true, None), &p("b", 32.0, false, Some(false)));
    assert!(!o.eligible);
}

#[test]
fn vacuous_when_no_children() {
    let o = evaluate_pair(&ruleset(), &p("a", 28.0, false, None), &p("b", 32.0, false, None));
    assert!(o.eligible);
    assert_eq!(o.soft_score, Some(100.0));
}

#[test]
fn missing_data_is_reported_not_guessed() {
    // b never stated accepts_children
    let o = evaluate_pair(&ruleset(), &p("a", 28.0, true, None), &p("b", 32.0, false, None));
    assert!(o.eligible);
    assert_eq!(o.needs_info, vec!["kids".to_string()]);
}

#[test]
fn soft_score_and_unknown() {
    let o = evaluate_pair(&ruleset(), &p("a", 28.0, false, None), &p("b", 40.0, false, None));
    assert_eq!(o.soft_score, Some(0.0));
    let mut b = Profile::new("b");
    b.set("has_children", Value::Bool(false), Provenance::User); // no age
    let o = evaluate_pair(&ruleset(), &p("a", 28.0, false, None), &b);
    assert_eq!(o.soft_score, None);
    assert_eq!(o.unknown_soft, vec!["age".to_string()]);
}

#[test]
fn ai_cannot_overwrite_human_data() {
    let mut x = Profile::new("x");
    assert!(x.set("age", Value::Num(30.0), Provenance::User));
    assert!(!x.set("age", Value::Num(25.0), Provenance::AiInferred));
    assert_eq!(x.get("age"), Some(&Value::Num(30.0)));
    assert!(x.set("age", Value::Num(31.0), Provenance::Matchmaker));
}

#[test]
fn ruleset_roundtrips_as_json() {
    let s = serde_json::to_string(&ruleset()).unwrap();
    let back: RuleSet = serde_json::from_str(&s).unwrap();
    assert_eq!(back.version, 3);
    assert_eq!(back.rules.len(), 2);
}
