use matchmaking_core::profile::{Provenance, Value};
use matchmaking_core::*;
use std::collections::BTreeMap;
use std::time::Instant;

fn run(text: &str) -> BTreeMap<String, (Value, String)> {
    let reg = default_registry();
    let out = extract_with_rules(&reg, text, None, &Profile::new("x"));
    out.suggestions.into_iter().map(|s| (s.field, (s.value, s.evidence))).collect()
}

fn value(text: &str, field: &str) -> Option<Value> {
    run(text).get(field).map(|(v, _)| v.clone())
}

fn num(x: f64) -> Option<Value> { Some(Value::Num(x)) }
fn txt(s: &str) -> Option<Value> { Some(Value::Text(s.into())) }

#[test]
fn age_in_english_and_persian_with_persian_digits() {
    assert_eq!(value("She is 28 years old.", "age"), num(28.0));
    assert_eq!(value("خانمی ۲۸ ساله", "age"), num(28.0));
    assert_eq!(value("سن ۳۱", "age"), num(31.0));
    assert_eq!(value("سن ٣٣", "age"), num(33.0));
}

#[test]
fn out_of_range_values_are_rejected() {
    assert_eq!(value("12 years old", "age"), None);
    assert_eq!(value("150 ساله", "age"), None);
    assert_eq!(value("height 90 cm", "height_cm"), None);
    assert_eq!(value("300 kg", "weight_kg"), None);
}

#[test]
fn height_and_weight() {
    assert_eq!(value("165 cm tall", "height_cm"), num(165.0));
    assert_eq!(value("قد ۱۷۵ سانت", "height_cm"), num(175.0));
    assert_eq!(value("1.80 m", "height_cm"), num(180.0));
    assert_eq!(value("62 kg", "weight_kg"), num(62.0));
    assert_eq!(value("وزن ۷۰ کیلو", "weight_kg"), num(70.0));
}

#[test]
fn gender_marital_and_children() {
    assert_eq!(value("A woman, never married.", "gender"), txt("female"));
    assert_eq!(value("A man from Tehran", "gender"), txt("male"));
    assert_eq!(value("آقایی مجرد", "marital_status"), txt("never_married"));
    assert_eq!(value("She is divorced", "marital_status"), txt("divorced"));
    assert_eq!(value("no children", "has_children"), Some(Value::Bool(false)));
    assert_eq!(value("فرزند دارد", "has_children"), Some(Value::Bool(true)));
    assert_eq!(value("has 2 children", "children_count"), num(2.0));
    assert_eq!(value("three kids", "children_count"), num(3.0));
    assert_eq!(value("سه فرزند", "children_count"), num(3.0));
}

#[test]
fn siblings_are_summed_not_concatenated() {
    assert_eq!(value("تعداد خواهر و برادر: ۲ خواهر و ۱ برادر", "siblings_count"), num(3.0));
    assert_eq!(value("number of siblings: 2 sisters and 1 brother", "siblings_count"), num(3.0));
    assert_eq!(value("siblings count: 4", "siblings_count"), num(4.0));
}

#[test]
fn education_longest_match_wins() {
    assert_eq!(value("کارشناسی ارشد مهندسی", "education"), txt("master"));
    assert_eq!(value("کارشناسی مهندسی", "education"), txt("bachelor"));
    assert_eq!(value("فوق دیپلم", "education"), txt("other"));
    assert_eq!(value("دیپلم", "education"), txt("diploma"));
    assert_eq!(value("PhD in physics", "education"), txt("doctorate"));
}

#[test]
fn keywords_do_not_match_inside_longer_words() {
    assert_eq!(value("woman", "gender"), txt("female"));
    assert_eq!(value("human", "gender"), None);
}

#[test]
fn partner_preference_clauses_are_ignored() {
    let found = run("دنبال خانمی ۲۵ ساله");
    assert!(!found.contains_key("age") && !found.contains_key("gender"), "{found:?}");
    let found = run("He is looking for a woman, 25 years old. He is 31 years old.");
    assert_eq!(found.get("age").map(|(v, _)| v.clone()), num(31.0));
    assert!(!found.contains_key("gender") || found["gender"].0 != Value::Text("female".into()));
}

