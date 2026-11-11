//! Deterministic field extraction: a fast, local rule tier that runs before any model.
//!
//! * Pure: no I/O, no clock, no randomness. The same text always yields the same suggestions.
//! * Every suggestion quotes an exact substring of the text the matchmaker typed.
//! * Anything that looks like it describes the *desired partner* is ignored, so "looking for a
//!   woman" never becomes the person's own gender.
//! * When the rules disagree about a field, no suggestion is made and the field is reported as
//!   ambiguous, so the caller can hand it to the model tier.

use crate::ai::{FieldSuggestion, SuggestionSource, MAX_VALUE_CHARS};
use crate::field::{FieldDef, FieldKind, FieldRegistry};
use crate::profile::{Profile, Value};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

pub const CONFIDENCE_LABEL: f64 = 0.95;
pub const CONFIDENCE_PATTERN: f64 = 0.85;
pub const CONFIDENCE_KEYWORD: f64 = 0.75;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RuleExtraction {
    pub suggestions: Vec<FieldSuggestion>,
    /// Fields with two different rule values: deliberately left for the model tier.
    pub ambiguous: BTreeSet<String>,
}

/// Contact details the rule tier never fills, whatever the matchmaker asks for. Names are read: the rules run
/// locally, and `full_name` stays out of introductions and model prompts through [`crate::sharing::NEVER_SHARED`].
pub const RULES_SKIPPED: &[&str] = &["phone", "telegram_username", "date_of_birth"];

/// Whether the rule tier may read a field at all.
pub fn rules_may_read(key: &str) -> bool {
    !never_by_rules(key)
}

/// Fields the rule tier never fills.
fn never_by_rules(key: &str) -> bool {
    RULES_SKIPPED.contains(&key)
}

/// Which fields may be read: `None` means every eligible field in the registry.
fn wanted(registry: &FieldRegistry, targets: Option<&[String]>, key: &str) -> bool {
    if never_by_rules(key) {
        return false;
    }
    match registry.get(key) {
        Some(def) if matches!(def.kind, FieldKind::Records(_)) => false,
        None => false,
        _ => match targets {
            None => true,
            Some(list) => list.iter().any(|k| k == key),
        },
    }
}

// ------------------------------------------------------------------ normalisation

/// Lower-case, digit and letter folded copy of the text with the same number of characters, so
/// index `i` in the copy is index `i` in the original and evidence stays an exact substring.
fn fold(text: &str) -> Vec<char> {
    text.chars()
        .map(|c| match c {
            '۰'..='۹' => char::from(b'0' + (c as u32 - '۰' as u32) as u8),
            '٠'..='٩' => char::from(b'0' + (c as u32 - '٠' as u32) as u8),
            'ي' => 'ی',
            'ك' => 'ک',
            '\u{200c}' | '\u{200d}' | '\u{200e}' | '\u{200f}' | '\u{feff}' => '\u{200c}',
            // keep one char per char: lower-case the first scalar of the expansion only
            other => other.to_lowercase().next().unwrap_or(other),
        })
        .collect()
}

fn slice_chars(chars: &[char], start: usize, end: usize) -> String {
    chars[start..end.min(chars.len())].iter().collect::<String>().trim().to_string()
}

fn is_boundary(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '؟' | '؛' | ';' | '\n' | '\r')
}

/// Words that mark text as describing the desired partner rather than the person in the form.
const PARTNER_MARKERS: &[&str] = &[
    "looking for", "searching for", "seeking", "want a", "want an", "want to meet", "wants a", "wants an",
    "would like a", "hoping to meet", "partner should", "partner must", "must be a", "should be a",
    "دنبال", "مایل به", "همسری", "می‌خواهد", "میخواهد", "همسر آینده", "طالب", "انتظار",
];

/// Where a clause that starts at `marker` begins: after the nearest coordinating conjunction or comma, so
/// "41 years old and looking for a woman" keeps the age and only ignores the woman.
fn clause_start(norm: &[char], marker: usize) -> usize {
    const BREAKS: &[&str] = &[" and ", " but ", " or ", ", ", "، ", " و ", " اما ", " یا "];
    let breaks: Vec<Vec<char>> = BREAKS.iter().map(|sep| sep.chars().collect()).collect();
    let mut left = marker;
    while left > 0 && !is_boundary(norm[left - 1]) {
        for sep in &breaks {
            let len = sep.len();
            if left >= len && norm[left - len..left] == sep[..] {
                return left;
            }
        }
        left -= 1;
    }
    left
}

/// Clauses that talk about the desired partner. Matches inside them describe someone else.
fn partner_clauses(norm: &[char]) -> Vec<(usize, usize)> {
    let joined: String = norm.iter().collect();
    let mut out: Vec<(usize, usize)> = vec![];
    for m in PARTNER_MARKERS {
        for (byte_start, _) in joined.match_indices(m) {
            let start = joined[..byte_start].chars().count();
            let len = m.chars().count();
            let left = clause_start(norm, start);
            let mut right = (start + len).min(norm.len());
            while right < norm.len() && !is_boundary(norm[right]) {
                right += 1;
            }
            out.push((left, right));
        }
    }
    out
}


fn guarded(clauses: &[(usize, usize)], start: usize, end: usize) -> bool {
    clauses.iter().any(|(a, b)| start < *b && end > *a)
}

// ------------------------------------------------------------------ patterns

struct Patterns {
    heading: Regex,
    age: Vec<Regex>,
    height_cm: Vec<Regex>,
    height_m: Vec<Regex>,
    weight: Vec<Regex>,
    children_count: Vec<Regex>,
    birth_date: Regex,
    lives_in: Vec<Regex>,
    born_in: Vec<Regex>,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("static rule regex");
        Patterns {
            // A labelled line, with or without a value: a heading whose value is empty owns the lines below it.
            heading: r(r"(?m)^[^\p{L}\p{N}\n]{0,6}(?P<label>[^:：\-\n]{1,60}?)[ \t]*(?P<sep>[:：\-])[ \t]*(?P<value>[^\n]*)$"),
            age: vec![
                r(r"(?P<v>\d{1,3})[ \t]*(?:years?[ \t]*old|y/?o\b)"),
                r(r"(?P<v>\d{1,3})[ \t]*(?:ساله|سال[ \t]*دارد|سال[ \t]*سن)"),
                r(r"(?:^|[^\p{L}])سن[ \t]*(?:است)?[ \t]*[:：]?[ \t]*(?P<v>\d{1,3})"),
                r(r"^(?:age)[ \t]*[:：][ \t]*(?P<v>\d{1,3})"),
            ],
            height_cm: vec![
                r(r"(?P<v>\d{2,3})(?:[.,](\d))?[ \t]*(?:cm|سانت(?:ی‌?متر)?|سانتی‌?متر)"),
                r(r"^(?:height)[ \t]*[:：][ \t]*(?P<v>\d{2,3})"),
            ],
            height_m: vec![r(r"(?P<v>\d(?:[.,]\d{1,2})?)[ \t]*(?:m\b|متر)")],
            weight: vec![
                r(r"(?P<v>\d{2,3})[ \t]*(?:kg|کیلو(?:گرم)?)"),
                r(r"^(?:weight)[ \t]*[:：][ \t]*(?P<v>\d{2,3})"),
            ],
            children_count: vec![
                r(r"(?P<v>\d{1,2})[ \t]*(?:فرزند|بچه|children|kids)"),
                r(r"(?:فرزند|بچه|children|kids)[ \t]*(?:دارد|has|:)?[ \t]*(?P<v>\d{1,2})"),
            ],
            birth_date: r(r"\b(?P<v>\d{4}-\d{2}-\d{2})\b"),
            // These three classes are written unbounded and clipped in code: a `{1,40}` bound over a large
            // Unicode class costs ~12 ms to compile, and compilation happens on the first run of the process.
            lives_in: vec![r(r"(?:lives[ \t]+in|based[ \t]+in|living[ \t]+in)[ \t]+(?P<v>[\p{L}][\p{L} \-]*)"), r(r"(?:ساکن|سکونت[ \t]*در|زندگی[ \t]*در)[ \t]*(?P<v>[\p{L}][\p{L} \-]*)")],
            born_in: vec![r(r"(?:born[ \t]+in|birthplace)[ \t]*[:：]?[ \t]*(?P<v>[\p{L}][\p{L} \-]*)"), r(r"متولد[ \t]*(?P<v>[\p{L}][\p{L} \-]*)")],
        }
    })
}

fn number_words() -> &'static [(&'static str, u32)] {
    &[
        ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6), ("seven", 7), ("eight", 8),
        ("nine", 9), ("ten", 10), ("یک", 1), ("دو", 2), ("سه", 3), ("چهار", 4), ("پنج", 5), ("شش", 6),
        ("هفت", 7), ("هشت", 8), ("نه", 9), ("ده", 10),
    ]
}

