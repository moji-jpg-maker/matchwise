use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where a value came from. AI-inferred values must never silently overwrite
/// user/matchmaker-provided ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    User,
    Matchmaker,
    Questionnaire,
    AiInferred,
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Bool(_) => "bool",
            Value::Num(_) => "number",
            Value::Text(_) => "text",
            Value::List(_) => "list",
        }
    }
}

impl Provenance {
    /// Higher rank wins when two sources disagree.
    pub fn rank(self) -> u8 {
        match self {
            Provenance::AiInferred => 0,
            Provenance::Questionnaire => 1,
            Provenance::User => 2,
            Provenance::Matchmaker => 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Bool(bool),
    Num(f64),
    Text(String),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub value: Value,
    pub source: Provenance,
}

/// A profile is a bag of registry-defined fields, not a fixed table row.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub fields: BTreeMap<String, Entry>,
}

impl Profile {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), fields: BTreeMap::new() }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key).map(|e| &e.value)
    }

    /// Set a field. An AI-inferred value is refused if a higher-trust value exists.
    /// Returns true if the value was stored.
    pub fn set(&mut self, key: &str, value: Value, source: Provenance) -> bool {
        if let Some(existing) = self.fields.get(key) {
            if source.rank() < existing.source.rank() {
                return false;
            }
        }
        self.fields.insert(key.to_string(), Entry { value, source });
        true
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
}

impl Profile {
    /// Report every stored value that does not fit its registered field.
    pub fn validate(&self, registry: &crate::field::FieldRegistry) -> Vec<ValidationIssue> {
        self.fields
            .iter()
            .filter_map(|(k, e)| {
                registry.validate_value(k, &e.value).err().map(|message| ValidationIssue { field: k.clone(), message })
            })
            .collect()
    }

    /// Fraction (0..=1) of `required` fields that have a value; None if no field is required.
    pub fn completeness(&self, registry: &crate::field::FieldRegistry) -> Option<f64> {
        let required: Vec<&str> = registry.defs().filter(|d| d.required).map(|d| d.key.as_str()).collect();
        if required.is_empty() {
            return None;
        }
        let filled = required.iter().filter(|k| self.fields.contains_key(**k)).count();
        Some(filled as f64 / required.len() as f64)
    }

    /// Required fields that are still empty: the "missing information" list.
    pub fn missing_required(&self, registry: &crate::field::FieldRegistry) -> Vec<String> {
        registry
            .defs()
            .filter(|d| d.required && !self.fields.contains_key(&d.key))
            .map(|d| d.key.clone())
            .collect()
    }
}
