//! What may be shown to the *other* person in an introduction.
//!
//! An introduction card is built only from fields the matchmaker has chosen to share, never from anything else
//! in the profile. A few fields can never appear on a card whatever the setting says: contact details, the full
//! name and the exact date of birth. Contact details are exchanged by the matchmaker, never automatically.

use crate::field::{FieldKind, FieldRegistry};
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
pub fn first_name(profile: &Profile, show_first_name: bool) -> String {
    if show_first_name {
        if let Some(Value::Text(full)) = profile.get("full_name") {
            if let Some(first) = full.split_whitespace().next() {
                return first.chars().take(40).collect();
            }
        }
    }
    "Someone".to_string()
}

/// Human wording for a value: whole numbers without ".0", yes/no, underscores as spaces, lists joined.
pub fn display_value(v: &Value) -> Option<String> {
    match v {
        Value::Bool(b) => Some(if *b { "yes" } else { "no" }.to_string()),
        Value::Num(n) if n.is_finite() => Some(if n.fract() == 0.0 { format!("{}", *n as i64) } else { format!("{n}") }),
        Value::Num(_) => None,
        Value::Text(t) => {
            let t = t.trim();
            if t.is_empty() { None } else { Some(t.replace('_', " ")) }
        }
        Value::List(l) if !l.is_empty() => Some(l.iter().map(|s| s.replace('_', " ")).collect::<Vec<_>>().join(", ")),
        Value::List(_) | Value::Records(_) => None,
    }
}

pub fn build_introduction_card(profile: &Profile, registry: &FieldRegistry, fields: &[String], show_first_name: bool) -> IntroCard {
    let mut lines = vec![];
    for key in fields {
        if NEVER_SHARED.contains(&key.as_str()) {
            continue;
        }
        let Some(def) = registry.get(key) else { continue };
        if matches!(def.kind, FieldKind::Records(_)) {
            continue;
        }
        if let Some(text) = profile.get(key).and_then(display_value) {
            lines.push((def.label.clone(), text));
        }
    }
    IntroCard { title: first_name(profile, show_first_name), lines }
}
