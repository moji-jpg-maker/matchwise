//! Turning recorded decisions into a learning set (M7b).
//!
//! The dataset exists so ranking can later be *checked against* what actually happened, so two properties matter more
//! than convenience:
//! * **No profile content.** A row carries the pair's identities and what was decided, never a name, a field value or a
//!   note. Everything the export needs lives in the match record itself.
//! * **Reproducible text.** The same decisions must always serialise to the same bytes, so rows are emitted in a fixed
//!   order and numbers are formatted deterministically.
//!
//! The output is intentionally not localized: headers are machine keys, numbers use ASCII digits and timestamps are
//! ISO-8601, whatever language the interface uses. Only the screens that trigger and describe the export are
//! translated.

/// How many finished matches are needed before a learned ranking could be compared with the deterministic baseline.
/// The gate is *reported*, never enforced: the dataset and the readiness view both quote this one number, so a
/// matchmaker reading either sees the same threshold. Below it a comparison would fit noise.
pub const RANKING_MINIMUM: i64 = 100;

/// One decision, flattened for export. Mirrors the app layer's `OutcomeRow`; kept here so formatting is testable
/// without a database.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRow {
    pub match_id: String,
    pub pair_key: String,
    pub profile_a: String,
    pub profile_b: String,
    pub status: String,
    pub a_response: String,
    pub b_response: String,
    pub outcome: Option<String>,
    pub was_overridden: bool,
    pub rule_set_id: String,
    pub rule_set_version: u32,
    /// Which scoring config judged this pair, and which version of it. Blank for decisions recorded before scoring
    /// configs existed: the numbers a past snapshot used were never stored, so they are not reconstructed here.
    pub scoring_config_id: Option<String>,
    pub scoring_config_version: Option<u32>,
    pub snapshot_trigger: Option<String>,
    pub scorecard_at: Option<String>,
    pub overall: Option<f64>,
    pub confidence: Option<f64>,
    pub ranking_score: Option<f64>,
    pub eligible: Option<bool>,
    pub meets_threshold: Option<bool>,
    /// Dimension key -> score, for dimensions that had a score at snapshot time.
    pub dimension_scores: Vec<(String, f64)>,
    pub created_at: String,
    pub updated_at: String,
}

/// The fixed columns, in order, before the per-dimension ones.
pub const BASE_COLUMNS: &[&str] = &[
    "match_id",
    "pair_key",
    "profile_a",
    "profile_b",
    "status",
    "a_response",
    "b_response",
    "outcome",
    "was_overridden",
    "rule_set_id",
    "rule_set_version",
    "scoring_config_id",
    "scoring_config_version",
    "snapshot_trigger",
    "scorecard_at",
    "overall",
    "confidence",
    "ranking_score",
    "eligible",
    "meets_threshold",
    "created_at",
    "updated_at",
];

/// Compose the header: the base columns, then `dim_<key>` for every dimension that appears anywhere in the data.
pub fn header(dimension_keys: &[String]) -> Vec<String> {
    let mut cols: Vec<String> = BASE_COLUMNS.iter().map(|c| c.to_string()).collect();
    cols.extend(dimension_keys.iter().map(|k| format!("dim_{k}")));
    cols
}