const CITIES: &[&str] = &[
    "tehran", "mashhad", "isfahan", "esfahan", "shiraz", "tabriz", "karaj", "qom", "ahvaz", "kermanshah",
    "rasht", "yazd", "kerman", "urmia", "zahedan", "hamadan", "sanandaj", "bandar abbas", "ardabil", "arak",
    "zanjan", "tehran", "تهران", "مشهد", "اصفهان", "شیراز", "تبریز", "کرج", "قم", "اهواز", "کرمانشاه", "رشت",
    "یزد", "کرمان", "ارومیه", "زاهدان", "همدان", "سنندج", "بندرعباس", "اردبیل", "اراک", "زنجان",
];

const PROVINCES: &[&str] = &[
    "tehran", "isfahan", "khuzestan", "fars", "gilan", "mazandaran", "kerman", "kermanshah", "kurdistan",
    "hamadan", "markazi", "qazvin", "zanjan", "yazd", "hormozgan", "lorestan", "ardabil", "bushehr", "golestan",
    "qom", "semnan", "ilam", "alborz", "تهران", "اصفهان", "خوزستان", "فارس", "گیلان", "مازندران", "کرمان",
    "کرمانشاه", "کردستان", "همدان", "مرکزی", "قزوین", "زنجان", "یزد", "هرمزگان", "لرستان", "اردبیل", "بوشهر",
    "گلستان", "قم", "سمنان", "ایلام", "البرز", "خراسان رضوی", "خراسان جنوبی", "خراسان شمالی",
    "آذربایجان شرقی", "آذربایجان غربی", "سیستان و بلوچستان", "چهارمحال و بختیاری",
];

fn pick_city(value: &str) -> Option<&'static str> {
    let v = value.trim().trim_end_matches([',', '،', '.']).to_lowercase();
    CITIES.iter().find(|c| **c == v).copied()
}

fn pick_province(value: &str) -> Option<&'static str> {
    let v = value.trim().trim_end_matches([',', '،', '.']).to_lowercase();
    PROVINCES.iter().find(|c| **c == v).copied()
}

/// A matched span may run long when the pattern leaves the tail open: keep the first words only.
fn clip_words(value: &str, max: usize) -> String {
    value.trim().chars().take(max).collect::<String>().trim_end().to_string()
}

// ------------------------------------------------------------------ candidate collection

struct Candidate {
    field: String,
    value: Value,
    start: usize,
    end: usize,
    confidence: f64,
}

struct Collector {
    clauses: Vec<(usize, usize)>,
    /// The value spans labelled lines own, with the field that owns them. A pattern or keyword inside another
    /// field's value describes that other field, so it is dropped. The map is built while headings are read, so
    /// it does not depend on which fields the caller asked for or on a value converting successfully.
    owned: Vec<(usize, usize, String)>,
    /// The label text of every labelled line, from its first character to just after the colon or dash. A
    /// keyword inside a label names the field the label belongs to, never a value of its own.
    labels: Vec<(usize, usize)>,
    /// Spans that describe something other than this person's fields, such as a partner block or a photo
    /// marker. Nothing is read from inside them.
    ignored: Vec<(usize, usize)>,
    out: Vec<Candidate>,
}

impl Collector {
    fn push(&mut self, field: &str, value: Value, start: usize, end: usize, confidence: f64) {
        if guarded(&self.clauses, start, end) || guarded(&self.ignored, start, end) || guarded(&self.labels, start, end) {
            return;
        }
        self.out.push(Candidate { field: field.to_string(), value, start, end, confidence });
    }

    /// Push a candidate found by a pattern or keyword: dropped when it sits inside another field's labelled value.
    fn push_inferred(&mut self, field: &str, value: Value, start: usize, end: usize, confidence: f64) {
        if self.owned.iter().any(|(a, b, owner)| owner != field && start < *b && end > *a) {
            return;
        }
        self.push(field, value, start, end, confidence);
    }

    /// `text` is `norm` joined once by the caller, so a run builds the searched string a single time.
    fn regex_all(&mut self, text: &str, re: &Regex, field: &str, conf: f64, map: &dyn Fn(&str) -> Option<Value>) {
        for cap in re.captures_iter(text) {
            let Some(m) = cap.name("v") else { continue };
            let start = text[..m.start()].chars().count();
            let end = start + m.as_str().chars().count();
            let raw = m.as_str();
            if let Some(v) = map(raw) {
                self.push_inferred(field, v, start, end, conf);
            }
        }
    }
}

fn digit_value(c: char) -> Option<char> {
    match c {
        '0'..='9' => Some(c),
        '۰'..='۹' => Some(char::from(b'0' + (c as u32 - '۰' as u32) as u8)),
        '٠'..='٩' => Some(char::from(b'0' + (c as u32 - '٠' as u32) as u8)),
        _ => None,
    }
}

/// The first run of digits in `s`, as a number. Persian and Arabic-Indic digits count as digits, a single
/// `.` or `,` between two digits is the decimal separator, and a second run ("2 sisters, 1 brother") is not
/// glued onto the first ("21").
fn first_number(s: &str) -> Option<f64> {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if digit_value(c).is_some() {
            break;
        }
        chars.next();
    }
    while let Some(&c) = chars.peek() {
        if let Some(d) = digit_value(c) {
            out.push(d);
            chars.next();
        } else if matches!(c, '.' | ',') && !out.contains('.') && chars.clone().nth(1).and_then(digit_value).is_some() {
            out.push('.');
            chars.next();
        } else {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        out.parse().ok()
    }
}

/// «۲ خواهر و ۱ برادر» is three siblings, not twenty-one. Every sibling noun immediately preceded by a
/// number adds that number; with no such pair the first number is the count.
fn siblings_total(s: &str) -> Option<f64> {
    const NOUNS: &[&str] = &["خواهر", "برادر", "sister", "brother"];
    let mut total: u32 = 0;
    let mut seen: Vec<usize> = vec![];
    let mut found = false;
    for noun in NOUNS {
        for (b, _) in s.match_indices(noun) {
            let prefix = s[..b].trim_end();
            let trailing = prefix.chars().rev().take_while(|c| digit_value(*c).is_some()).count();
            let digits: String = prefix.chars().skip(prefix.chars().count() - trailing).filter_map(digit_value).collect();
            if digits.is_empty() || trailing > 2 {
                continue;
            }
            let Ok(n) = digits.parse::<u32>() else { continue };
            let byte_start = prefix.char_indices().nth(prefix.chars().count() - trailing).map(|(b, _)| b).unwrap_or(prefix.len());
            if seen.contains(&byte_start) {
                continue;
            }
            seen.push(byte_start);
            total += n;
            found = true;
        }
    }
    if found {
        Some(total as f64)
    } else {
        first_number(s)
    }
}

fn number_in_range(v: f64, lo: f64, hi: f64) -> Option<Value> {
    if v.is_finite() && v >= lo && v <= hi {
        Some(Value::Num(v))
    } else {
        None
    }
}

// ------------------------------------------------------------------ labelled lines

