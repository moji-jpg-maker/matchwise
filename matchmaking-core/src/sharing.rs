//! What may be shown to the *other* person in an introduction.
//!
//! An introduction card is built only from fields the matchmaker has chosen to share, never from anything else
//! in the profile. A few fields can never appear on a card whatever the setting says: contact details, the full
//! name and the exact date of birth. Contact details are exchanged by the matchmaker, never automatically.

use crate::field::{FieldKind, FieldRegistry};
use crate::locale::{localize_digits, Locale};
use crate::profile::{Profile, Value};
use serde::Serialize;

/// Starting point for a new organization: practical, non-sensitive facts.
pub const DEFAULT_INTRODUCTION_FIELDS: &[&str] = &[
    "age", "city", "education", "occupation", "marital_status", "has_children", "children_count", "height_cm", "smoking",
];

/// Never shown on a card, even if listed.
pub const NEVER_SHARED: &[&str] = &["phone", "telegram_username", "full_name", "date_of_birth"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IntroCard {
    /// First name only (or a neutral placeholder).
    pub title: String,
    pub lines: Vec<(String, String)>,
}

/// First word of the full name, if the organization shows first names and one is recorded.
pub fn first_name_opt(profile: &Profile, show_first_name: bool) -> Option<String> {
    if show_first_name {
        if let Some(Value::Text(full)) = profile.get("full_name") {
            if let Some(first) = full.split_whitespace().next() {
                return Some(first.chars().take(40).collect());
            }
        }
    }
    None
}

/// The neutral stand-in used when no first name may be shown.
pub fn anonymous_name(locale: Locale) -> &'static str {
    locale.pick("Someone", "یک نفر")
}

pub fn first_name_for(profile: &Profile, show_first_name: bool, locale: Locale) -> String {
    first_name_opt(profile, show_first_name).unwrap_or_else(|| anonymous_name(locale).to_string())
}

/// First word of the full name, or "Someone".
pub fn first_name(profile: &Profile, show_first_name: bool) -> String {
    first_name_for(profile, show_first_name, Locale::En)
}

/// Human wording for a value: whole numbers without ".0", yes/no, underscores as spaces, lists joined.
pub fn display_value(v: &Value) -> Option<String> {
    display_value_for(v, Locale::En)
}

/// [`display_value`] in the reader's language: yes/no, list separators and digits follow the locale.
/// Stored option keys and free text are shown as they are, apart from digits.
pub fn display_value_for(v: &Value, locale: Locale) -> Option<String> {
    let text = match v {
        Value::Bool(b) => Some(if *b { locale.pick("yes", "بله") } else { locale.pick("no", "خیر") }.to_string()),
        Value::Num(n) if n.is_finite() => Some(if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{n}") }),
        Value::Num(_) => None,
        Value::Text(t) => {
            let t = t.trim();
            if t.is_empty() { None } else { Some(t.replace('_', " ")) }
        }
        Value::List(l) if !l.is_empty() => Some(l.iter().map(|s| s.replace('_', " ")).collect::<Vec<_>>().join(locale.pick(", ", "، "))),
        Value::List(_) | Value::Records(_) => None,
    };
    text.map(|t| localize_digits(locale, &t))
}

pub fn build_introduction_card(profile: &Profile, registry: &FieldRegistry, fields: &[String], show_first_name: bool) -> IntroCard {
    build_introduction_card_for(profile, registry, fields, show_first_name, Locale::En)
}

/// [`build_introduction_card`] with labels, values and the placeholder name in the reader's language.
pub fn build_introduction_card_for(profile: &Profile, registry: &FieldRegistry, fields: &[String], show_first_name: bool, locale: Locale) -> IntroCard {
    let mut lines = vec![];
    for key in fields {
        if NEVER_SHARED.contains(&key.as_str()) {
            continue;
        }
        let Some(def) = registry.get(key) else { continue };
        if matches!(def.kind, FieldKind::Records(_)) {
            continue;
        }
        if let Some(text) = profile.get(key).and_then(|v| display_value_for(v, locale)) {
            lines.push((def.label_for(locale).to_string(), text));
        }
    }
    IntroCard { title: first_name_for(profile, show_first_name, locale), lines }
}
