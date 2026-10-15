use matchmaking_core::profile::{Provenance, Value};
use matchmaking_core::*;
use std::collections::BTreeMap;

fn answers(f: impl Fn(&Item) -> i32, i: &Instrument) -> BTreeMap<String, i32> {
    i.items.iter().map(|it| (it.id.clone(), f(it))).collect()
}

#[test]
fn the_shipped_instrument_is_valid_and_complete() {
    let reg = default_registry();
    let mini = builtin_mini_ipip();
    assert!(validate_instrument(&mini, &reg).is_empty(), "{:?}", validate_instrument(&mini, &reg));
    assert_eq!(mini.items.len(), 20);
    for s in &mini.subscales {
        assert_eq!(mini.items.iter().filter(|i| i.subscale == s.key).count(), 4, "{}: four items per trait", s.key);
        assert!(reg.get(&s.field).map_or(false, |d| d.sensitive), "{} is a default sensitive field", s.field);
    }
    assert!(missing_fields(&mini, &reg).is_empty(), "the default registry already has the Big Five fields");
    // honesty: the record says where it comes from, how strong it is, and what it cannot tell
    assert_eq!(mini.evidence.level, EvidenceLevel::ValidatedInstrument);
    assert!(mini.evidence.sources.len() >= 3 && mini.evidence.limitations.contains("from memory") && mini.evidence.limitations.contains("clinical"));
    assert!(mini.evidence.limitations.contains("similarity"));
}

#[test]
fn scoring_handles_reverse_keys_and_the_scale_midpoint() {
    let mini = builtin_mini_ipip();
    // answering 3 (the midpoint) to everything gives 12 on every trait, reversed or not
    let mid = score_answers(&mini, &answers(|_| 3, &mini));
    assert!(mid.complete());
    assert!(mid.subscales.values().all(|v| *v == 12.0), "{:?}", mid.subscales);
    // agreeing with every statement: positively keyed items give 5, reversed give 1 (2 reversed items of 4 for extraversion)
    let all5 = score_answers(&mini, &answers(|_| 5, &mini));
    assert_eq!(all5.subscales["extraversion"], 12.0, "two items at 5 and two reversed items at 1");
    assert_eq!(all5.subscales["openness"], 8.0, "one item at 5 and three reversed at 1");
    // the maximum for a trait: positive items 5, reversed items 1
    let max_e = score_answers(&mini, &answers(|it| if it.subscale == "extraversion" { if it.reverse { 1 } else { 5 } } else { 3 }, &mini));
    assert_eq!(max_e.subscales["extraversion"], 20.0);
    let min_e = score_answers(&mini, &answers(|it| if it.subscale == "extraversion" { if it.reverse { 5 } else { 1 } } else { 3 }, &mini));
    assert_eq!(min_e.subscales["extraversion"], 4.0);
}

#[test]
fn partial_answers_score_only_complete_subscales() {
    let mini = builtin_mini_ipip();
    let mut a: BTreeMap<String, i32> = BTreeMap::new();
    for it in mini.items.iter().filter(|i| i.subscale == "neuroticism") {
        a.insert(it.id.clone(), 3);
    }
    a.insert("q1".into(), 4); // one of four extraversion items
    let s = score_answers(&mini, &a);
    assert_eq!(s.subscales.len(), 1);
    assert_eq!(s.subscales["neuroticism"], 12.0);
    assert_eq!((s.answered, s.total, s.complete()), (5, 20, false));
    // items are asked in order
    assert_eq!(next_item(&mini, &BTreeMap::new()).unwrap().1.id, "q1");
    assert_eq!(next_item(&mini, &a).unwrap().1.id, "q2");
    assert!(next_item(&mini, &answers(|_| 3, &mini)).is_none());
}

#[test]
fn answers_are_validated() {
    let mini = builtin_mini_ipip();
    assert!(validate_answer(&mini, "q1", 1).is_ok() && validate_answer(&mini, "q1", 5).is_ok());
    assert!(validate_answer(&mini, "q1", 0).is_err() && validate_answer(&mini, "q1", 6).is_err());
    assert!(validate_answer(&mini, "q99", 3).is_err());
}