/// Built-in label aliases: the forms matchmakers actually type, mapped to the fields they name. A label may
/// name more than one field: «استان و شهر محل سکونت» fills both the province and the city.
const LABEL_ALIASES: &[(&str, &[&str])] = &[
    // name and contact details
    ("full name", &["full_name"]),
    ("name", &["full_name"]),
    ("first and last name", &["full_name"]),
    ("نام و نام خانوادگی", &["full_name"]),
    ("نام کامل", &["full_name"]),
    ("نام", &["full_name"]),
    ("اسم و فامیل", &["full_name"]),
    ("اسم", &["full_name"]),
    // basics
    ("gender", &["gender"]),
    ("sex", &["gender"]),
    ("جنسیت", &["gender"]),
    ("جنس", &["gender"]),
    ("age", &["age"]),
    ("سن", &["age"]),
    ("سن فعلی", &["age"]),
    ("date of birth", &["date_of_birth"]),
    ("dob", &["date_of_birth"]),
    ("birth date", &["date_of_birth"]),
    ("birthday", &["date_of_birth"]),
    ("تاریخ تولد", &["date_of_birth"]),
    ("تاریخ تولد میلادی", &["date_of_birth"]),
    ("height", &["height_cm"]),
    ("height cm", &["height_cm"]),
    ("قد", &["height_cm"]),
    ("قد به سانتی متر", &["height_cm"]),
    ("weight", &["weight_kg"]),
    ("weight kg", &["weight_kg"]),
    ("وزن", &["weight_kg"]),
    ("وزن به کیلوگرم", &["weight_kg"]),
    // places
    ("birthplace", &["birthplace"]),
    ("place of birth", &["birthplace"]),
    ("city of birth", &["birthplace"]),
    ("محل تولد", &["birthplace"]),
    ("شهر تولد", &["birthplace"]),
    ("city", &["city"]),
    ("current city", &["city"]),
    ("city of residence", &["city"]),
    ("شهر", &["city"]),
    ("شهر محل سکونت", &["city"]),
    ("شهر سکونت", &["city"]),
    ("محل سکونت", &["city"]),
    ("province", &["province"]),
    ("region", &["province"]),
    ("province region", &["province"]),
    ("استان", &["province"]),
    ("منطقه", &["province"]),
    ("استان و شهر محل سکونت", &["province", "city"]),
    ("استان و شهر", &["province", "city"]),
    ("استان شهر محل سکونت", &["province", "city"]),
    ("family origin", &["family_origin"]),
    ("اصالت خانوادگی", &["family_origin"]),
    ("اصالت", &["family_origin"]),
    // education, work and money
    ("education", &["education"]),
    ("education level", &["education"]),
    ("degree", &["education"]),
    ("تحصیلات", &["education"]),
    ("سطح تحصیلات", &["education"]),
    ("مدرک تحصیلی", &["education"]),
    ("occupation", &["occupation"]),
    ("job", &["occupation"]),
    ("profession", &["occupation"]),
    ("شغل", &["occupation"]),
    ("وضعیت و نوع شغل", &["occupation"]),
    ("نوع شغل", &["occupation"]),
    ("employment status", &["employment_status"]),
    ("وضعیت اشتغال", &["employment_status"]),
    ("وضعیت شغلی", &["employment_status"]),
    ("financial situation", &["financial_situation"]),
    ("financial status", &["financial_situation"]),
    ("وضعیت مالی", &["financial_situation"]),
    ("وضعیت اقتصادی", &["financial_situation"]),
    ("توضیحاتی در مورد شرایط مالی", &["financial_situation"]),
    ("شرایط مالی", &["financial_situation"]),
    // faith, marriage and family
    ("religion", &["religion"]),
    ("دین", &["religion"]),
    ("مذهب", &["religion"]),
    ("religiosity", &["religiosity"]),
    ("religious observance", &["religiosity"]),
    ("میزان پایبندی دینی", &["religiosity"]),
    ("پایبندی دینی", &["religiosity"]),
    ("اعتقادات مذهبی", &["religiosity"]),
    ("میزان اعتقادات مذهبی", &["religiosity"]),
    ("marital status", &["marital_status"]),
    ("marriage history", &["marital_status"]),
    ("سابقه ازدواج", &["marital_status"]),
    ("وضعیت تاهل", &["marital_status"]),
    ("وضعیت تإهل", &["marital_status"]),
    ("وضعیت ازدواج", &["marital_status"]),
    ("has children", &["has_children"]),
    ("فرزند دارد", &["has_children"]),
    ("دارای فرزند", &["has_children"]),
    ("number of children", &["children_count"]),
    ("children count", &["children_count"]),
    ("تعداد فرزندان", &["children_count"]),
    ("تعداد فرزند", &["children_count"]),
    ("تعداد بچه", &["children_count"]),
    ("number of siblings", &["siblings_count"]),
    ("siblings count", &["siblings_count"]),
    ("تعداد خواهر و برادر", &["siblings_count"]),
    ("تعداد خواهر برادر", &["siblings_count"]),
    ("father's occupation", &["father_occupation"]),
    ("father occupation", &["father_occupation"]),
    ("شغل پدر", &["father_occupation"]),
    ("کار پدر", &["father_occupation"]),
    ("mother's occupation", &["mother_occupation"]),
    ("mother occupation", &["mother_occupation"]),
    ("شغل مادر", &["mother_occupation"]),
    ("کار مادر", &["mother_occupation"]),
    ("accepts a partner's children", &["accepts_children"]),
    ("فرزندان همسر را می پذیرد", &["accepts_children"]),
    // health and habits
    ("smoking", &["smoking"]),
    ("مصرف دخانیات", &["smoking"]),
    ("استعمال دخانیات", &["smoking"]),
    ("میزان استفاده از سیگار", &["smoking"]),
    ("سیگار", &["smoking"]),
    ("health information", &["health_notes"]),
    ("سلامتی", &["health_notes"]),
    ("وضعیت سلامتی", &["health_notes"]),
    ("اطلاعات سلامت", &["health_notes"]),
    ("lifestyle", &["lifestyle_notes"]),
    ("سبک زندگی", &["lifestyle_notes"]),
    ("about", &["about"]),
    ("about me", &["about"]),
    ("درباره", &["about"]),
    ("درباره من", &["about"]),
    ("توضیحات", &["about"]),
    ("مشخصات روانشناختی", &["about"]),
    ("ویژگی های شخصیتی", &["about"]),
    ("خصوصیات اخلاقی", &["about"]),
];

fn is_decoration(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF
            | 0xFE00..=0xFE0F
            | 0x200D
            | 0x200B
            | 0x2600..=0x27BF
            | 0x2B00..=0x2BFF
            | 0x2190..=0x21FF
            | 0x2300..=0x23FF
            | 0x25A0..=0x25FF
            | 0x2934..=0x2935
            | 0x3030
            | 0x303D
            | 0x3297
            | 0x3299
    ) || matches!(c, '*' | '·' | '~' | '|' | '»' | '«')
}

/// The label as a matchmaker wrote it, reduced to the plain words: no emoji, bullets or numbering, nothing in
/// brackets, no trailing «خودم» / «شما» / «من», ZWNJ as a space and folded Persian letters.
fn clean_label(raw: &str) -> String {
    let mut kept = String::new();
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '(' | '（' | '[' | '【' => depth += 1,
            ')' | '）' | ']' | '】' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            _ if is_decoration(c) => {}
            ',' | '،' | ';' | '؛' => kept.push(' '),
            'ي' => kept.push('ی'),
            'ك' => kept.push('ک'),
            '\u{200c}' | '\u{200e}' | '\u{200f}' | '\u{feff}' => kept.push(' '),
            other => kept.push(other.to_lowercase().next().unwrap_or(other)),
        }
    }
    let mut label: String = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    // leading numbering: "1." "۲)" "(3)" and bullet leftovers such as "- " or ": "
    let without_number = {
        let chars: Vec<char> = label.chars().collect();
        let digits = chars.iter().take_while(|c| digit_value(**c).is_some()).count();
        let after = chars.get(digits).copied();
        if digits > 0 && matches!(after, Some('.' | ')' | '-' | ':' | '、')) {
            chars[digits + 1..].iter().collect::<String>()
        } else {
            label.clone()
        }
    };
    label = without_number
        .trim_matches(|c: char| matches!(c, '-' | '–' | '—' | ':' | '：' | '.' | ')') || c.is_whitespace())
        .to_string();
    for _ in 0..4 {
        let trimmed = label.trim_end();
        let mut cut = None;
        for word in ["خودم", "شما", "من"] {
            if let Some(head) = trimmed.strip_suffix(word) {
                if head.is_empty() || head.ends_with(char::is_whitespace) {
                    cut = Some(head.trim_end().to_string());
                    break;
                }
            }
        }
        match cut {
            Some(rest) => label = rest,
            None => break,
        }
    }
    label.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The cleaned labels a registry accepts, each mapped to the fields it names. Built once per run: the registry
/// is the same for every line, so its labels are cleaned once rather than per line.
fn label_index(registry: &FieldRegistry) -> BTreeMap<String, Vec<String>> {
    let mut index: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let cleaned_aliases = LABEL_ALIASES.iter().map(|(alias, fields)| (clean_label(alias), *fields)).collect::<Vec<_>>();
    for def in registry.defs() {
        let names = [
            clean_label(&def.key),
            clean_label(&def.label),
            def.label_fa.as_deref().map(clean_label).unwrap_or_default(),
            clean_label(&def.key.replace('_', " ")),
        ];
        for name in names.into_iter().filter(|name| !name.is_empty()) {
            let fields = index.entry(name).or_default();
            if !fields.contains(&def.key) {
                fields.push(def.key.clone());
            }
        }
    }
    for (alias, fields) in cleaned_aliases {
        if alias.is_empty() {
            continue;
        }
        let entry = index.entry(alias).or_default();
        for field in fields {
            let field = (*field).to_string();
            if !entry.contains(&field) {
                entry.push(field);
            }
        }
    }
    index
}

fn value_from_label(def: &FieldDef, raw: &str) -> Option<Value> {
    let v = raw.trim();
    if v.is_empty() {
        return None;
    }
    match &def.kind {
        FieldKind::Number => {
            let mut n = if def.key == "siblings_count" { siblings_total(v) } else { first_number(v) }?;
            // «قد: ۱.۷۲ متر» is metres; anything above 3 is already centimetres.
            if def.key == "height_cm" && n > 0.0 && n < 3.0 {
                n *= 100.0;
            }
            number_in_range(n, number_floor(&def.key), number_ceiling(&def.key))
        }
        FieldKind::Bool => match v.to_lowercase().as_str() {
            "yes" | "true" | "y" | "1" | "بله" | "دارد" | "درست" => Some(Value::Bool(true)),
            "no" | "false" | "n" | "0" | "خیر" | "نه" | "ندارد" => Some(Value::Bool(false)),
            _ => None,
        },
        FieldKind::Choice(opts) => {
            if def.key == "education" {
                return highest_education(opts, v).map(Value::Text);
            }
            choice_in_value(opts, v).map(Value::Text)
        }
        FieldKind::MultiChoice(opts) => {
            let mut picked = vec![];
            for part in v.split([',', '،', '/']).map(str::trim).filter(|p| !p.is_empty()) {
                let needle = part.to_lowercase();
                if let Some(o) = opts.iter().find(|o| o.to_lowercase() == needle).cloned() {
                    picked.push(o);
                } else if let Some(o) = synonym(&needle, opts) {
                    picked.push(o);
                }
            }
            if picked.is_empty() { None } else { Some(Value::List(picked)) }
        }
        FieldKind::Text => {
            let clipped: String = v.chars().take(MAX_VALUE_CHARS).collect();
            Some(Value::Text(clipped))
        }
        FieldKind::Records(_) => None,
    }
}

