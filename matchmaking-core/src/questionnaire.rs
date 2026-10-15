//! Questionnaires and the evidence behind them.
//!
//! A questionnaire is *data*: an instrument with items, a response scale, subscales, and an **evidence record** saying
//! where it comes from, how strong the evidence is, and what it cannot tell you. Scores become ordinary profile fields
//! (with "questionnaire" provenance), so rules and the scorecard use them like any other field.
//!
//! The registry exists to keep the application honest: an instrument written by the matchmaker is labelled as exactly
//! that, and nothing here diagnoses a condition or presents a score as a clinical assessment.

use crate::locale::Locale;
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
        self.label_for(Locale::En)
    }

    pub fn label_for(self, locale: Locale) -> &'static str {
        match self {
            EvidenceLevel::ValidatedInstrument => locale.pick("Published, validated instrument", "ابزار منتشرشده و معتبر‌سنجی‌شده"),
            EvidenceLevel::AdaptedInstrument => locale.pick("Adapted from a published instrument", "اقتباس از یک ابزار منتشرشده"),
            EvidenceLevel::MatchmakerDefined => locale.pick("Written by the matchmaker (not validated)", "نوشته‌شده توسط خواستگارباز (معتبر‌سنجی‌نشده)"),
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
    validate_instrument_for(i, registry, Locale::En)
}

pub fn validate_instrument_for(i: &Instrument, registry: &FieldRegistry, locale: Locale) -> Vec<String> {
    let fa = locale == Locale::Fa;
    let mut out = vec![];
    if !slug_ok(&i.key, 40) {
        out.push(if fa { "کلید باید با یک حرف کوچک انگلیسی شروع شود و فقط شامل a-z، ارقام 0-9 و _ باشد (حداکثر ۴۰ نویسه)".into() } else { "The key must start with a lowercase letter and use only a-z, 0-9 and _ (max 40 characters)".into() });
    }
    if i.name.trim().is_empty() || i.name.len() > 120 {
        out.push(locale.pick("The questionnaire needs a name (max 120 characters)", "پرسشنامه به نام نیاز دارد (حداکثر ۱۲۰ نویسه)").into());
    }
    if i.kind != "standard" && i.kind != "custom" {
        out.push(locale.pick("Kind must be 'standard' or 'custom'", "نوع باید «standard» یا «custom» باشد").into());
    }
    if !dimension_catalog().iter().any(|d| d.key == i.dimension) {
        out.push(if fa { format!("«{}» یک بُعد کارنامه نیست", i.dimension) } else { format!("'{}' is not a scorecard dimension", i.dimension) });
    }
    let n_points = i.scale.max - i.scale.min + 1;
    if !(2..=7).contains(&n_points) || i.scale.labels.len() as i32 != n_points || i.scale.labels.iter().any(|l| l.trim().is_empty()) {
        out.push(locale.pick("The scale needs 2 to 7 points with a label for each", "مقیاس به ۲ تا ۷ نقطه نیاز دارد و هر نقطه باید برچسب داشته باشد").into());
    }
    if i.evidence.level == EvidenceLevel::ValidatedInstrument && i.evidence.sources.is_empty() {
        out.push(locale.pick("A validated instrument must cite its source", "ابزار معتبرسنجی‌شده باید منبع خود را ذکر کند").into());
    }
    if i.evidence.limitations.trim().is_empty() {
        out.push(locale.pick("State what this questionnaire cannot tell you", "بنویسید این پرسشنامه چه چیزی را نمی‌تواند نشان دهد").into());
    }
    if !(3..=100).contains(&i.items.len()) {
        out.push(locale.pick("A questionnaire needs between 3 and 100 items", "پرسشنامه به ۳ تا ۱۰۰ گویه نیاز دارد").into());
    }
    let mut ids = BTreeSet::new();
    let sub_keys: BTreeSet<&str> = i.subscales.iter().map(|s| s.key.as_str()).collect();
    for it in &i.items {
        if !slug_ok(&it.id, 20) || !ids.insert(it.id.clone()) {
            out.push(if fa { format!("شناسهٔ گویه «{}» نامعتبر یا تکراری است", it.id) } else { format!("Item id '{}' is invalid or duplicated", it.id) });
        }
        if it.text.trim().is_empty() || it.text.chars().count() > 300 {
            out.push(if fa { format!("گویهٔ {} به متنی بین ۱ تا ۳۰۰ نویسه نیاز دارد", it.id) } else { format!("Item {} needs text of 1 to 300 characters", it.id) });
        }
        if !sub_keys.contains(it.subscale.as_str()) {
            out.push(if fa { format!("گویهٔ {} به زیرمقیاس ناشناختهٔ «{}» اشاره دارد", it.id, it.subscale) } else { format!("Item {} refers to an unknown subscale '{}'", it.id, it.subscale) });
        }
    }
    if i.subscales.is_empty() {
        out.push(locale.pick("At least one subscale is needed: it decides which profile field receives the score", "دست‌کم یک زیرمقیاس لازم است: تعیین می‌کند نمره در کدام فیلد پروفایل ثبت شود").into());
    }
    let mut fields = BTreeSet::new();
    let mut keys = BTreeSet::new();
    for s in &i.subscales {
        if !slug_ok(&s.key, 40) || !keys.insert(s.key.clone()) {
            out.push(if fa { format!("کلید زیرمقیاس «{}» نامعتبر یا تکراری است", s.key) } else { format!("Subscale key '{}' is invalid or duplicated", s.key) });
        }
        if s.label.trim().is_empty() {
            out.push(if fa { format!("زیرمقیاس «{}» به برچسب نیاز دارد", s.key) } else { format!("Subscale '{}' needs a label", s.key) });
        }
        if !slug_ok(&s.field, 64) || !fields.insert(s.field.clone()) {
            out.push(if fa { format!("فیلد «{}» نامعتبر است یا دو بار استفاده شده", s.field) } else { format!("Field '{}' is invalid or used twice", s.field) });
        } else if let Some(def) = registry.get(&s.field) {
            if !matches!(def.kind, FieldKind::Number) {
                out.push(if fa { format!("فیلد «{}» وجود دارد اما عددی نیست", s.field) } else { format!("Field '{}' exists but is not a number field", s.field) });
            }
        }
        if !i.items.iter().any(|it| it.subscale == s.key) {
            out.push(if fa { format!("زیرمقیاس «{}» گویه‌ای ندارد", s.key) } else { format!("Subscale '{}' has no items", s.key) });
        }
    }
    out
}