#[test]
fn conflicting_values_become_ambiguous_with_no_suggestion() {
    let reg = default_registry();
    let out = extract_with_rules(&reg, "28 years old. Also 35 years old.", None, &Profile::new("x"));
    assert!(out.suggestions.iter().all(|s| s.field != "age"));
    assert!(out.ambiguous.contains("age"));
}

#[test]
fn stored_values_are_not_suggested_again() {
    let reg = default_registry();
    let mut profile = Profile::new("x");
    profile.set("age", Value::Num(40.0), Provenance::User);
    let out = extract_with_rules(&reg, "28 years old, 170 cm", None, &profile);
    assert!(out.suggestions.iter().all(|s| s.field != "age"));
    assert!(out.suggestions.iter().any(|s| s.field == "height_cm"));
}

#[test]
fn targets_limit_the_fields() {
    let reg = default_registry();
    let targets = vec!["height_cm".to_string()];
    let out = extract_with_rules(&reg, "28 years old, 170 cm", Some(&targets), &Profile::new("x"));
    assert_eq!(out.suggestions.len(), 1);
    assert_eq!(out.suggestions[0].field, "height_cm");
}

#[test]
fn contact_details_are_skipped_but_names_are_read() {
    let reg = default_registry();
    let out = extract_with_rules(&reg, "full name: Sara Karimi\nphone: 09123456789\ntelegram: @sara", None, &Profile::new("x"));
    assert_eq!(value("full name: Sara Karimi", "full_name"), txt("Sara Karimi"));
    assert!(out.suggestions.iter().all(|s| s.field != "phone" && s.field != "telegram_username"), "{:?}", out.suggestions);
}

#[test]
fn aliases_resolve_a_noisy_form() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extraction/fa-aliases.json");
    let sample: Sample = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let found = run(&sample.text);
    for (field, want) in sample.expected {
        let Some((v, _)) = found.get(&field) else { panic!("{field} missing: {found:?}") };
        let got = serde_json::to_value(v).unwrap();
        assert!(got == want || value_matches(&want, &got), "{field}: {found:?}");
    }
    assert!(!found.contains_key("gender") && !found.contains_key("employment_status"), "{found:?}");
}

#[test]
fn labelled_lines_for_each_custom_field_kind() {
    let mut reg = default_registry();
    let def = |key: &str, kind: FieldKind| FieldDef { key: key.into(), label: key.replace('_', " "), label_fa: None, kind, sensitive: false, required: false };
    reg.register(def("siblings", FieldKind::Number));
    reg.register(def("owns_car", FieldKind::Bool));
    reg.register(def("diet", FieldKind::Choice(vec!["vegan".into(), "omnivore".into()])));
    reg.register(def("languages", FieldKind::MultiChoice(vec!["fa".into(), "en".into()])));
    reg.register(def("hobby", FieldKind::Text));
    let text = "siblings: 3\nowns car: yes\ndiet: vegan\nlanguages: fa, en\nhobby: hiking in the mountains";
    let out = extract_with_rules(&reg, text, None, &Profile::new("x"));
    let got: BTreeMap<_, _> = out.suggestions.iter().map(|s| (s.field.as_str(), s.value.clone())).collect();
    assert_eq!(got["siblings"], Value::Num(3.0));
    assert_eq!(got["owns_car"], Value::Bool(true));
    assert_eq!(got["diet"], Value::Text("vegan".into()));
    assert_eq!(got["hobby"], Value::Text("hiking in the mountains".into()));
    assert!(got.contains_key("languages"));
    assert!(out.suggestions.iter().all(|s| s.confidence <= 0.95 && s.source == SuggestionSource::Rule));
}

#[test]
fn evidence_is_an_exact_substring_of_the_original() {
    let text = "نام: سارا\nسن ۲۸ ساله، قد ۱۶۵ سانت، وزن ۶۰ کیلو، کارشناسی ارشد، ساکن تهران.\nAlso: 30 years old? lives in Shiraz, 1.65 m";
    let reg = default_registry();
    let out = extract_with_rules(&reg, text, None, &Profile::new("x"));
    assert!(!out.suggestions.is_empty());
    for suggestion in out.suggestions {
        assert!(text.contains(&suggestion.evidence), "{:?} not in text", suggestion.evidence);
    }
}

