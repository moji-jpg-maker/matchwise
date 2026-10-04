//! Tauri-independent matchmaking domain logic.
//!
//! Nothing here depends on Tauri, SQLite, Telegram, or any LLM provider, so the
//! same engine can be reused by the desktop app, a Telegram adapter, or a future server.

pub mod expr;
pub mod field;
pub mod profile;

pub use expr::{evaluate, Expr, Trace};
pub use field::{FieldDef, FieldKind, FieldRegistry};
pub use profile::{Profile, Provenance, Value};
pub mod rules;
pub use rules::{evaluate_pair, MatchOutcome, Rule, RuleKind, RuleSet};
