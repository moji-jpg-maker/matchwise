//! Tauri-independent matchmaking domain logic.
//!
//! Nothing here depends on Tauri, SQLite, Telegram, or any LLM provider, so the
//! same engine can be reused by the desktop app, a Telegram adapter, or a future server.

pub mod ai;
pub mod defaults;
pub mod matching;
pub mod expr;
pub mod lifecycle;
pub mod field;
pub mod preferences;
pub mod rule_validation;
pub mod scoring;
pub mod profile;
pub mod search;
pub mod sharing;

pub use expr::{evaluate, Expr, Trace};
pub use field::{FieldDef, FieldKind, FieldRegistry};
pub use defaults::{default_registry, default_ruleset};
pub use profile::{Profile, Provenance, ValidationIssue, Value};
pub use preferences::{
    describe_preference, evaluate_mutual, evaluate_preferences, preferences_to_ruleset, validate_preference, MutualOutcome, Preference, Strength,
};
pub use search::{condition_to_expr, conditions_to_expr, search, Condition, ConditionOp, RelativeValue, SearchHit};
pub mod rules;
pub use rules::{evaluate_pair, evaluate_ruleset_mutual, Direction, MatchOutcome, Rule, RuleKind, RuleResult, RuleScope, RuleSet};
pub use matching::{evaluate_match, MatchEvaluation};
pub use rule_validation::{validate_ruleset, RuleIssue};
pub use scoring::{
    dimension_catalog, resolve_dimension, score_pair, DimStatus, DimensionDef, DimensionScore, Finding, FindingSource, HardConstraints,
    HardStatus, MissingField, ScoreCard,
};
pub use lifecycle::{
    apply_responses, can_record_outcome, can_record_responses, validate_text, validate_transition, Interest, MatchStatus, Outcome,
    TransitionContext, TransitionPlan, TransitionRequest,
};
pub use sharing::{build_introduction_card, display_value, first_name, IntroCard, DEFAULT_INTRODUCTION_FIELDS, NEVER_SHARED};
pub use ai::{
    build_pair_facts, extract_json_object, extraction_prompt, pair_prompt, parse_extraction, parse_pair_analysis, profile_text_for_ai, redact_text, AiFact,
    Claim, Contradiction, Extraction, FieldSuggestion, PairAnalysis, PreferenceSuggestion,
};
