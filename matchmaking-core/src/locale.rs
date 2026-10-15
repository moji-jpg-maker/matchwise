//! Supported UI locales for everything the Rust side renders (Telegram, exports, labels).
//!
//! Stored values stay locale-independent; only display text is localized.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Locale {
    #[default]
    En,
    Fa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

impl Locale {
    pub const ALL: [Locale; 2] = [Locale::En, Locale::Fa];

    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Fa => "fa",
        }
    }

    pub fn direction(self) -> TextDirection {
        match self {
            Locale::En => TextDirection::Ltr,
            Locale::Fa => TextDirection::Rtl,
        }
    }

    pub fn html_dir(self) -> &'static str {
        match self.direction() {
            TextDirection::Ltr => "ltr",
            TextDirection::Rtl => "rtl",
        }
    }

    pub fn english_name(self) -> &'static str {
        match self {
            Locale::En => "English",
            Locale::Fa => "Persian",
        }
    }

    pub fn parse(raw: &str) -> Option<Locale> {
        let lower = raw.trim().to_ascii_lowercase().replace('_', "-");
        match lower.split('-').next().unwrap_or("") {
            "en" => Some(Locale::En),
            "fa" | "per" | "fas" => Some(Locale::Fa),
            _ => None,
        }
    }

    pub fn parse_or_default(raw: &str) -> Locale {
        Locale::parse(raw).unwrap_or_default()
    }

    pub fn pick<'a>(self, en: &'a str, fa: &'a str) -> &'a str {
        match self {
            Locale::En => en,
            Locale::Fa => fa,
        }
    }
}

const PERSIAN_DIGITS: [char; 10] = ['۰', '۱', '۲', '۳', '۴', '۵', '۶', '۷', '۸', '۹'];

pub fn localize_digits(locale: Locale, text: &str) -> String {
    match locale {
        Locale::En => text.to_string(),
        Locale::Fa => text
            .chars()
            .map(|ch| match ch.to_digit(10) {
                Some(digit) if ch.is_ascii_digit() => PERSIAN_DIGITS[digit as usize],
                _ => ch,
            })
            .collect(),
    }
}

/// A message with one string per supported locale.
#[derive(Debug, Clone, Copy)]
pub struct Message {
    pub en: &'static str,
    pub fa: &'static str,
}

impl Message {
    pub const fn new(en: &'static str, fa: &'static str) -> Self {
        Message { en, fa }
    }

    pub fn get(&self, locale: Locale) -> &'static str {
        match locale {
            Locale::En => self.en,
            Locale::Fa => self.fa,
        }
    }
}

/// Replaces `{name}` placeholders. Unknown placeholders are left untouched.
pub fn interpolate(template: &str, params: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (name, value) in params {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_regional_and_alias_tags() {
        assert_eq!(Locale::parse("fa"), Some(Locale::Fa));
        assert_eq!(Locale::parse("fa-IR"), Some(Locale::Fa));
        assert_eq!(Locale::parse("FA_ir"), Some(Locale::Fa));
        assert_eq!(Locale::parse("en-GB"), Some(Locale::En));
        assert_eq!(Locale::parse("de"), None);
        assert_eq!(Locale::parse(""), None);
        assert_eq!(Locale::parse_or_default("xx"), Locale::En);
    }

    #[test]
    fn direction_matches_locale() {
        assert_eq!(Locale::En.html_dir(), "ltr");
        assert_eq!(Locale::Fa.html_dir(), "rtl");
        assert_eq!(Locale::Fa.code(), "fa");
    }

    #[test]
    fn localizes_digits_only_for_persian() {
        assert_eq!(localize_digits(Locale::En, "Room 12"), "Room 12");
        assert_eq!(localize_digits(Locale::Fa, "سن 30 - 1405"), "سن ۳۰ - ۱۴۰۵");
    }

    #[test]
    fn interpolates_known_placeholders() {
        let message = Message::new("Hello {name}", "سلام {name}");
        assert_eq!(interpolate(message.get(Locale::Fa), &[("name", "Sara")]), "سلام Sara");
        assert_eq!(interpolate("{a} {b}", &[("a", "1")]), "1 {b}");
    }

    #[test]
    fn serde_uses_lowercase_codes() {
        assert_eq!(serde_json::to_string(&Locale::Fa).unwrap(), "\"fa\"");
        assert_eq!(serde_json::from_str::<Locale>("\"en\"").unwrap(), Locale::En);
    }
}
