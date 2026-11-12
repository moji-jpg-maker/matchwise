use regex::Regex;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundDenial {
    Empty,
    TooLong,
    Contact,
    ProtectedValue,
    Score,
}

fn normalized(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .map(|character| match character {
            '۰'..='۹' => char::from_u32('0' as u32 + character as u32 - '۰' as u32).unwrap(),
            '٠'..='٩' => char::from_u32('0' as u32 + character as u32 - '٠' as u32).unwrap(),
            'ي' | 'ى' => 'ی',
            'ك' => 'ک',
            '！'..='～' => char::from_u32(character as u32 - 0xfee0).unwrap(),
            other => other,
        })
        .collect()
}

fn compact(text: &str) -> String {
    normalized(text)
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

pub fn screen(text: &str, protected_values: &[String]) -> Result<(), OutboundDenial> {
    if text.trim().is_empty() {
        return Err(OutboundDenial::Empty);
    }
    if text.chars().count() > 600 {
        return Err(OutboundDenial::TooLong);
    }
    static FORMAT: OnceLock<Regex> = OnceLock::new();
    let normalized = normalized(text);
    let normalized = FORMAT
        .get_or_init(|| Regex::new(r"\p{Cf}").unwrap())
        .replace_all(&normalized, "");
    static CONTACT: OnceLock<Regex> = OnceLock::new();
    let contact = CONTACT.get_or_init(|| Regex::new(
        r"(?ix) @ | [a-z][a-z0-9+.-]*:// | www\s*\. | [\p{L}\p{N}-]+\s*\.\s*\p{L}{2,}(?:\b|/) | (?:[0-9][\s().+\-]* ){7,}"
    ).unwrap());
    if contact.is_match(&normalized) {
        return Err(OutboundDenial::Contact);
    }
    static SCORE: OnceLock<Regex> = OnceLock::new();
    let score = SCORE.get_or_init(|| Regex::new(
        r"(?i)\b(?:scores?|ratings?|rankings?|ranked|compatibility|percent)\b|امتیاز|نمره|رتبه|درصد|سازگاری|[%٪]|[0-9]\s*/\s*[0-9]"
    ).unwrap());
    if score.is_match(&normalized) {
        return Err(OutboundDenial::Score);
    }
    let compact_text = compact(text);
    if protected_values
        .iter()
        .map(|value| compact(value))
        .any(|value| !value.is_empty() && compact_text.contains(&value))
    {
        return Err(OutboundDenial::ProtectedValue);
    }
    Ok(())
}
