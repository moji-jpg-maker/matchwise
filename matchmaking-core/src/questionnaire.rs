//! Questionnaires and the evidence behind them.
//!
//! A questionnaire is *data*: an instrument with items, a response scale, subscales, and an **evidence record** saying
//! where it comes from, how strong the evidence is, and what it cannot tell you. Scores become ordinary profile fields
//! (with "questionnaire" provenance), so rules and the scorecard use them like any other field.
//!
//! The registry exists to keep the application honest: an instrument written by the matchmaker is labelled as exactly
//! that, and nothing here diagnoses a condition or presents a score as a clinical assessment.

use crate::field::{FieldDef, FieldKind, FieldRegistry};
use crate::profile::{Profile, Provenance, Value};
use crate::scoring::dimension_catalog;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceLevel {
    /// A published instrument with reported reliability and validity, used as published.
    ValidatedInstrument,
    /// Based on a published instrument but shortened, reworded or translated: its evidence does not carry over automatically.
    AdaptedInstrument,
    /// Written by the matchmaker. Useful for conversation and for structured notes; no evidence of validity.
    MatchmakerDefined,
}

impl EvidenceLevel {
    pub fn label(self) -> &'static str {
        match self {
            EvidenceLevel::ValidatedInstrument => "Published, validated instrument",
            EvidenceLevel::AdaptedInstrument => "Adapted from a published instrument",
            EvidenceLevel::MatchmakerDefined => "Written by the matchmaker (not validated)",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceSource {
    pub citation: String,
    /// What the source shows, in a sentence.
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub level: EvidenceLevel,
    #[serde(default)]
    pub sources: Vec<EvidenceSource>,
    /// What this questionnaire cannot tell you.
    #[serde(default)]
    pub limitations: String,
    /// How a matchmaker should read the scores.
    #[serde(default)]
    pub interpretation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    pub min: i32,
    pub max: i32,
    /// One label per point, lowest first.
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub subscale: String,
    #[serde(default)]
    pub reverse: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Sum,
    Mean,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subscale {
    pub key: String,
    pub label: String,
    /// The profile field (numeric) that receives this score.
    pub field: String,
    pub method: Method,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    pub key: String,
    pub name: String,
    pub version: u32,
    /// "standard" (shipped with the app) or "custom"
    pub kind: String,
    /// The scorecard dimension this instrument informs (a key from the dimension catalog).
    pub dimension: String,
    #[serde(default)]
    pub description: String,
    pub evidence: Evidence,
    pub scale: Scale,
    pub items: Vec<Item>,
    pub subscales: Vec<Subscale>,
}

fn slug_ok(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.chars().next().map_or(false, |c| c.is_ascii_lowercase()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Everything wrong with an instrument definition (empty = fine). `registry` is used to check target fields.
pub fn validate_instrument(i: &Instrument, registry: &FieldRegistry) -> Vec<String> {
    let mut out = vec![];
    if !slug_ok(&i.key, 40) {
        out.push("The key must start with a lowercase letter and use only a-z, 0-9 and _ (max 40 characters)".into());
    }
    if i.name.trim().is_empty() || i.name.len() > 120 {
        out.push("The questionnaire needs a name (max 120 characters)".into());
    }
    if i.kind != "standard" && i.kind != "custom" {
        out.push("Kind must be 'standard' or 'custom'".into());
    }
    if !dimension_catalog().iter().any(|d| d.key == i.dimension) {
        out.push(format!("'{}' is not a scorecard dimension", i.dimension));
    }
    let n_points = i.scale.max - i.scale.min + 1;
    if !(2..=7).contains(&n_points) || i.scale.labels.len() as i32 != n_points || i.scale.labels.iter().any(|l| l.trim().is_empty()) {
        out.push("The scale needs 2 to 7 points with a label for each".into());
    }
    if i.evidence.level == EvidenceLevel::ValidatedInstrument && i.evidence.sources.is_empty() {
        out.push("A validated instrument must cite its source".into());
    }
    if i.evidence.limitations.trim().is_empty() {
        out.push("State what this questionnaire cannot tell you".into());
    }
    if !(3..=100).contains(&i.items.len()) {
        out.push("A questionnaire needs between 3 and 100 items".into());
    }
    let mut ids = BTreeSet::new();
    let sub_keys: BTreeSet<&str> = i.subscales.iter().map(|s| s.key.as_str()).collect();
    for it in &i.items {
        if !slug_ok(&it.id, 20) || !ids.insert(it.id.clone()) {
            out.push(format!("Item id '{}' is invalid or duplicated", it.id));
        }
        if it.text.trim().is_empty() || it.text.chars().count() > 300 {
            out.push(format!("Item {} needs text of 1 to 300 characters", it.id));
        }
        if !sub_keys.contains(it.subscale.as_str()) {
            out.push(format!("Item {} refers to an unknown subscale '{}'", it.id, it.subscale));
        }
    }
    if i.subscales.is_empty() {
        out.push("At least one subscale is needed: it decides which profile field receives the score".into());
    }
    let mut fields = BTreeSet::new();
    let mut keys = BTreeSet::new();
    for s in &i.subscales {
        if !slug_ok(&s.key, 40) || !keys.insert(s.key.clone()) {
            out.push(format!("Subscale key '{}' is invalid or duplicated", s.key));
        }
        if s.label.trim().is_empty() {
            out.push(format!("Subscale '{}' needs a label", s.key));
        }
        if !slug_ok(&s.field, 64) || !fields.insert(s.field.clone()) {
            out.push(format!("Field '{}' is invalid or used twice", s.field));
        } else if let Some(def) = registry.get(&s.field) {
            if !matches!(def.kind, FieldKind::Number) {
                out.push(format!("Field '{}' exists but is not a number field", s.field));
            }
        }
        if !i.items.iter().any(|it| it.subscale == s.key) {
            out.push(format!("Subscale '{}' has no items", s.key));
        }
    }
    out
}

/// The numeric fields a new instrument needs that do not exist yet. Scores are psychological information, so they start as sensitive.
pub fn missing_fields(i: &Instrument, registry: &FieldRegistry) -> Vec<FieldDef> {
    i.subscales
        .iter()
        .filter(|s| registry.get(&s.field).is_none())
        .map(|s| FieldDef { key: s.field.clone(), label: format!("{} ({})", s.label, i.name), kind: FieldKind::Number, sensitive: true, required: false })
        .collect()
}

pub fn validate_answer(i: &Instrument, item_id: &str, value: i32) -> Result<(), String> {
    if !i.items.iter().any(|it| it.id == item_id) {
        return Err(format!("There is no item '{item_id}'"));
    }
    if value < i.scale.min || value > i.scale.max {
        return Err(format!("An answer must be between {} and {}", i.scale.min, i.scale.max));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    /// Subscale key -> score. A subscale is scored only when all of its items are answered.
    pub subscales: BTreeMap<String, f64>,
    pub answered: usize,
    pub total: usize,
}

impl Scores {
    pub fn complete(&self) -> bool {
        self.answered == self.total
    }
}

/// Score answers (item id -> value), reversing reverse-keyed items. Unanswered items leave their subscale unscored.
pub fn score(i: &Instrument, answers: &BTreeMap<String, i32>) -> Scores {
    let mut subscales = BTreeMap::new();
    for s in &i.subscales {
        let items: Vec<&Item> = i.items.iter().filter(|it| it.subscale == s.key).collect();
        let vals: Vec<f64> = items
            .iter()
            .filter_map(|it| answers.get(&it.id).map(|v| if it.reverse { (i.scale.min + i.scale.max - v) as f64 } else { *v as f64 }))
            .collect();
        if !items.is_empty() && vals.len() == items.len() {
            let sum: f64 = vals.iter().sum();
            subscales.insert(s.key.clone(), match s.method { Method::Sum => sum, Method::Mean => sum / vals.len() as f64 });
        }
    }
    Scores { subscales, answered: i.items.iter().filter(|it| answers.contains_key(&it.id)).count(), total: i.items.len() }
}

/// Write finished subscale scores into the profile with questionnaire provenance. A value entered by a person
/// outranks a questionnaire score and is left alone (reported as not applied).
pub fn apply_scores(profile: &mut Profile, i: &Instrument, scores: &Scores) -> Vec<(String, bool)> {
    let mut out = vec![];
    for s in &i.subscales {
        if let Some(v) = scores.subscales.get(&s.key) {
            out.push((s.field.clone(), profile.set(&s.field, Value::Num((*v * 100.0).round() / 100.0), Provenance::Questionnaire)));
        }
    }
    out
}

/// The questionnaire's next unanswered item, in order.
pub fn next_item<'a>(i: &'a Instrument, answers: &BTreeMap<String, i32>) -> Option<(usize, &'a Item)> {
    i.items.iter().enumerate().find(|(_, it)| !answers.contains_key(&it.id))
}

/// The Mini-IPIP (Donnellan, Oswald, Baird & Lucas, 2006): 20 items, public domain (from the International Personality Item Pool).
/// Item wording and keying were reproduced from memory of the published scale, so the evidence record says to check them against the source.
pub fn builtin_mini_ipip() -> Instrument {
    let q = |n: usize, text: &str, sub: &str, rev: bool| Item { id: format!("q{n}"), text: text.to_string(), subscale: sub.to_string(), reverse: rev };
    let sc = |key: &str, label: &str, field: &str| Subscale { key: key.into(), label: label.into(), field: field.into(), method: Method::Sum };
    Instrument {
        key: "mini_ipip".into(),
        name: "Mini-IPIP (Big Five, 20 items)".into(),
        version: 1,
        kind: "standard".into(),
        dimension: "personality".into(),
        description: "How much each statement describes you. Twenty short statements; about three minutes.".into(),
        evidence: Evidence {
            level: EvidenceLevel::ValidatedInstrument,
            sources: vec![
                EvidenceSource {
                    citation: "Donnellan, M. B., Oswald, F. L., Baird, B. M., & Lucas, R. E. (2006). The Mini-IPIP scales: Tiny-yet-effective measures of the Big Five factors of personality. Psychological Assessment, 18(2), 192-203.".into(),
                    note: "Introduces the 20-item scale (four items per trait) and reports acceptable reliability and validity for research use; reliabilities are lower than for longer scales.".into(),
                },
                EvidenceSource {
                    citation: "Goldberg, L. R., et al. (2006). The International Personality Item Pool and the future of public-domain personality measures. Journal of Research in Personality, 40, 84-96.".into(),
                    note: "The IPIP items are in the public domain (ipip.ori.org), so the scale can be used freely.".into(),
                },
                EvidenceSource {
                    citation: "Dyrenforth, P. S., Kashy, D. A., Donnellan, M. B., & Lucas, R. E. (2010). Predicting relationship and life satisfaction from personality in nationally representative samples from three countries: The relative importance of actor, partner, and similarity effects. Journal of Personality and Social Psychology, 99(4), 690-702.".into(),
                    note: "Found that a person's own traits and their partner's traits relate modestly to satisfaction, while similarity between partners added almost nothing.".into(),
                },
                EvidenceSource {
                    citation: "Joel, S., Eastwick, P. W., Allison, C. J., et al. (2020). Machine learning uncovers the most robust self-report predictors of relationship quality across 43 longitudinal couples studies. Proceedings of the National Academy of Sciences, 117(32), 19061-19071.".into(),
                    note: "Across many studies, relationship-specific variables predicted relationship quality far better than individual traits, and pre-relationship compatibility added little.".into(),
                },
            ],
            limitations: "Check the item wording and keying against the published scale before relying on the scores: they were reproduced from memory. \
Self-reported traits explain only a small part of how a relationship turns out, and similarity between two people's trait scores has not been found to predict satisfaction. \
Four items per trait gives a rough picture, not a precise measurement. Scores describe self-reported tendencies; they are not a clinical assessment and say nothing about whether a person is good or bad. \
Scores can differ between cultures and languages, and this wording is English only."
                .into(),
            interpretation: "Each trait score runs from 4 to 20 (midpoint 12). Higher means the person describes themselves as more extraverted, agreeable, conscientious, emotionally unstable (neuroticism) or imaginative/intellectual. \
Treat the scores as conversation starters. The matching rules that use them are switched off by default for the reasons above."
                .into(),
        },
        scale: Scale { min: 1, max: 5, labels: ["Very inaccurate", "Moderately inaccurate", "Neither accurate nor inaccurate", "Moderately accurate", "Very accurate"].iter().map(|s| s.to_string()).collect() },
        items: vec![
            q(1, "Am the life of the party.", "extraversion", false),
            q(2, "Sympathize with others' feelings.", "agreeableness", false),
            q(3, "Get chores done right away.", "conscientiousness", false),
            q(4, "Have frequent mood swings.", "neuroticism", false),
            q(5, "Have a vivid imagination.", "openness", false),
            q(6, "Don't talk a lot.", "extraversion", true),
            q(7, "Am not interested in other people's problems.", "agreeableness", true),
            q(8, "Often forget to put things back in their proper place.", "conscientiousness", true),
            q(9, "Am relaxed most of the time.", "neuroticism", true),
            q(10, "Am not interested in abstract ideas.", "openness", true),
            q(11, "Talk to a lot of different people at parties.", "extraversion", false),
            q(12, "Feel others' emotions.", "agreeableness", false),
            q(13, "Like order.", "conscientiousness", false),
            q(14, "Get upset easily.", "neuroticism", false),
            q(15, "Have difficulty understanding abstract ideas.", "openness", true),
            q(16, "Keep in the background.", "extraversion", true),
            q(17, "Am not really interested in others.", "agreeableness", true),
            q(18, "Make a mess of things.", "conscientiousness", true),
            q(19, "Seldom feel blue.", "neuroticism", true),
            q(20, "Do not have a good imagination.", "openness", true),
        ],
        subscales: vec![
            sc("extraversion", "Extraversion", "big5_extraversion"),
            sc("agreeableness", "Agreeableness", "big5_agreeableness"),
            sc("conscientiousness", "Conscientiousness", "big5_conscientiousness"),
            sc("neuroticism", "Neuroticism", "big5_neuroticism"),
            sc("openness", "Intellect / imagination", "big5_openness"),
        ],
    }
}
