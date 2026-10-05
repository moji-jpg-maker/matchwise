//! Tauri-independent matchmaking domain logic.
//!
//! Nothing here depends on Tauri, SQLite, Telegram, or any LLM provider, so the
//! same engine can be reused by the desktop app, a Telegram adapter, or a future server.

pub mod defaults;
pub mod expr;
pub mod field;
pub mod preferences;
pub mod profile;
pub mod search;

pub use expr::{evaluate, Expr, Trace};
pub use field::{FieldDef, FieldKind, FieldRegistry};
pub use defaults::default_registry;
pub use profile::{Profile, Provenance, ValidationIssue, Value};
pub use preferences::{
    evaluate_mutual, evaluate_preferences, preferences_to_ruleset, validate_preference, MutualOutcome, Preference, Strength,
};
pub use search::{condition_to_expr, conditions_to_expr, search, Condition, ConditionOp, RelativeValue, SearchHit};
pub mod rules;
pub use rules::{evaluate_pair, MatchOutcome, Rule, RuleKind, RuleSet};
