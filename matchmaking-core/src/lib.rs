//! Tauri-independent matchmaking domain logic.
//!
//! Nothing here depends on Tauri, SQLite, Telegram, or any LLM provider, so the
//! same engine can be reused by the desktop app, a Telegram adapter, or a future server.

pub mod defaults;
pub mod expr;
pub mod field;
pub mod profile;
pub mod search;

pub use expr::{evaluate, Expr, Trace};
pub use field::{FieldDef, FieldKind, FieldRegistry};
pub use defaults::default_registry;
pub use profile::{Profile, Provenance, ValidationIssue, Value};
pub use search::{conditions_to_expr, search, Condition, ConditionOp, SearchHit};
pub mod rules;
pub use rules::{evaluate_pair, MatchOutcome, Rule, RuleKind, RuleSet};