/// The numeric fields a new instrument needs that do not exist yet. Scores are psychological information, so they start as sensitive.
pub fn missing_fields(i: &Instrument, registry: &FieldRegistry) -> Vec<FieldDef> {
    i.subscales
        .iter()
        .filter(|s| registry.get(&s.field).is_none())
        .map(|s| FieldDef { key: s.field.clone(), label: format!("{} ({})", s.label, i.name), label_fa: None, kind: FieldKind::Number, sensitive: true, required: false })
        .collect()
}

pub fn validate_answer(i: &Instrument, item_id: &str, value: i32) -> Result<(), String> {
    validate_answer_for(i, item_id, value, Locale::En)
}

pub fn validate_answer_for(i: &Instrument, item_id: &str, value: i32, locale: Locale) -> Result<(), String> {
    if !i.items.iter().any(|it| it.id == item_id) {
        return Err(match locale {
            Locale::En => format!("There is no item '{item_id}'"),
            Locale::Fa => format!("گویه‌ای با شناسهٔ «{item_id}» وجود ندارد"),
        });
    }
    if value < i.scale.min || value > i.scale.max {
        return Err(match locale {
            Locale::En => format!("An answer must be between {} and {}", i.scale.min, i.scale.max),
            Locale::Fa => format!(
                "پاسخ باید بین {} و {} باشد",
                crate::locale::localize_digits(locale, &i.scale.min.to_string()),
                crate::locale::localize_digits(locale, &i.scale.max.to_string())
            ),
        });
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

const MINI_IPIP_ITEMS_FA: [&str; 20] = [
    "در مهمانی‌ها مرکز توجه‌ام.",
    "با احساسات دیگران همدردی می‌کنم.",
    "کارهای روزمره را بلافاصله انجام می‌دهم.",
    "حالت روحی‌ام زود‌به‌زود تغییر می‌کند.",
    "تخیل پرتوان و زنده‌ای دارم.",
    "زیاد حرف نمی‌زنم.",
    "به مشکلات دیگران علاقه‌ای ندارم.",
    "اغلب فراموش می‌کنم چیزها را سر جایشان بگذارم.",
    "بیشتر وقت‌ها آرام و آسوده‌ام.",
    "به ایده‌های انتزاعی علاقه‌ای ندارم.",
    "در مهمانی‌ها با آدم‌های زیاد و متفاوتی صحبت می‌کنم.",
    "هیجان‌های دیگران را حس می‌کنم.",
    "نظم را دوست دارم.",
    "زود ناراحت می‌شوم.",
    "درک ایده‌های انتزاعی برایم سخت است.",
    "ترجیح می‌دهم در حاشیه بمانم.",
    "واقعاً به دیگران علاقه‌ای ندارم.",
    "کارها را به هم می‌ریزم.",
    "به‌ندرت احساس دلگرفتگی می‌کنم.",
    "تخیل خوبی ندارم.",
];

impl Instrument {
    /// The built-in Mini-IPIP with its wording in `locale`; any other instrument, or an edited copy, is returned unchanged.
    pub fn localized(mut self, locale: Locale) -> Instrument {
        if locale != Locale::Fa || self.key != "mini_ipip" || self.kind != "standard" || self.version != 1 || self.items.len() != MINI_IPIP_ITEMS_FA.len() {
            return self;
        }
        self.name = "Mini-IPIP (پنج عامل بزرگ، ۲۰ گویه)".into();
        self.description = "هر عبارت تا چه اندازه شما را توصیف می‌کند. بیست عبارت کوتاه؛ حدود سه دقیقه.".into();
        for (item, text) in self.items.iter_mut().zip(MINI_IPIP_ITEMS_FA) {
            item.text = text.into();
        }
        if self.scale.labels.len() == 5 {
            self.scale.labels = ["کاملاً نادرست", "تا حدی نادرست", "نه درست نه نادرست", "تا حدی درست", "کاملاً درست"].iter().map(|s| s.to_string()).collect();
        }
        for subscale in self.subscales.iter_mut() {
            let label = match subscale.key.as_str() {
                "extraversion" => "برون‌گرایی",
                "agreeableness" => "توافق‌پذیری",
                "conscientiousness" => "وظیفه‌شناسی",
                "neuroticism" => "روان‌رنجوری",
                "openness" => "خلاقیت / گشودگی ذهنی",
                _ => continue,
            };
            subscale.label = label.into();
        }
        self.evidence.limitations = "پیش از اتکا به نمره‌ها، متن و جهت‌دهی گویه‌ها را با مقیاس منتشرشده مقایسه کنید: آن‌ها از حافظه بازتولید شده‌اند. \
ویژگی‌های خودگزارشی فقط بخش کوچکی از سرنوشت یک رابطه را توضیح می‌دهند و شباهت نمره‌های دو نفر پیش‌بینی‌کننده رضایت نبوده است. \
چهار گویه برای هر ویژگی تصویری تقریبی می‌دهد، نه اندازه‌گیری دقیق. نمره‌ها گرایش‌های خودگزارشی را توصیف می‌کنند؛ ارزیابی بالینی نیستند و درباره خوب یا بد بودن یک فرد چیزی نمی‌گویند. \
نمره‌ها ممکن است میان فرهنگ‌ها و زبان‌ها فرق کنند و این ترجمه فارسی هنوز اعتباریابی نشده است."
            .into();
        self.evidence.interpretation = "نمره هر ویژگی از ۴ تا ۲۰ است (نقطه میانی ۱۲). نمره بالاتر یعنی فرد خود را برون‌گراتر، توافق‌پذیرتر، وظیفه‌شناس‌تر، از نظر هیجانی ناپایدارتر (روان‌رنجوری) یا خلاق‌تر و اهل فکر بیشتر توصیف می‌کند. \
نمره‌ها را آغازگر گفتگو بدانید. قاعده‌های تطبیقی که از آن‌ها استفاده می‌کنند، به دلایل بالا به‌طور پیش‌فرض خاموش‌اند."
            .into();
        self
    }
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