/// The largest number a labelled value may hold. Everything else is a count from zero up.
fn number_ceiling(key: &str) -> f64 {
    match key {
        "age" => 100.0,
        "height_cm" => 230.0,
        "weight_kg" => 250.0,
        _ => 20.0,
    }
}

/// The smallest number a labelled value may hold.
fn number_floor(key: &str) -> f64 {
    match key {
        "age" => 16.0,
        "height_cm" => 120.0,
        "weight_kg" => 35.0,
        _ => 0.0,
    }
}

/// An option written exactly, or the longest synonym found anywhere in the value. Returns the option string.
fn choice_in_value(opts: &[String], value: &str) -> Option<String> {
    let needle = value.trim().to_lowercase();
    if let Some(exact) = opts.iter().find(|o| o.to_lowercase() == needle) {
        return Some(exact.clone());
    }
    // Longest synonym wins so «غیرمذهبی» is not read as «مذهبی».
    let mut best: Option<(usize, String)> = None;
    for name in SYNONYMS {
        if !opts.iter().any(|o| o == name.0) {
            continue;
        }
        for alias in name.1 {
            let alias = alias.to_lowercase();
            if !alias.is_empty() && needle.contains(&alias) && best.as_ref().is_none_or(|(len, _)| alias.chars().count() > *len) {
                best = Some((alias.chars().count(), name.0.to_string()));
            }
        }
    }
    best.map(|(_, option)| option)
}

/// The highest education level named anywhere in the value. A labelled education field never leaves the rules
/// ambiguous: "کارشناسی ارشد" names a master's even though it also contains "کارشناسی".
fn highest_education(opts: &[String], value: &str) -> Option<String> {
    const ORDER: &[&str] = &["secondary", "diploma", "bachelor", "master", "doctorate"];
    let needle = value.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    for (rank, option) in ORDER.iter().enumerate() {
        if !opts.iter().any(|o| o == option) {
            continue;
        }
        let mut named = needle == *option;
        if !named {
            if let Some((_, aliases)) = SYNONYMS.iter().find(|(name, _)| name == option) {
                named = aliases.iter().any(|alias| {
                    let alias = alias.to_lowercase();
                    !alias.is_empty() && needle.contains(&alias)
                });
            }
        }
        if named && best.is_none_or(|(seen, _)| rank > seen) {
            best = Some((rank, option));
        }
    }
    best.map(|(_, option)| option.to_string())
}

/// A small built-in synonym table for the default choice fields. The first entry of each pair is the option
/// string; the rest are the words a matchmaker uses for it, in English and Persian.
const SYNONYMS: &[(&str, &[&str])] = &[
    ("female", &["woman", "girl", "زن", "خانم", "دختر", "زنانه"]),
    ("male", &["man", "boy", "مرد", "آقا", "پسر", "مردانه"]),
    ("never_married", &["single", "never married", "مجرد", "هرگز ازدواج نکرده", "بدون سابقه ازدواج"]),
    ("divorced", &["divorced", "مطلقه", "جدا شده"]),
    ("widowed", &["widow", "widowed", "بیوه", "همسر فوت شده"]),
    ("separated", &["separated", "جدا زندگی می‌کند"]),
    ("secondary", &["secondary", "دبیرستان", "متوسطه", "سیکل"]),
    ("diploma", &["diploma", "دیپلم"]),
    ("bachelor", &["bachelor", "ba", "bsc", "کارشناسی", "لیسانس"]),
    ("master", &["master", "ma", "msc", "کارشناسی ارشد", "فوق لیسانس"]),
    ("doctorate", &["phd", "doctorate", "دکترا", "دکتری"]),
    ("other", &["other", "فوق دیپلم", "کاردانی", "سایر"]),
    ("employed", &["employed", "full time", "شاغل", "تمام وقت"]),
    ("self_employed", &["self employed", "freelance", "خودشتغل", "آزاد"]),
    ("student", &["student", "دانشجو", "محصل"]),
    ("unemployed", &["unemployed", "بیکار"]),
    ("retired", &["retired", "بازنشسته"]),
    ("never", &["never", "no", "هیچ", "هرگز", "اصلا", "نمی‌کشد", "نمی‌کشم", "سیگاری نیست", "سیگار نمی‌کشد"]),
    ("occasionally", &["occasionally", "sometimes", "گاهی", "به ندرت", "کمی"]),
    ("regularly", &["regularly", "daily", "regular", "روزانه", "همیشه", "مستمر", "سیگاری"]),
    ("low", &["low", "کم", "ضعیف", "غیرمذهبی", "بدون اعتقادات"]),
    ("moderate", &["moderate", "متوسط", "معمولی", "تا حدی"]),
    ("high", &["high", "زیاد", "خیلی", "قوی", "مذهبی", "پایبند"]),
];

/// Kept for callers that only have a bare value rather than a labelled one.
fn synonym(needle: &str, opts: &[String]) -> Option<String> {
    for (key, names) in SYNONYMS {
        if opts.iter().any(|o| o == key) && names.iter().any(|name| name.to_lowercase() == needle) {
            return Some((*key).to_string());
        }
    }
    None
}

/// A labelled heading and the fields its cleaned label names. `line_end` is the end of the heading's own line,
/// so a value written below it can be told apart from the label.
struct Heading {
    start: usize,
    /// Just after the colon or dash that separates the label from its value.
    value_start: usize,
    line_end: usize,
    label: String,
    fields: Vec<String>,
}

/// Every labelled line in the text, in document order. A heading is recognised by cleanable label shape alone;
/// whether it names a field is decided later.
fn headings(chars: &[char], index: &BTreeMap<String, Vec<String>>) -> Vec<Heading> {
    let text: String = chars.iter().collect();
    let mut out = vec![];
    for cap in patterns().heading.captures_iter(&text) {
        let (Some(label), Some(sep)) = (cap.name("label"), cap.name("sep")) else { continue };
        let start = text[..cap.get(0).map_or(label.start(), |m| m.start())].chars().count();
        let value_start = text[..sep.end()].chars().count();
        let line_end = start + cap.get(0).map_or(0, |m| m.as_str().chars().count());
        let label = label.as_str().to_string();
        let fields = index.get(&clean_label(&label)).cloned().unwrap_or_default();
        out.push(Heading { start, value_start, line_end, label, fields });
    }
    out
}

/// The heading a value may continue below: a later labelled line ends it. A value never crosses a blank line or
/// a line that is not part of the text.
fn next_heading_line(headings: &[Heading], i: usize, chars_len: usize) -> usize {
    headings.get(i + 1).map_or(chars_len, |h| h.start)
}

/// The value a heading owns. A value on the label's own line is taken as typed. When nothing follows the colon,
/// the value is the lines below it up to the next labelled line, capped at [`MAX_VALUE_CHARS`].
struct HeadingValue {
    text: String,
    start: usize,
    end: usize,
}

fn heading_value(chars: &[char], original: &[char], headings: &[Heading], i: usize, ignored: &[(usize, usize)]) -> Option<HeadingValue> {
    let heading = &headings[i];
    // Everything after the label on its own line. A Colon-less dash line still leaves the text after the dash.
    let rest_start = skip_whitespace(original, heading.value_start, heading.line_end);
    let inline = trim_span(original, rest_start, heading.line_end);
    if inline.1 > inline.0 {
        let text: String = original[inline.0..inline.1].iter().collect();
        return Some(HeadingValue { text, start: inline.0, end: inline.1 });
    }
    // Nothing after the colon: the value is the block below the heading. An ignored line is never part of it,
    // neither as a leading instruction nor in the middle, so the evidence stays one contiguous span.
    let block_end = next_heading_line(headings, i, chars.len());
    let lines: Vec<(usize, usize)> = line_spans(&chars[..block_end])
        .into_iter()
        .filter(|(start, _)| *start >= heading.line_end)
        .map(|(start, end)| trim_span(original, start, end))
        .filter(|(start, end)| end > start)
        .collect();
    let kept: Vec<(usize, usize)> = lines.iter().copied().filter(|(start, end)| !is_ignored(ignored, *start, *end)).collect();
    let (first, rest) = kept.split_first()?;
    let mut value_end = first.1;
    for (start, end) in rest {
        if lines.iter().any(|line| line.0 > value_end && line.1 <= *start && is_ignored(ignored, line.0, line.1)) {
            break;
        }
        value_end = *end;
    }
    let block: String = original[first.0..value_end].iter().collect();
    let clipped = block.trim().trim_end();
    if clipped.is_empty() {
        return None;
    }
    let text: String = clipped.chars().take(MAX_VALUE_CHARS).collect();
    let text = text.trim_end().to_string();
    Some(HeadingValue { start: first.0, end: first.0 + text.chars().count(), text })
}

