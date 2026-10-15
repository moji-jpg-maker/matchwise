//! Tauri-independent matchmaking domain logic.
//!
//! Nothing here depends on Tauri, SQLite, Telegram, or any LLM provider, so the
//! same engine can be reused by the desktop app, a Telegram adapter, or a future server.

pub mod ai;
pub mod defaults;
pub mod matching;
pub mod expr;
pub mod export;
pub mod lifecycle;
pub mod locale;
pub mod field;
pub mod intake;
pub mod preferences;
pub mod questionnaire;
pub mod rule_validation;
pub mod scoring;
pub mod scoring_config;
pub mod profile;
pub mod ranking;
pub mod search;
pub mod sharing;

pub use expr::{evaluate, Expr, Trace};
pub use locale::{interpolate as interpolate_message, localize_digits, Locale, Message, TextDirection};
pub use field::{FieldDef, FieldKind, FieldRegistry};
pub use intake::{parse_entries, parse_json_corpus, split_corpus, DropReason, DroppedEntry, IntakeEntry, ParsedCorpus, MAX_ENTRY_CHARS};
pub use defaults::{default_registry, default_rule_description_fa, default_ruleset};
pub use profile::{Profile, Provenance, ValidationIssue, Value};
pub use preferences::{
    describe_preference, describe_preference_for, evaluate_mutual, evaluate_preferences, preferences_to_ruleset, validate_preference, validate_preference_for, MutualOutcome, Preference, Strength,
};
pub use search::{condition_to_expr, conditions_to_expr, search, Condition, ConditionOp, RelativeValue, SearchHit};
pub mod rules;
pub use rules::{evaluate_pair, evaluate_ruleset_mutual, Direction, MatchOutcome, Rule, RuleKind, RuleResult, RuleScope, RuleSet};
pub use matching::{evaluate_match, MatchEvaluation};
pub use rule_validation::{validate_ruleset, validate_ruleset_for, RuleIssue};
pub use scoring::{
    dimension_catalog, resolve_dimension, score_pair, DimStatus, DimensionDef, DimensionScore, Finding, FindingSource, HardConstraints,
    HardStatus, MissingField, ScoreCard, other_dimension_text, localize_scorecard_json, PreferenceText,
};
pub use scoring_config::{validate_scoring_config, ScoringConfig};
pub use lifecycle::{
    apply_responses, can_record_outcome, can_record_responses, validate_text, validate_text_for, validate_transition, validate_transition_for, Interest, MatchStatus, Outcome,
    TransitionContext, TransitionPlan, TransitionRequest,
};
pub use sharing::{
    anonymous_name, build_introduction_card, build_introduction_card_for, display_value, display_value_for, first_name, first_name_for, first_name_opt, IntroCard, DEFAULT_INTRODUCTION_FIELDS, NEVER_SHARED};
pub use ai::{
    build_pair_facts, extract_json_object, extraction_prompt, extraction_prompt_for, pair_prompt, pair_prompt_for, parse_extraction, parse_pair_analysis, profile_text_for_ai, redact_text, AiFact,
    Claim, Contradiction, Extraction, FieldSuggestion, PairAnalysis, PreferenceSuggestion,
};
pub use export::{header as export_header, to_csv as rows_to_csv, ExportRow, RANKING_MINIMUM};
pub use ranking::{classify as classify_outcome, compare, ComparisonRow, OrderingResult, Progress, RankingComparison};
pub use questionnaire::{
    apply_scores, builtin_mini_ipip, missing_fields, next_item, score as score_answers, validate_answer, validate_answer_for, validate_instrument, validate_instrument_for, Evidence, EvidenceLevel,
    EvidenceSource, Instrument, Item, Method, Scale, Scores, Subscale,
};