/// Escape one CSV field. Anything containing a quote, comma, newline or carriage return is quoted, with embedded quotes
/// doubled, per RFC 4180.
pub fn escape(field: &str) -> String {
    if field.contains('"') || field.contains(',') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Format a number so the same value always serialises the same way: no exponent, no trailing zeros, no locale.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

fn opt_num(v: Option<f64>) -> String {
    v.map(num).unwrap_or_default()
}

fn opt_bool(v: Option<bool>) -> String {
    v.map(|b| if b { "true" } else { "false" }).unwrap_or("").to_string()
}

/// One row as an escaped CSV line (without a trailing newline).
pub fn row_line(r: &ExportRow, dimension_keys: &[String]) -> String {
    let mut fields: Vec<String> = vec![
        escape(&r.match_id),
        escape(&r.pair_key),
        escape(&r.profile_a),
        escape(&r.profile_b),
        escape(&r.status),
        escape(&r.a_response),
        escape(&r.b_response),
        escape(r.outcome.as_deref().unwrap_or("")),
        opt_bool(Some(r.was_overridden)),
        escape(&r.rule_set_id),
        r.rule_set_version.to_string(),
        escape(r.scoring_config_id.as_deref().unwrap_or("")),
        r.scoring_config_version.map(|v| v.to_string()).unwrap_or_default(),
        escape(r.snapshot_trigger.as_deref().unwrap_or("")),
        escape(r.scorecard_at.as_deref().unwrap_or("")),
        opt_num(r.overall),
        opt_num(r.confidence),
        opt_num(r.ranking_score),
        opt_bool(r.eligible),
        opt_bool(r.meets_threshold),
        escape(&r.created_at),
        escape(&r.updated_at),
    ];
    // A dimension the row did not score stays blank; blank means "no score", which is not the same as zero.
    for key in dimension_keys {
        let score = r.dimension_scores.iter().find(|(k, _)| k == key).map(|(_, s)| *s);
        fields.push(opt_num(score));
    }
    fields.join(",")
}

/// The whole document as CSV, header included. Rows keep the order they are given in, so the caller decides it.
pub fn to_csv(rows: &[ExportRow], dimension_keys: &[String]) -> String {
    let mut out = String::new();
    out.push_str(&header(dimension_keys).iter().map(|c| escape(c)).collect::<Vec<_>>().join(","));
    out.push('\n');
    for r in rows {
        out.push_str(&row_line(r, dimension_keys));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row() -> ExportRow {
        ExportRow {
            match_id: "m1".into(),
            pair_key: "p1|p2".into(),
            profile_a: "p1".into(),
            profile_b: "p2".into(),
            status: "closed".into(),
            a_response: "interested".into(),
            b_response: "not_interested".into(),
            outcome: Some("stopped".into()),
            was_overridden: false,
            rule_set_id: "default".into(),
            rule_set_version: 3,
            scoring_config_id: Some("sc1".into()),
            scoring_config_version: Some(2),
            snapshot_trigger: Some("created".into()),
            scorecard_at: Some("2026-10-12T08:00:00+00:00".into()),
            overall: Some(72.5),
            confidence: Some(0.6),
            ranking_score: Some(63.5),
            eligible: Some(true),
            meets_threshold: Some(true),
            dimension_scores: vec![("age".into(), 80.0), ("location".into(), 55.25)],
            created_at: "2026-10-01T08:00:00+00:00".into(),
            updated_at: "2026-10-05T08:00:00+00:00".into(),
        }
    }

    #[test]
    fn header_is_base_columns_then_sorted_dimensions() {
        let cols = header(&["age".to_string(), "location".to_string()]);
        assert_eq!(cols.len(), BASE_COLUMNS.len() + 2);
        assert_eq!(&cols[..BASE_COLUMNS.len()], BASE_COLUMNS);
        assert_eq!(&cols[BASE_COLUMNS.len()..], &["dim_age".to_string(), "dim_location".to_string()]);
    }

    #[test]
    fn fields_with_commas_quotes_or_newlines_are_escaped() {
        assert_eq!(escape("plain"), "plain");
        assert_eq!(escape("a,b"), "\"a,b\"");
        assert_eq!(escape("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(escape("two\nlines"), "\"two\nlines\"");
        assert_eq!(escape("cr\rlf"), "\"cr\rlf\"");
    }

    #[test]
    fn numbers_are_stable_and_missing_stays_blank() {
        assert_eq!(num(72.5), "72.5");
        assert_eq!(num(80.0), "80");
        assert_eq!(num(55.25), "55.25");
        assert_eq!(num(0.0), "0");
        assert_eq!(num(-12.5), "-12.5");
        // no exponent for small or large values
        assert_eq!(num(0.0001), "0.0001");
        assert!(!num(1234567.0).contains('e'));
        assert_eq!(opt_num(None), "");
        assert_eq!(opt_bool(None), "");
    }

    #[test]
    fn a_row_has_one_field_per_column_even_when_data_is_missing() {
        let mut r = row();
        r.outcome = None;
        r.eligible = None;
        r.dimension_scores = vec![("age".into(), 80.0)]; // location missing
        let keys = vec!["age".to_string(), "location".to_string()];
        let line = row_line(&r, &keys);
        assert_eq!(line.split(',').count(), BASE_COLUMNS.len() + 2);
        assert!(line.ends_with(",80,"), "a dimension with no score is blank, not zero: {line}");
    }

    #[test]
    fn csv_has_a_header_and_one_line_per_row_in_the_given_order() {
        let csv = to_csv(&[row()], &["age".to_string()]);
        let lines: Vec<&str> = csv.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("match_id,pair_key,profile_a"));
        assert!(lines[0].ends_with("dim_age"));
        assert_eq!(lines[1], row_line(&row(), &["age".to_string()]));
    }

    #[test]
    fn the_export_carries_no_profile_content() {
        let csv = to_csv(&[row()], &["age".to_string()]);
        let lower = csv.to_lowercase();
        for forbidden in ["full_name", "phone", "email", "telegram", "note", "override_reason"] {
            assert!(!lower.contains(forbidden), "export must not carry profile content: {forbidden}");
        }
    }
}