/// Spans that describe something other than this person's fields: text fully wrapped in brackets, a photo
/// marker line, and a partner block. Nothing inside them becomes a suggestion.
fn ignored_ranges(chars: &[char], headings: &[Heading]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    for (start, end) in line_spans(chars) {
        let line = &chars[start..end];
        if bracketed_text(line) {
            out.push((start, end));
        }
        if photo_marker(line) {
            out.push((start, end));
        }
    }
    for (i, heading) in headings.iter().enumerate() {
        if !partner_heading(&heading.label) {
            continue;
        }
        out.push((heading.start, next_heading_line(headings, i, chars.len())));
    }
    out
}

/// Every line as a `(start, end)` span without its trailing newline.
fn line_spans(chars: &[char]) -> Vec<(usize, usize)> {
    let mut out = vec![];
    let mut start = 0;
    for (i, c) in chars.iter().enumerate() {
        if *c == '\n' {
            out.push((start, i));
            start = i + 1;
        }
    }
    if start < chars.len() {
        out.push((start, chars.len()));
    }
    out
}

fn trim_span(chars: &[char], start: usize, end: usize) -> (usize, usize) {
    let (mut a, mut b) = (start.min(chars.len()), end.min(chars.len()));
    while a < b && chars[a].is_whitespace() {
        a += 1;
    }
    while b > a && chars[b - 1].is_whitespace() {
        b -= 1;
    }
    (a, b)
}

fn is_ignored(ignored: &[(usize, usize)], start: usize, end: usize) -> bool {
    ignored.iter().any(|(a, b)| start < *b && end > *a)
}

fn skip_whitespace(chars: &[char], start: usize, end: usize) -> usize {
    let mut i = start.min(end);
    while i < end && (chars[i].is_whitespace() || chars[i] == '\u{feff}') {
        i += 1;
    }
    i
}

/// A whole line inside one pair of brackets: an instruction such as «( لطفا توضیح دهید )», never a value.
fn bracketed_text(line: &[char]) -> bool {
    const PAIRS: &[(char, char)] = &[('(', ')'), ('（', '）'), ('[', ']'), ('【', '】')];
    let (start, end) = trim_span(line, 0, line.len());
    if end <= start {
        return false;
    }
    let (Some(first), Some(last)) = (line.get(start).copied(), line.get(end - 1).copied()) else {
        return false;
    };
    PAIRS.iter().any(|(open, close)| *open == first && *close == last)
}

/// A line that starts with a word naming a photo, such as «🌷 عکس من» or «photo 2»: the picture is stored by the
/// photo tier, not read as text.
fn photo_marker(line: &[char]) -> bool {
    const WORDS: &[&str] = &[
        "عکس", "عکسها", "عکس\u{200c}ها", "تصویر", "تصاویر", "photo", "photos", "pic", "pics", "picture", "pictures",
    ];
    let text: String = line.iter().filter(|c| !is_decoration(**c)).collect();
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric() && c != '\u{200c}').to_lowercase())
        .find(|word| !word.is_empty() && !word.chars().all(|c| digit_value(c).is_some()))
        .is_some_and(|word| WORDS.contains(&word.as_str()))
}

/// A heading that starts a block about the wanted partner rather than the person whose form this is.
fn partner_heading(label: &str) -> bool {
    let text = label.to_lowercase();
    PARTNER_MARKERS.iter().any(|marker| text.contains(marker))
}

/// Labels are matched on the folded text, values are taken as typed: a name keeps its capital letters and the
/// evidence stays an exact substring of the original.
fn labelled(registry: &FieldRegistry, index: &BTreeMap<String, Vec<String>>, folded: &str, original: &[char], c: &mut Collector, targets: Option<&[String]>) {
    let chars: Vec<char> = folded.chars().collect();
    let headings = headings(&chars, index);
    c.ignored.extend(ignored_ranges(&chars, &headings));
    for (i, heading) in headings.iter().enumerate() {
        c.labels.push((heading.start, heading.value_start));
        let Some(value) = heading_value(&chars, original, &headings, i, &c.ignored) else { continue };
        // The heading owns its value even when no suggestion comes out of it: the text still describes that
        // field, so a keyword or pattern inside it must not be read as some other field.
        for key in &heading.fields {
            c.owned.push((value.start, value.end, key.clone()));
        }
        // One value may name two fields (a place that is a city and a province): emit each pair once.
        let mut emitted: Vec<(String, Value)> = vec![];
        for key in &heading.fields {
            if !wanted(registry, targets, key) {
                continue;
            }
            let Some(def) = registry.get(key) else { continue };
            let Some(v) = value_from_label(def, &value.text) else { continue };
            for (field, v) in label_values(registry, key, v, &value.text) {
                if !wanted(registry, targets, &field) || registry.validate_value(&field, &v).is_err() {
                    continue;
                }
                if emitted.contains(&(field.clone(), v.clone())) {
                    continue;
                }
                emitted.push((field.clone(), v.clone()));
                c.push(&field, v, value.start, value.end, CONFIDENCE_LABEL);
            }
        }
    }
}

/// The suggestions one labelled value makes: the field the label names, plus the ones the value implies. A
/// children count says whether there are children. A place that is both a city and a province fills both, and
/// «X، Y» is read as province, then city.
fn label_values(registry: &FieldRegistry, key: &str, value: Value, raw: &str) -> Vec<(String, Value)> {
    match key {
        "children_count" => {
            let Value::Num(n) = value else { return vec![] };
            let mut out = vec![(key.to_string(), Value::Num(n))];
            if registry.get("has_children").is_some() {
                out.push(("has_children".to_string(), Value::Bool(n > 0.0)));
            }
            out
        }
        "city" => place_labels(raw)
            .into_iter()
            .map(|(field, text)| (field.to_string(), Value::Text(text)))
            .collect(),
        "province" => {
            let first = raw.split([',', '،', '/']).next().map(str::trim).unwrap_or(raw);
            let text = pick_province(first).map(str::to_string).unwrap_or_else(|| first.to_string());
            vec![(key.to_string(), Value::Text(text))]
        }
        _ => vec![(key.to_string(), value)],
    }
}

/// The city and province a place value names. A known name is stored under its built-in spelling, so two
/// entries that write "Tehran" and "tehran" compare equal.
fn place_labels(raw: &str) -> Vec<(&'static str, String)> {
    let parts: Vec<&str> = raw.split([',', '،', '/']).map(str::trim).filter(|p| !p.is_empty()).collect();
    if let [first, second, ..] = parts.as_slice() {
        if let (Some(province), Some(city)) = (pick_province(first), pick_city(second)) {
            return vec![("province", province.to_string()), ("city", city.to_string())];
        }
    }
    let whole = parts.first().copied().unwrap_or(raw).trim();
    let mut out = vec![];
    if let Some(city) = pick_city(whole) {
        out.push(("city", city.to_string()));
    }
    if let Some(province) = pick_province(whole) {
        out.push(("province", province.to_string()));
    }
    if out.is_empty() {
        out.push(("city", whole.to_string()));
    }
    out
}

// ------------------------------------------------------------------ keyword patterns