#[test]
fn scores_become_profile_fields_with_questionnaire_provenance() {
    let mini = builtin_mini_ipip();
    let reg = default_registry();
    let mut p = Profile::new("x");
    let scores = score_answers(&mini, &answers(|_| 3, &mini));
    let applied = apply_scores(&mut p, &mini, &scores);
    assert!(applied.iter().all(|(_, ok)| *ok) && applied.len() == 5);
    assert_eq!(p.get("big5_neuroticism"), Some(&Value::Num(12.0)));
    assert_eq!(p.fields["big5_neuroticism"].source, Provenance::Questionnaire);
    assert!(p.validate(&reg).is_empty());
    // a retake replaces the earlier score
    let retake = score_answers(&mini, &answers(|it| if it.reverse { 1 } else { 5 }, &mini));
    apply_scores(&mut p, &mini, &retake);
    assert_eq!(p.get("big5_neuroticism"), Some(&Value::Num(20.0)));
    // a value a person entered outranks a questionnaire score and is reported as not applied
    let mut q = Profile::new("y");
    q.set("big5_openness", Value::Num(7.0), Provenance::Matchmaker);
    let res = apply_scores(&mut q, &mini, &scores);
    assert!(res.iter().any(|(f, ok)| f == "big5_openness" && !*ok));
    assert_eq!(q.get("big5_openness"), Some(&Value::Num(7.0)));
}

fn custom() -> Instrument {
    Instrument {
        key: "family_views".into(),
        name: "Family views".into(),
        version: 1,
        kind: "custom".into(),
        dimension: "family".into(),
        description: String::new(),
        evidence: Evidence { level: EvidenceLevel::MatchmakerDefined, sources: vec![], limitations: "Not validated: for conversation only.".into(), interpretation: "Higher means more traditional views.".into() },
        scale: Scale { min: 1, max: 5, labels: ["Strongly disagree", "Disagree", "Neutral", "Agree", "Strongly agree"].iter().map(|s| s.to_string()).collect() },
        items: vec![
            Item { id: "a1".into(), text: "Family decisions should involve elders.".into(), subscale: "tradition".into(), reverse: false },
            Item { id: "a2".into(), text: "Couples should live apart from both families.".into(), subscale: "tradition".into(), reverse: true },
            Item { id: "a3".into(), text: "Holidays should be spent with extended family.".into(), subscale: "tradition".into(), reverse: false },
        ],
        subscales: vec![Subscale { key: "tradition".into(), label: "Traditional family life".into(), field: "family_tradition".into(), method: Method::Mean }],
    }
}

#[test]
fn custom_instruments_are_labelled_validated_and_create_their_fields() {
    let reg = default_registry();
    let c = custom();
    assert!(validate_instrument(&c, &reg).is_empty(), "{:?}", validate_instrument(&c, &reg));
    let new_fields = missing_fields(&c, &reg);
    assert_eq!(new_fields.len(), 1);
    assert_eq!(new_fields[0].key, "family_tradition");
    assert!(new_fields[0].sensitive && matches!(new_fields[0].kind, FieldKind::Number), "scores start as sensitive numeric fields");
    // mean scoring with a reversed item: (5 + (6-4) + 3) / 3
    let a = BTreeMap::from([("a1".to_string(), 5), ("a2".to_string(), 4), ("a3".to_string(), 3)]);
    assert!((score_answers(&c, &a).subscales["tradition"] - 10.0 / 3.0).abs() < 1e-9);
    assert_eq!(EvidenceLevel::MatchmakerDefined.label(), "Written by the matchmaker (not validated)");
}

#[test]
fn bad_definitions_are_rejected_with_reasons() {
    let reg = default_registry();
    let check = |f: &dyn Fn(&mut Instrument), expect: &str| {
        let mut c = custom();
        f(&mut c);
        let issues = validate_instrument(&c, &reg).join(" | ");
        assert!(issues.contains(expect), "expected '{expect}' in: {issues}");
    };
    check(&|c| c.key = "Bad Key".into(), "key must start");
    check(&|c| c.name = " ".into(), "needs a name");
    check(&|c| c.dimension = "nonsense".into(), "not a scorecard dimension");
    check(&|c| c.scale.labels.pop().map(|_| ()).unwrap_or(()), "scale needs");
    check(&|c| c.scale = Scale { min: 1, max: 9, labels: (1..=9).map(|n| n.to_string()).collect() }, "scale needs");
    check(&|c| c.items.truncate(2), "between 3 and 100");
    check(&|c| c.items[1].id = "a1".into(), "duplicated");
    check(&|c| c.items[0].subscale = "ghost".into(), "unknown subscale");
    check(&|c| c.items[0].text = String::new(), "needs text");
    check(&|c| c.subscales.clear(), "At least one subscale");
    check(&|c| c.subscales[0].field = "city".into(), "not a number field");
    check(&|c| c.evidence.limitations = String::new(), "cannot tell you");
    check(&|c| { c.evidence.level = EvidenceLevel::ValidatedInstrument; }, "must cite its source");
    check(&|c| c.subscales.push(Subscale { key: "empty".into(), label: "Empty".into(), field: "x_empty".into(), method: Method::Sum }), "has no items");
}