#[test]
fn output_is_deterministic() {
    let text = "28 years old woman, 165 cm, 60 kg, master, lives in Tehran, never married, no children";
    let reg = default_registry();
    let first = extract_with_rules(&reg, text, None, &Profile::new("x"));
    for _ in 0..5 {
        assert_eq!(first, extract_with_rules(&reg, text, None, &Profile::new("x")));
    }
}

#[test]
fn twenty_thousand_characters_are_read_quickly() {
    let chunk = "She is 28 years old, 165 cm, lives in Tehran. دنبال همسری ۳۰ ساله. Loves hiking and books.\n";
    let text = chunk.repeat(20_000 / chunk.chars().count() + 1);
    let reg = default_registry();
    let started = Instant::now();
    let _ = extract_with_rules(&reg, &text, None, &Profile::new("x"));
    let limit = if cfg!(debug_assertions) { 2000 } else { 20 };
    assert!(started.elapsed().as_millis() < limit, "took {:?}", started.elapsed());
}

#[derive(serde::Deserialize)]
struct Sample {
    text: String,
    expected: BTreeMap<String, serde_json::Value>,
}

#[test]
fn golden_corpus_precision_and_recall() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/extraction");
    let reg = default_registry();
    let (mut true_positive, mut returned, mut expected_total) = (0usize, 0usize, 0usize);
    let mut files: Vec<_> = std::fs::read_dir(&dir).expect("fixtures").filter_map(Result::ok).map(|entry| entry.path()).collect();
    files.sort();
    assert!(files.len() >= 30, "corpus too small: {}", files.len());
    for path in files {
        let sample: Sample = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let out = extract_with_rules(&reg, &sample.text, None, &Profile::new("x"));
        expected_total += sample.expected.len();
        for suggestion in out.suggestions {
            returned += 1;
            let got = serde_json::to_value(&suggestion.value).unwrap();
            let want = sample.expected.get(&suggestion.field).map(|v| v == &got || value_matches(v, &got));
            if want == Some(true) {
                true_positive += 1;
            } else {
                eprintln!("{}: unexpected {} = {got}", path.display(), suggestion.field);
            }
        }
    }
    let precision = true_positive as f64 / returned.max(1) as f64;
    let recall = true_positive as f64 / expected_total.max(1) as f64;
    eprintln!("rules precision {precision:.3} recall {recall:.3} ({true_positive}/{returned}, expected {expected_total})");
    assert!(precision >= 0.98, "precision {precision:.3}");
}

fn value_matches(want: &serde_json::Value, got: &serde_json::Value) -> bool {
    match (want.as_f64(), got.as_f64()) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-6,
        _ => false,
    }
}

#[test]
fn slim_prompt_lists_only_targets_and_parser_drops_the_rest() {
    let reg = default_registry();
    let fields = vec!["age".to_string(), "diet_unknown".to_string()];
    let (_, user) = slim_extraction_prompt_for(&reg, "She is 28 years old", &fields, Locale::En, false);
    assert!(user.contains("age") && !user.contains("height_cm") && !user.contains("full_name"));
    let raw = r#"{"fields":{"age":{"value":28,"evidence":"28 years old"},"height_cm":{"value":170,"evidence":"170"},"gender":{"value":"female","evidence":"made up"}}}"#;
    let parsed = parse_slim_extraction(raw, &reg, "She is 28 years old", &Profile::new("x"), false, &["age".to_string()]).unwrap();
    assert_eq!(parsed.suggestions.len(), 1);
    assert_eq!(parsed.suggestions[0].field, "age");
}

#[test]
fn constraint_schema_accepts_valid_and_rejects_bad_output() {
    let reg = default_registry();
    let fields = vec!["gender".to_string(), "age".to_string()];
    let constraint = output_constraint(&reg, &fields, false);
    let schema = &constraint.json_schema;
    let props = &schema["properties"]["fields"]["properties"];
    assert_eq!(props["gender"]["properties"]["value"]["enum"], serde_json::json!(["female", "male"]));
    assert_eq!(props["age"]["properties"]["value"]["type"], "number");
    assert_eq!(schema["properties"]["fields"]["additionalProperties"], false);
    assert!(props.get("height_cm").is_none());
    assert!(constraint.gbnf.contains("root ::="));
    assert!(!constraint.gbnf.contains("list-value"));
    assert!(output_constraint(&reg, &fields, true).json_schema["properties"].get("preferences").is_some());
}
