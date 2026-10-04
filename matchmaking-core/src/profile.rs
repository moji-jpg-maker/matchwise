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