#[test]
fn instruments_round_trip_as_json_and_starter_personality_rules_are_off() {
    let mini = builtin_mini_ipip();
    let back: Instrument = serde_json::from_str(&serde_json::to_string(&mini).unwrap()).unwrap();
    assert_eq!(back, mini);
    let json = serde_json::to_value(&mini).unwrap();
    assert_eq!(json["evidence"]["level"], "validated_instrument");
    // the starter rule set mentions personality only in disabled examples, so the dimension stays "not assessed" until the matchmaker opts in
    let set = default_ruleset();
    let personality: Vec<&Rule> = set.rules.iter().filter(|r| r.group.as_deref() == Some("personality")).collect();
    assert_eq!(personality.len(), 2);
    assert!(personality.iter().all(|r| !r.enabled && r.description.contains("Example")));
    assert!(validate_ruleset(&set, &default_registry()).is_empty());
    let a = Profile::new("a");
    let card = score_pair(&set, &ScoringConfig::default(), &a, &Profile::new("b"), &[], &[]).unwrap();
    assert_eq!(card.dimensions.iter().find(|d| d.key == "personality").unwrap().status, DimStatus::NotAssessed);
}

#[test]
fn evidence_and_dimension_labels_follow_the_locale() {
    use matchmaking_core::{dimension_catalog, Locale};
    assert_eq!(EvidenceLevel::MatchmakerDefined.label_for(Locale::En), EvidenceLevel::MatchmakerDefined.label());
    assert!(EvidenceLevel::AdaptedInstrument.label_for(Locale::Fa).contains("اقتباس"));
    for def in dimension_catalog() {
        assert_ne!(def.label_for(Locale::Fa), def.label, "{}", def.key);
        assert!(!def.description_for(Locale::Fa).is_empty());
    }
}

#[test]
fn scorecard_findings_are_described_in_persian_for_starter_rules() {
    use matchmaking_core::{default_ruleset, localize_scorecard_json, Locale, Profile, ScoringConfig};
    let set = default_ruleset();
    let mut card = score_pair(&set, &ScoringConfig::default(), &Profile::new("a"), &Profile::new("b"), &[], &[]).unwrap();
    let mut json = serde_json::to_value(&card).unwrap();
    card.localize(Locale::Fa, &|_, _| None);
    localize_scorecard_json(&mut json, Locale::Fa, &|_, _| None);
    let all = card.strengths.iter().chain(&card.concerns).chain(&card.unknowns).collect::<Vec<_>>();
    assert!(!all.is_empty());
    assert!(all.iter().all(|f| f.description.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c))));
    assert_eq!(json["dimensions"][0]["label"], "سن و مرحله زندگی");
    assert_eq!(card.dimensions[0].label, "سن و مرحله زندگی");
}

#[test]
fn answer_and_instrument_errors_follow_the_locale() {
    use matchmaking_core::{validate_answer_for, validate_instrument_for, FieldRegistry, Locale};
    let mini = builtin_mini_ipip();
    let range = validate_answer_for(&mini, "q1", 99, Locale::Fa).unwrap_err();
    assert!(range.contains("پاسخ باید بین"));
    assert!(validate_answer_for(&mini, "nope", 3, Locale::En).unwrap_err().contains("There is no item"));
    let mut broken = mini.clone();
    broken.items.clear();
    let issues = validate_instrument_for(&broken, &FieldRegistry::default(), Locale::Fa);
    assert!(issues.iter().any(|issue| issue.contains("گویه")));
}

#[test]
fn the_shipped_instrument_reads_in_persian_without_changing_its_structure() {
    let english = builtin_mini_ipip();
    let persian = english.clone().localized(matchmaking_core::Locale::Fa);
    assert_ne!(persian.name, english.name);
    assert_eq!(persian.key, english.key);
    assert_eq!(persian.items.len(), english.items.len());
    for (fa_item, en_item) in persian.items.iter().zip(&english.items) {
        assert_ne!(fa_item.text, en_item.text);
        assert_eq!((&fa_item.id, &fa_item.subscale, fa_item.reverse), (&en_item.id, &en_item.subscale, en_item.reverse));
    }
    assert!(persian.subscales.iter().all(|sub| sub.label.chars().any(|ch| ('\u{0600}'..='\u{06FF}').contains(&ch))));
    assert_eq!(english.clone().localized(matchmaking_core::Locale::En).name, english.name);
}
