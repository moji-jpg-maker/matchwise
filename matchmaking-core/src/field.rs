use crate::profile::Value;
use crate::locale::{localize_digits, Locale};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Bool,
    Number,
    Text,
    /// Single choice from a fixed set.
    Choice(Vec<String>),
    MultiChoice(Vec<String>),
    /// A repeating group of sub-fields (for example children: gender, age, custody, ...).
    Records(Vec<FieldDef>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDef {
    pub key: String,
    pub label: String,
    /// Persian display label. Absent for fields a matchmaker defined without one; display falls back to `label`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_fa: Option<String>,
    pub kind: FieldKind,
    /// Sensitive fields (religion, health, finances, children...) get stricter
    /// visibility, redaction before cloud-LLM calls, and encryption handling.
    #[serde(default)]
    pub sensitive: bool,
    /// Counts toward profile completeness.
    #[serde(default)]
    pub required: bool,
}

impl FieldDef {
    pub fn label_for(&self, locale: Locale) -> &str {
        match locale {
            Locale::En => &self.label,
            Locale::Fa => self.label_fa.as_deref().filter(|text| !text.trim().is_empty()).unwrap_or(&self.label),
        }
    }
}

impl FieldKind {
    /// Check that `value` has the right shape for this kind of field.
    pub fn validate(&self, value: &Value) -> Result<(), String> {
        self.validate_for(value, Locale::En)
    }

    /// Like [`FieldKind::validate`], with the message in `locale`.
    pub fn validate_for(&self, value: &Value, locale: Locale) -> Result<(), String> {
        let not_one_of = |item: &str, opts: &Vec<String>| match locale {
            Locale::En => format!("'{item}' is not one of {opts:?}"),
            Locale::Fa => format!("«{item}» جزو گزینه‌های مجاز نیست: {}", opts.join("، ")),
        };
        match (self, value) {
            (FieldKind::Bool, Value::Bool(_)) => Ok(()),
            (FieldKind::Number, Value::Num(n)) if n.is_finite() => Ok(()),
            (FieldKind::Number, Value::Num(_)) => Err(locale.pick("number must be finite", "عدد باید متناهی باشد").into()),
            (FieldKind::Text, Value::Text(_)) => Ok(()),
            (FieldKind::Choice(opts), Value::Text(t)) => {
                if opts.iter().any(|o| o == t) { Ok(()) } else { Err(not_one_of(t, opts)) }
            }
            (FieldKind::MultiChoice(opts), Value::List(items)) => {
                match items.iter().find(|i| !opts.iter().any(|o| o == *i)) {
                    Some(bad) => Err(not_one_of(bad, opts)),
                    None => Ok(()),
                }
            }
            (FieldKind::Records(_), Value::List(items)) if items.is_empty() => Ok(()),
            (FieldKind::Records(defs), Value::Records(rows)) => {
                if rows.len() > 50 {
                    return Err(locale.pick("too many entries (max 50)", "تعداد موارد بیش از حد است (حداکثر ۵۰)").into());
                }
                for (i, row) in rows.iter().enumerate() {
                    for (k, v) in row {
                        match defs.iter().find(|d| &d.key == k) {
                            Some(d) => d.kind.validate_for(v, locale).map_err(|e| match locale {
                                Locale::En => format!("entry {}: {k}: {e}", i + 1),
                                Locale::Fa => format!("مورد {}: {k}: {e}", localize_digits(locale, &(i + 1).to_string())),
                            })?,
                            None => return Err(match locale {
                                Locale::En => format!("entry {}: unknown sub-field '{k}'", i + 1),
                                Locale::Fa => format!("مورد {}: زیرفیلد ناشناخته «{k}»", localize_digits(locale, &(i + 1).to_string())),
                            }),
                        }
                    }
                }
                Ok(())
            }
            (k, v) => Err(match locale {
                Locale::En => format!("expected {}, got {}", k.name(), v.type_name()),
                Locale::Fa => format!("مقدار مورد انتظار از نوع {} بود، اما {} دریافت شد", k.name(), v.type_name()),
            }),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FieldKind::Bool => "bool",
            FieldKind::Number => "number",
            FieldKind::Text => "text",
            FieldKind::Choice(_) => "choice",
            FieldKind::MultiChoice(_) => "multi_choice",
            FieldKind::Records(_) => "records",
        }
    }
}

/// Matchmakers add fields here at runtime; no schema migration needed. Keeps insertion order so the
/// UI shows fields in the order the matchmaker (or the default set) defines.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldRegistry {
    fields: Vec<FieldDef>,
}

impl FieldRegistry {
    pub fn new() -> Self { Self::default() }

    /// Add a field, or replace the definition of an existing key in place (keeping its position).
    pub fn register(&mut self, def: FieldDef) {
        match self.fields.iter_mut().find(|d| d.key == def.key) {
            Some(existing) => *existing = def,
            None => self.fields.push(def),
        }
    }

    pub fn get(&self, key: &str) -> Option<&FieldDef> { self.fields.iter().find(|d| d.key == key) }

    pub fn is_sensitive(&self, key: &str) -> bool {
        self.get(key).map(|d| d.sensitive).unwrap_or(true) // unknown => treat as sensitive
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> { self.fields.iter().map(|d| &d.key) }

    pub fn defs(&self) -> impl Iterator<Item = &FieldDef> { self.fields.iter() }

    /// Validate a value for a registered field. Unknown fields are rejected.
    pub fn validate_value(&self, key: &str, value: &Value) -> Result<(), String> {
        self.validate_value_for(key, value, Locale::En)
    }

    /// Like [`FieldRegistry::validate_value`], with the message in `locale`.
    pub fn validate_value_for(&self, key: &str, value: &Value, locale: Locale) -> Result<(), String> {
        match self.get(key) {
            Some(def) => def.kind.validate_for(value, locale),
            None => Err(match locale {
                Locale::En => format!("unknown field '{key}'"),
                Locale::Fa => format!("فیلد ناشناخته «{key}»"),
            }),
        }
    }
}
