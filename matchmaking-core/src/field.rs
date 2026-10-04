use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Bool,
    Number,
    Text,
    /// Single choice from a fixed set.
    Choice(Vec<String>),
    MultiChoice(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    /// Sensitive fields (religion, health, finances, children...) get stricter
    /// visibility, redaction before cloud-LLM calls, and encryption handling.
    #[serde(default)]
    pub sensitive: bool,
}

/// Matchmakers add fields here at runtime; no schema migration needed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldRegistry {
    fields: BTreeMap<String, FieldDef>,
}

impl FieldRegistry {
    pub fn new() -> Self { Self::default() }

    pub fn register(&mut self, def: FieldDef) {
        self.fields.insert(def.key.clone(), def);
    }

    pub fn get(&self, key: &str) -> Option<&FieldDef> { self.fields.get(key) }

    pub fn is_sensitive(&self, key: &str) -> bool {
        self.get(key).map(|d| d.sensitive).unwrap_or(true) // unknown => treat as sensitive
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> { self.fields.keys() }
}
