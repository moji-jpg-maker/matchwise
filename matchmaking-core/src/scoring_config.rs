//! Versioned scoring configuration: the numbers that turn dimension scores into a verdict.
//!
//! Why this is separate from the rule set: a rule set says *what* to measure, the scoring config says *how to read
//! the result*. Keeping the two apart means the same 75/50 cutoffs can be discussed, versioned and revisited without
//! touching a single rule, and it means every stored snapshot can say exactly which reading it was judged by.
//!
//! Three numbers, no more:
//! * `strong_threshold` — a dimension at or above this is `strong`.
//! * `mixed_threshold` — a dimension at or above this (but below strong) is `mixed`; below it is a concern.
//! * `prior_score` — the neutral value thin evidence is pulled towards in the ranking score.

use serde::{Deserialize, Serialize};

fn default_strong() -> f64 {
    75.0
}

fn default_mixed() -> f64 {
    50.0
}

fn default_prior() -> f64 {
    50.0
}

/// How dimension scores are labelled and how a sparse scorecard is pulled towards neutral.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoringConfig {
    /// A dimension score at or above this is `strong` (0..=100).
    #[serde(default = "default_strong")]
    pub strong_threshold: f64,
    /// A dimension score at or above this (and below `strong_threshold`) is `mixed` (0..=100).
    #[serde(default = "default_mixed")]
    pub mixed_threshold: f64,
    /// Neutral score (0..=100) that thin evidence is pulled towards in the ranking score.
    #[serde(default = "default_prior")]
    pub prior_score: f64,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        ScoringConfig { strong_threshold: default_strong(), mixed_threshold: default_mixed(), prior_score: default_prior() }
    }
}

/// Messages describing why a scoring config cannot be used. Empty means valid.
pub fn validate_scoring_config(cfg: &ScoringConfig) -> Vec<String> {
    let mut issues = vec![];
    let range = |v: f64| v.is_finite() && (0.0..=100.0).contains(&v);
    if !range(cfg.mixed_threshold) {
        issues.push("The mixed threshold must be between 0 and 100".into());
    }
    if !range(cfg.strong_threshold) {
        issues.push("The strong threshold must be between 0 and 100".into());
    }
    if range(cfg.mixed_threshold) && range(cfg.strong_threshold) && cfg.mixed_threshold >= cfg.strong_threshold {
        issues.push("The mixed threshold must be lower than the strong threshold".into());
    }
    if !range(cfg.prior_score) {
        issues.push("The neutral baseline score must be between 0 and 100".into());
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_the_documented_numbers() {
        let cfg = ScoringConfig::default();
        assert_eq!(cfg.strong_threshold, 75.0);
        assert_eq!(cfg.mixed_threshold, 50.0);
        assert_eq!(cfg.prior_score, 50.0);
        assert!(validate_scoring_config(&cfg).is_empty());
    }

    #[test]
    fn serde_fills_missing_fields_from_defaults() {
        let cfg: ScoringConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg, ScoringConfig::default());
        let cfg: ScoringConfig = serde_json::from_str(r#"{"prior_score":20}"#).unwrap();
        assert_eq!(cfg.prior_score, 20.0);
        assert_eq!(cfg.strong_threshold, 75.0);
        assert_eq!(cfg.mixed_threshold, 50.0);
    }

    #[test]
    fn serde_round_trip() {
        let cfg = ScoringConfig { strong_threshold: 80.0, mixed_threshold: 40.0, prior_score: 25.0 };
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<ScoringConfig>(&json).unwrap(), cfg);
    }

    #[test]
    fn validation_rejects_crossed_thresholds() {
        let base = ScoringConfig::default();
        let crossed = ScoringConfig { mixed_threshold: 75.0, ..base.clone() };
        assert_eq!(validate_scoring_config(&crossed).len(), 1);
        let inverted = ScoringConfig { strong_threshold: 30.0, mixed_threshold: 70.0, ..base.clone() };
        assert_eq!(validate_scoring_config(&inverted).len(), 1);
        let equal = ScoringConfig { strong_threshold: 60.0, mixed_threshold: 60.0, ..base.clone() };
        assert!(!validate_scoring_config(&equal).is_empty());
    }

    #[test]
    fn validation_rejects_out_of_range_values() {
        let base = ScoringConfig::default();
        assert_eq!(validate_scoring_config(&ScoringConfig { strong_threshold: 101.0, ..base.clone() }).len(), 1);
        assert_eq!(validate_scoring_config(&ScoringConfig { mixed_threshold: -1.0, ..base.clone() }).len(), 1);
        assert_eq!(validate_scoring_config(&ScoringConfig { prior_score: 100.5, ..base.clone() }).len(), 1);
        assert_eq!(validate_scoring_config(&ScoringConfig { prior_score: f64::NAN, ..base.clone() }).len(), 1);
        // Boundaries are allowed.
        assert!(validate_scoring_config(&ScoringConfig { strong_threshold: 100.0, mixed_threshold: 0.0, prior_score: 0.0 }).is_empty());
    }
}