fn keywords(c: &mut Collector, text: &str, registry: &FieldRegistry, targets: Option<&[String]>) {
    let idx = |byte: usize| text[..byte].chars().count();
    let mut scan = |needle: &str, field: &str, value: Value, conf: f64| {
        if !wanted(registry, targets, field) {
            return;
        }
        for (b, _) in text.match_indices(needle) {
            if !word_bounded(&text, b, b + needle.len()) {
                continue;
            }
            let start = idx(b);
            c.push_inferred(field, value.clone(), start, start + needle.chars().count(), conf);
        }
    };

    // gender
    scan("female", "gender", Value::Text("female".into()), CONFIDENCE_KEYWORD);
    scan("woman", "gender", Value::Text("female".into()), CONFIDENCE_KEYWORD);
    scan("خانم", "gender", Value::Text("female".into()), CONFIDENCE_KEYWORD);
    scan("زن", "gender", Value::Text("female".into()), CONFIDENCE_KEYWORD);
    scan("male", "gender", Value::Text("male".into()), CONFIDENCE_KEYWORD);
    scan("man", "gender", Value::Text("male".into()), CONFIDENCE_KEYWORD);
    scan("مرد", "gender", Value::Text("male".into()), CONFIDENCE_KEYWORD);
    scan("آقا", "gender", Value::Text("male".into()), CONFIDENCE_KEYWORD);

    // marital history
    scan("never married", "marital_status", Value::Text("never_married".into()), CONFIDENCE_KEYWORD);
    scan("مجرد", "marital_status", Value::Text("never_married".into()), CONFIDENCE_KEYWORD);
    scan("divorced", "marital_status", Value::Text("divorced".into()), CONFIDENCE_KEYWORD);
    scan("مطلقه", "marital_status", Value::Text("divorced".into()), CONFIDENCE_KEYWORD);
    scan("widowed", "marital_status", Value::Text("widowed".into()), CONFIDENCE_KEYWORD);
    scan("بیوه", "marital_status", Value::Text("widowed".into()), CONFIDENCE_KEYWORD);
    scan("separated", "marital_status", Value::Text("separated".into()), CONFIDENCE_KEYWORD);

    // children
    scan("has children", "has_children", Value::Bool(true), CONFIDENCE_KEYWORD);
    scan("no children", "has_children", Value::Bool(false), CONFIDENCE_KEYWORD);
    scan("بدون فرزند", "has_children", Value::Bool(false), CONFIDENCE_KEYWORD);
    scan("بدون بچه", "has_children", Value::Bool(false), CONFIDENCE_KEYWORD);
    scan("فرزند ندارد", "has_children", Value::Bool(false), CONFIDENCE_KEYWORD);
    scan("بچه ندارد", "has_children", Value::Bool(false), CONFIDENCE_KEYWORD);
    scan("فرزند دارد", "has_children", Value::Bool(true), CONFIDENCE_KEYWORD);
    scan("بچه دارد", "has_children", Value::Bool(true), CONFIDENCE_KEYWORD);

    // education, longest phrase first
    scan("کارشناسی ارشد", "education", Value::Text("master".into()), CONFIDENCE_KEYWORD);
    scan("فوق لیسانس", "education", Value::Text("master".into()), CONFIDENCE_KEYWORD);
    scan("master", "education", Value::Text("master".into()), CONFIDENCE_KEYWORD);
    scan("دکترا", "education", Value::Text("doctorate".into()), CONFIDENCE_KEYWORD);
    scan("دکتری", "education", Value::Text("doctorate".into()), CONFIDENCE_KEYWORD);
    scan("phd", "education", Value::Text("doctorate".into()), CONFIDENCE_KEYWORD);
    scan("کارشناسی", "education", Value::Text("bachelor".into()), CONFIDENCE_KEYWORD);
    scan("لیسانس", "education", Value::Text("bachelor".into()), CONFIDENCE_KEYWORD);
    scan("bachelor", "education", Value::Text("bachelor".into()), CONFIDENCE_KEYWORD);
    scan("فوق دیپلم", "education", Value::Text("other".into()), CONFIDENCE_KEYWORD);
    scan("کاردانی", "education", Value::Text("other".into()), CONFIDENCE_KEYWORD);
    scan("دیپلم", "education", Value::Text("diploma".into()), CONFIDENCE_KEYWORD);
    scan("diploma", "education", Value::Text("diploma".into()), CONFIDENCE_KEYWORD);

    // employment
    scan("self employed", "employment_status", Value::Text("self_employed".into()), CONFIDENCE_KEYWORD);
    scan("خودشتغل", "employment_status", Value::Text("self_employed".into()), CONFIDENCE_KEYWORD);
    scan("شاغل", "employment_status", Value::Text("employed".into()), CONFIDENCE_KEYWORD);
    scan("employed", "employment_status", Value::Text("employed".into()), CONFIDENCE_KEYWORD);
    scan("student", "employment_status", Value::Text("student".into()), CONFIDENCE_KEYWORD);
    scan("دانشجو", "employment_status", Value::Text("student".into()), CONFIDENCE_KEYWORD);
    scan("unemployed", "employment_status", Value::Text("unemployed".into()), CONFIDENCE_KEYWORD);
    scan("بیکار", "employment_status", Value::Text("unemployed".into()), CONFIDENCE_KEYWORD);
    scan("retired", "employment_status", Value::Text("retired".into()), CONFIDENCE_KEYWORD);
    scan("بازنشسته", "employment_status", Value::Text("retired".into()), CONFIDENCE_KEYWORD);

    // smoking: explicit phrases only
    scan("سیگار نمی‌کشد", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("سیگاری نیست", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("اهل سیگار نیست", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("non smoker", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("non-smoker", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("does not smoke", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("do not smoke", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("never smoked", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("never smokes", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("never smoke", "smoking", Value::Text("never".into()), CONFIDENCE_KEYWORD);
    scan("سیگار می‌کشد", "smoking", Value::Text("regularly".into()), CONFIDENCE_KEYWORD);
    scan("سیگاری", "smoking", Value::Text("regularly".into()), CONFIDENCE_KEYWORD);
    scan("smoker", "smoking", Value::Text("regularly".into()), CONFIDENCE_KEYWORD);
    scan("گاهی سیگار", "smoking", Value::Text("occasionally".into()), CONFIDENCE_KEYWORD);

    // religious observance: explicit phrases only
    scan("غیرمذهبی", "religiosity", Value::Text("low".into()), CONFIDENCE_KEYWORD);
    scan("کم‌مذهب", "religiosity", Value::Text("low".into()), CONFIDENCE_KEYWORD);
    scan("not religious", "religiosity", Value::Text("low".into()), CONFIDENCE_KEYWORD);
    scan("مذهبی متوسط", "religiosity", Value::Text("moderate".into()), CONFIDENCE_KEYWORD);
    scan("معمولی مذهبی", "religiosity", Value::Text("moderate".into()), CONFIDENCE_KEYWORD);
    scan("moderately religious", "religiosity", Value::Text("moderate".into()), CONFIDENCE_KEYWORD);
    scan("خیلی مذهبی", "religiosity", Value::Text("high".into()), CONFIDENCE_KEYWORD);
    scan("مذهبی", "religiosity", Value::Text("high".into()), CONFIDENCE_KEYWORD);
    scan("very religious", "religiosity", Value::Text("high".into()), CONFIDENCE_KEYWORD);
}

/// A keyword may not sit inside a longer word: "man" is not found in "woman".
fn word_bounded(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    let joins = |c: Option<char>| c.map(|ch| ch.is_alphanumeric() || ch == '\u{200c}').unwrap_or(false);
    !joins(before) && !joins(after)
}

/// Whether any of `needles` occurs in `text`. Used to skip a sweep over a long word list when the text
/// cannot possibly contain a match.
fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn patterns_scan(c: &mut Collector, text: &str, registry: &FieldRegistry, targets: Option<&[String]>) {
    let p = patterns();
    let take = |c: &mut Collector, field: &str, re: &Regex, conf: f64, map: &dyn Fn(&str) -> Option<Value>| {
        if wanted(registry, targets, field) {
            c.regex_all(text, re, field, conf, map);
        }
    };

    for re in &p.age {
        take(c, "age", re, CONFIDENCE_PATTERN, &|raw: &str| number_in_range(first_number(raw)?, 16.0, 100.0));
    }
    for re in &p.height_cm {
        take(c, "height_cm", re, CONFIDENCE_PATTERN, &|raw: &str| number_in_range(first_number(raw)?, 120.0, 230.0));
    }
    for re in &p.height_m {
        take(c, "height_cm", re, CONFIDENCE_PATTERN, &|raw: &str| number_in_range(first_number(raw)? * 100.0, 120.0, 230.0));
    }
    for re in &p.weight {
        take(c, "weight_kg", re, CONFIDENCE_PATTERN, &|raw: &str| number_in_range(first_number(raw)?, 35.0, 250.0));
    }
    for re in &p.children_count {
        take(c, "children_count", re, CONFIDENCE_PATTERN, &|raw: &str| number_in_range(first_number(raw)?, 0.0, 20.0));
    }
    // number words for children
    if wanted(registry, targets, "children_count") {
        // A word pattern needs one of these nouns somewhere in the text; skipping the sweep when none is
        // present keeps the cost of a long entry proportional to what it actually mentions.
        if ["children", "kids", "فرزند", "بچه"].iter().any(|noun| text.contains(noun)) {
            for (word, n) in number_words() {
                for pat in [format!("{word} children"), format!("{word} kids"), format!("{word} فرزند"), format!("{word} بچه")] {
                    for (b, _) in text.match_indices(&pat) {
                        let start = text[..b].chars().count();
                        c.push_inferred("children_count", Value::Num(*n as f64), start, start + pat.chars().count(), CONFIDENCE_PATTERN);
                    }
                }
            }
        }
    }
    if wanted(registry, targets, "date_of_birth") {
        // Gregorian ISO only; a Jalali date is left for the model tier.
        c.regex_all(text, &p.birth_date, "date_of_birth", CONFIDENCE_PATTERN, &|raw: &str| {
            let year: i32 = raw.get(0..4)?.parse().ok()?;
            if (1900..=2026).contains(&year) { Some(Value::Text(raw.to_string())) } else { None }
        });
    }
    if wanted(registry, targets, "city") {
        for re in &p.lives_in {
            c.regex_all(text, re, "city", CONFIDENCE_PATTERN, &|raw: &str| {
                let raw = clip_words(raw, 40);
                let raw = raw.as_str();
                pick_city(raw).or_else(|| {
                    let first = raw.split_whitespace().next()?;
                    pick_city(first)
                })
                .map(|name| Value::Text(name.to_string()))
            });
        }
    }
    if wanted(registry, targets, "birthplace") {
        for re in &p.born_in {
            c.regex_all(text, re, "birthplace", CONFIDENCE_PATTERN, &|raw: &str| Some(Value::Text(clip_words(raw, 40))));
        }
    }
    // a bare city name next to "city" / a known city mentioned alone
    if wanted(registry, targets, "city") && has_any(&text, &["city: ", "شهر: ", "شهر "]) {
        for city in CITIES {
            if city.chars().count() < 4 {
                continue;
            }
            for pat in [format!("city: {city}"), format!("شهر: {city}"), format!("شهر {city}")] {
                for (b, _) in text.match_indices(&pat) {
                    let start = text[..b].chars().count() + pat.find(city).unwrap_or(0) + 1;
                    c.push_inferred("city", Value::Text((*city).to_string()), start, start + city.chars().count(), CONFIDENCE_PATTERN);
                }
            }
        }
    }
    if wanted(registry, targets, "province") && has_any(&text, &["province: ", "استان: ", "استان "]) {
        for prov in PROVINCES {
            if prov.chars().count() < 4 {
                continue;
            }
            for pat in [format!("province: {prov}"), format!("استان: {prov}"), format!("استان {prov}")] {
                for (b, _) in text.match_indices(&pat) {
                    let offset = pat.find(prov).unwrap_or(0);
                    let start = text[..b].chars().count() + pat[..offset].chars().count();
                    c.push_inferred("province", Value::Text((*prov).to_string()), start, start + prov.chars().count(), CONFIDENCE_PATTERN);
                }
            }
        }
    }
}

// ------------------------------------------------------------------ entry point

/// Read `text` with the local rules. `targets` limits which fields are considered (`None` = every
/// eligible field); `profile` supplies the fields that are already filled (nothing is suggested twice).
pub fn extract_with_rules(registry: &FieldRegistry, text: &str, targets: Option<&[String]>, profile: &Profile) -> RuleExtraction {
    // Compiling the static regexes costs far more than reading a 20 000-character entry. Build them once, on the
    // first run, so the first entry a matchmaker pastes is not the one that pays for it.
    patterns();
    let norm_chars = fold(text);
    let original: Vec<char> = text.chars().collect();
    // Joined once and shared by every scan: a run never rebuilds the searched string.
    let norm_text: String = norm_chars.iter().collect();
    let mut c = Collector { clauses: partner_clauses(&norm_chars), owned: vec![], labels: vec![], ignored: vec![], out: vec![] };
    labelled(registry, &label_index(registry), &norm_text, &original, &mut c, targets);
    keywords(&mut c, &norm_text, registry, targets);
    patterns_scan(&mut c, &norm_text, registry, targets);

    let mut by_field: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for cand in c.out {
        if profile.get(&cand.field).is_some() {
            continue; // already stored: the rules only fill gaps
        }
        by_field.entry(cand.field.clone()).or_default().push(cand);
    }
    for cands in by_field.values_mut() {
        // Longest match wins: a keyword inside a longer phrase of the same field is not a rival.
        let keep: Vec<bool> = cands
            .iter()
            .map(|a| {
                !cands.iter().any(|b| {
                    b.confidence >= a.confidence && (b.end - b.start) > (a.end - a.start) && b.start <= a.start && b.end >= a.end
                })
            })
            .collect();
        let mut flags = keep.into_iter();
        cands.retain(|_| flags.next().unwrap_or(true));
    }

    let mut out = RuleExtraction::default();
    for (field, mut cands) in by_field {
        // labelled > pattern > keyword; then the first occurrence for stability
        cands.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal).then(a.start.cmp(&b.start)));
        // Education names more than one level at once ("کارشناسی ارشد" contains "کارشناسی"): the highest level
        // is the person's, so the field is read rather than left for the model.
        if field == "education" {
            if let Some(best) = cands.iter().max_by_key(|c| education_rank(&c.value)) {
                let (value, start, end) = (best.value.clone(), best.start, best.end);
                let confidence = best.confidence;
                push_suggestion(&mut out, &original, field, value, start, end, confidence);
                if out.suggestions.len() >= 40 {
                    break;
                }
                continue;
            }
        }
        let best = &cands[0];
        if cands.iter().any(|c| c.value != best.value) {
            out.ambiguous.insert(field);
            continue;
        }
        push_suggestion(&mut out, &original, field, best.value.clone(), best.start, best.end, best.confidence);
        if out.suggestions.len() >= 40 {
            break;
        }
    }
    out
}

fn push_suggestion(out: &mut RuleExtraction, original: &[char], field: String, value: Value, start: usize, end: usize, confidence: f64) {
    let evidence = slice_chars(original, start, end);
    if evidence.is_empty() {
        return;
    }
    out.suggestions.push(FieldSuggestion { field, value, evidence, confidence, conflict: false, source: SuggestionSource::Rule });
}

/// How far up the education ladder a value sits; anything unrecognised ranks lowest.
fn education_rank(value: &Value) -> usize {
    const ORDER: &[&str] = &["secondary", "diploma", "bachelor", "master", "doctorate"];
    match value {
        Value::Text(text) => ORDER.iter().position(|level| *level == text).map_or(0, |i| i + 1),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{clean_label, extract_with_rules, first_number, siblings_total};
    use crate::defaults::default_registry;
    use crate::profile::Profile;
    use std::collections::BTreeMap;

    fn fields(text: &str) -> BTreeMap<String, crate::profile::Value> {
        extract_with_rules(&default_registry(), text, None, &Profile::new("x"))
            .suggestions
            .into_iter()
            .map(|s| (s.field, s.value))
            .collect()
    }

    #[test]
    fn first_number_reads_only_the_first_run_of_digits() {
        assert_eq!(first_number("28"), Some(28.0));
        assert_eq!(first_number("۲۸ ساله"), Some(28.0));
        assert_eq!(first_number("٢٨"), Some(28.0));
        assert_eq!(first_number("he is 31, she is 29"), Some(31.0));
        assert_eq!(first_number("Dob 1368/1/1"), Some(1368.0));
        assert_eq!(first_number("1.80 m"), Some(1.80));
        assert_eq!(first_number("1,5"), Some(1.5));
        assert_eq!(first_number("no digits"), None);
    }

    #[test]
    fn siblings_total_adds_each_sister_and_brother_count() {
        assert_eq!(siblings_total("۲ خواهر و ۱ برادر"), Some(3.0));
        assert_eq!(siblings_total("2 sisters and 1 brother"), Some(3.0));
        assert_eq!(siblings_total("۶"), Some(6.0));
        assert_eq!(siblings_total("4"), Some(4.0));
        assert_eq!(siblings_total("none"), None);
    }

    #[test]
    fn label_cleaning_removes_noise_but_keeps_the_words() {
        assert_eq!(clean_label("🌷نام و نام خانوادگی :"), "نام و نام خانوادگی");
        assert_eq!(clean_label("🌷سن"), "سن");
        assert_eq!(clean_label("🌷 عکس من 👇‍‍‍‍‍‍"), "عکس");
        assert_eq!(clean_label("١. سطح تحصیلات"), "سطح تحصیلات");
        assert_eq!(clean_label("۲) شغل"), "شغل");
        assert_eq!(clean_label("- تحصیلات -"), "تحصیلات");
        assert_eq!(clean_label("تحصیلات (کامل)"), "تحصیلات");
        assert_eq!(clean_label("( لطفا توضیح دهید )"), "");
        assert_eq!(clean_label("شهر محل سکونت خودم"), "شهر محل سکونت");
        assert_eq!(clean_label("نام شما"), "نام");
        assert_eq!(clean_label("توضیحات من"), "توضیحات");
        assert_eq!(clean_label("نام\u{200c}و\u{200c}نام\u{200c}خانوادگی"), "نام و نام خانوادگی");
        assert_eq!(clean_label("ميزان پايبندي ديني"), "میزان پایبندی دینی");
        assert_eq!(clean_label("  وزن   به   کیلوگرم  "), "وزن به کیلوگرم");
        assert_eq!(clean_label("Age:"), "age");
    }

    #[test]
    fn aliases_resolve_noisy_labels_and_the_combined_location_label() {
        let reg = default_registry();
        let run = |text: &str| {
            extract_with_rules(&reg, text, None, &Profile::new("x"))
                .suggestions
                .into_iter()
                .map(|s| (s.field, s.value))
                .collect::<BTreeMap<_, _>>()
        };
        let got = run("🌷نام و نام خانوادگی : مهسا قویمی\n🌷شغل پدر: بازنشسته");
        assert_eq!(got["full_name"], crate::profile::Value::Text("مهسا قویمی".into()));
        assert_eq!(got["father_occupation"], crate::profile::Value::Text("بازنشسته".into()));

        let got = run("🌷استان و شهر محل سکونت خودم : تهران");
        assert_eq!(got["province"], crate::profile::Value::Text("تهران".into()));
        assert_eq!(got["city"], crate::profile::Value::Text("تهران".into()));
    }

    #[test]
    fn a_heading_with_nothing_after_the_colon_reads_the_lines_below_it() {
        let got = fields("درباره:\nخط اول\nخط دوم\nسن: ۳۷");
        assert_eq!(got["about"], crate::profile::Value::Text("خط اول\nخط دوم".into()));
        assert_eq!(got["age"], crate::profile::Value::Num(37.0));
    }

    #[test]
    fn a_multi_line_value_is_capped_at_the_value_limit() {
        use crate::ai::MAX_VALUE_CHARS;
        let long = "الف ".repeat(MAX_VALUE_CHARS);
        let got = fields(&format!("درباره:\n{long}\nسن: ۳۷"));
        let crate::profile::Value::Text(text) = &got["about"] else { panic!("{:?}", got.get("about")) };
        assert!(text.chars().count() > MAX_VALUE_CHARS / 2, "value not read below the heading");
        assert!(text.chars().count() <= MAX_VALUE_CHARS, "value not capped: {}", text.chars().count());
        assert!(!text.ends_with(char::is_whitespace));
        assert_eq!(got["age"], crate::profile::Value::Num(37.0));
    }

    #[test]
    fn text_in_brackets_and_photo_marker_lines_is_ignored() {
        let got = fields("( ۲۵ ساله و مذهبی )\nعکس ۲۸ ساله\nسن: ۳۸");
        assert_eq!(got["age"], crate::profile::Value::Num(38.0));
        assert!(!got.contains_key("religiosity"), "{got:?}");

        let got = fields("🌷 عکس من 👇\n photo 2, 30 years old");
        assert!(!got.contains_key("age"), "{got:?}");
    }

    #[test]
    fn a_multi_line_value_skips_an_instruction_line_but_keeps_reading() {
        let got = fields("درباره:\n( توضیح دهید )\nخط اول\nخط دوم\nسن: ۳۷");
        assert_eq!(got["about"], crate::profile::Value::Text("خط اول\nخط دوم".into()));
        assert_eq!(got["age"], crate::profile::Value::Num(37.0));
    }

    #[test]
    fn a_partner_block_is_ignored_up_to_the_next_labelled_line() {
        let got = fields("سن: ۳۸\nهمسر آینده:\n۲۵ ساله، مذهبی\nشغل: مهندس");
        assert_eq!(got["age"], crate::profile::Value::Num(38.0));
        assert_eq!(got["occupation"], crate::profile::Value::Text("مهندس".into()));
        assert!(!got.contains_key("religiosity"), "{got:?}");
    }

    #[test]
    fn a_keyword_inside_another_fields_value_belongs_to_that_field() {
        let got = fields("🌷شغل پدر: بازنشسته");
        assert_eq!(got["father_occupation"], crate::profile::Value::Text("بازنشسته".into()));
        assert!(!got.contains_key("employment_status"), "{got:?}");

        let got = fields("نام و نام خانوادگی: مهسا قویمی\nشغل: مهندس");
        assert_eq!(got["occupation"], crate::profile::Value::Text("مهندس".into()));
        assert!(!got.contains_key("employment_status"), "{got:?}");
    }

    #[test]
    fn a_keyword_inside_a_label_is_not_a_value() {
        let got = fields("🌷میزان پایبندی دینی: متوسط");
        assert_eq!(got["religiosity"], crate::profile::Value::Text("moderate".into()));

        let got = fields("🌷وضعیت تاهل: مجرد\nسن: ۳۰");
        assert_eq!(got["marital_status"], crate::profile::Value::Text("never_married".into()));
        assert_eq!(got["age"], crate::profile::Value::Num(30.0));

        // A label that names no field at all still must not feed a keyword: the words before the colon are a
        // heading, never a value.
        let got = fields("🌷یادداشت مذهبی: متن ساده\nسن: ۳۰");
        assert!(!got.contains_key("religiosity"), "{got:?}");
        assert_eq!(got["age"], crate::profile::Value::Num(30.0));
    }

    #[test]
    fn a_value_that_does_not_convert_still_belongs_to_its_field() {
        // «بازنشسته» is a value for a field the rules cannot fill, not a fact about employment: the heading
        // owns the text even though no suggestion comes out of it.
        let got = fields("🌷توضیح بازنشسته: متن ساده");
        assert!(!got.contains_key("employment_status"), "{got:?}");
    }

    #[test]
    fn a_labelled_choice_takes_an_option_or_the_longest_synonym_in_the_value() {
        let got = fields("میزان پایبندی دینی: خیلی مذهبی");
        assert_eq!(got["religiosity"], crate::profile::Value::Text("high".into()));

        // «غیرمذهبی» contains «مذهبی»; the longer synonym is the one that counts.
        let got = fields("میزان پایبندی دینی: غیرمذهبی");
        assert_eq!(got["religiosity"], crate::profile::Value::Text("low".into()));

        let got = fields("وضعیت تاهل: مطلقه");
        assert_eq!(got["marital_status"], crate::profile::Value::Text("divorced".into()));

        let got = fields("مصرف دخانیات: سیگاری نیست");
        assert_eq!(got["smoking"], crate::profile::Value::Text("never".into()));

        let got = fields("مصرف دخانیات: گاهی سیگار می‌کشد");
        assert_eq!(got["smoking"], crate::profile::Value::Text("occasionally".into()));

        // An option written exactly wins even when it is a single word.
        let got = fields("Religious observance: moderate");
        assert_eq!(got["religiosity"], crate::profile::Value::Text("moderate".into()));
    }

    #[test]
    fn a_labelled_education_takes_the_highest_level_without_ambiguity() {
        let reg = default_registry();
        let out = extract_with_rules(&reg, "سطح تحصیلات: کارشناسی ارشد مهندسی", None, &Profile::new("x"));
        assert!(!out.ambiguous.contains("education"), "{:?}", out.ambiguous);
        assert_eq!(fields("سطح تحصیلات: کارشناسی ارشد مهندسی")["education"], crate::profile::Value::Text("master".into()));
        assert_eq!(fields("Education: PhD in physics")["education"], crate::profile::Value::Text("doctorate".into()));
        assert_eq!(fields("تحصیلات: دیپلم")["education"], crate::profile::Value::Text("diploma".into()));
    }

    #[test]
    fn the_highest_education_keyword_wins_and_is_not_ambiguous() {
        let reg = default_registry();
        let out = extract_with_rules(&reg, "کارشناسی ارشد مهندسی، دانشگاه تهران", None, &Profile::new("x"));
        assert!(!out.ambiguous.contains("education"), "{:?}", out.ambiguous);
        let education = out.suggestions.iter().find(|s| s.field == "education").expect("education");
        assert_eq!(education.value, crate::profile::Value::Text("master".into()));
    }

    #[test]
    fn a_labelled_children_count_also_says_whether_there_are_children() {
        let got = fields("تعداد فرزندان: ۲");
        assert_eq!(got["children_count"], crate::profile::Value::Num(2.0));
        assert_eq!(got["has_children"], crate::profile::Value::Bool(true));

        let got = fields("تعداد فرزند: ۰");
        assert_eq!(got["children_count"], crate::profile::Value::Num(0.0));
        assert_eq!(got["has_children"], crate::profile::Value::Bool(false));
    }

    #[test]
    fn labelled_age_height_and_weight_use_their_ranges() {
        let got = fields("سن: ۳۷\nقد: ۱۷۲\nوزن: ۶۴");
        assert_eq!(got["age"], crate::profile::Value::Num(37.0));
        assert_eq!(got["height_cm"], crate::profile::Value::Num(172.0));
        assert_eq!(got["weight_kg"], crate::profile::Value::Num(64.0));

        let got = fields("قد: ۱.۷۲ متر");
        assert_eq!(got["height_cm"], crate::profile::Value::Num(172.0));

        assert!(!fields("سن: ۱۲۰").contains_key("age"));
        assert!(!fields("قد: ۴۰").contains_key("height_cm"));
        assert!(!fields("وزن: ۵۰۰").contains_key("weight_kg"));
    }

    #[test]
    fn a_place_that_is_both_a_city_and_a_province_fills_both() {
        let got = fields("شهر محل سکونت: تهران");
        assert_eq!(got["city"], crate::profile::Value::Text("تهران".into()));
        assert_eq!(got["province"], crate::profile::Value::Text("تهران".into()));

        let got = fields("🌷 استان و شهر محل سکونت: گیلان، رشت");
        assert_eq!(got["province"], crate::profile::Value::Text("گیلان".into()));
        assert_eq!(got["city"], crate::profile::Value::Text("رشت".into()));

        let got = fields("City: Tehran");
        assert_eq!(got["city"], crate::profile::Value::Text("tehran".into()));
        assert_eq!(got["province"], crate::profile::Value::Text("tehran".into()));
    }
}

