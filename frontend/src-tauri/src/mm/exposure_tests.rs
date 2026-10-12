use super::exposure_repo::{ExposureRepo, ExposureRow};
use super::match_repo::MatchRepo;
use super::repository::MmRepository;
use super::rules_commands::{find_matches, MatchCandidate};
use super::scoring_config_repo::ScoringConfigRepo;
use matchmaking_core::expr::{CmpOp, Expr, Operand, Side};
use matchmaking_core::{MatchStatus, Outcome, Profile, Provenance, Rule, RuleKind, RuleSet, ScoringConfig, Value};
use serde_json::json;
use sqlx::sqlite::{SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};

const BEFORE: &str = "2026-01-01T00:00:00+00:00";
const AT: &str = "2026-01-02T00:00:00+00:00";
const AFTER: &str = "2026-01-03T00:00:00+00:00";

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::query("PRAGMA foreign_keys = ON").execute(&pool).await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    assert_eq!(sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys").fetch_one(&pool).await.unwrap(), 1);
    pool
}

async fn profile(pool: &SqlitePool, id: &str, age: f64) {
    let mut person = Profile::new(id);
    person.set("age", Value::Num(age), Provenance::Matchmaker);
    MmRepository::insert_profile(pool, &person).await.unwrap();
}

fn age_check(cmp: CmpOp, threshold: f64) -> Expr {
    Expr::Cmp {
        left: Operand::Field { of: Side::B, key: "age".into() },
        cmp,
        right: Operand::Lit { value: Value::Num(threshold) },
    }
}

fn rules() -> RuleSet {
    let mut checks = vec![Rule::new("adult", "Adult candidate", RuleKind::Hard, 1.0, age_check(CmpOp::Ge, 18.0))];
    for threshold in [25, 30, 35] {
        let mut check = Rule::new(&format!("age-{threshold}"), "Age preference", RuleKind::Soft, 1.0, age_check(CmpOp::Ge, threshold as f64));
        check.group = Some("age_life_stage".into());
        checks.push(check);
    }
    RuleSet::new("Exposure rules", 1, checks)
}

async fn rule_set(pool: &SqlitePool) -> String {
    MmRepository::create_rule_set(pool, "Exposure rules", "", &rules(), None).await.unwrap()
}

async fn config(pool: &SqlitePool, rule_id: &str) -> String {
    let id = ScoringConfigRepo::create(pool, "Exposure config", "", &ScoringConfig::default(), None).await.unwrap();
    ScoringConfigRepo::set_rule_set_config(pool, rule_id, Some(&id)).await.unwrap();
    id
}

fn exposure(candidate: &str, rank: i64) -> ExposureRow {
    ExposureRow { candidate_id: candidate.into(), rank, shown: true, score: Some(75.0), confidence: Some(0.6), eligible: Some(true) }
}

async fn record(pool: &SqlitePool, search: &str, source: &str, rule_id: &str, candidates: &[ExposureRow]) {
    ExposureRepo::record(pool, search, source, rule_id, 1, None, None, candidates).await.unwrap();
}

async fn rows(pool: &SqlitePool) -> Vec<SqliteRow> {
    sqlx::query("SELECT * FROM mm_exposure ORDER BY id").fetch_all(pool).await.unwrap()
}

async fn search_rows(pool: &SqlitePool, search: &str) -> Vec<SqliteRow> {
    sqlx::query("SELECT * FROM mm_exposure WHERE search_id = ? ORDER BY rank").bind(search).fetch_all(pool).await.unwrap()
}

fn assert_candidates(
    stored: &[SqliteRow],
    returned: &[MatchCandidate],
    source: &str,
    rule_id: &str,
    rule_version: i64,
    config_id: Option<&str>,
    config_version: Option<i64>,
) {
    assert_eq!(stored.len(), returned.len());
    for (rank, (row, candidate)) in stored.iter().zip(returned).enumerate() {
        assert!(row.get::<i64, _>("id") > 0);
        assert_eq!(row.get::<String, _>("profile_id"), source);
        assert_eq!(row.get::<String, _>("candidate_id"), candidate.summary.id);
        assert_eq!(row.get::<i64, _>("rank"), rank as i64);
        assert_eq!(row.get::<i64, _>("shown"), 1);
        assert_eq!(row.get::<Option<f64>, _>("score"), candidate.score);
        assert_eq!(row.get::<Option<f64>, _>("confidence"), candidate.confidence);
        assert_eq!(row.get::<Option<bool>, _>("eligible"), Some(candidate.eligible));
        assert_eq!(row.get::<String, _>("rule_set_id"), rule_id);
        assert_eq!(row.get::<i64, _>("rule_set_version"), rule_version);
        assert_eq!(row.get::<Option<String>, _>("scoring_config_id").as_deref(), config_id);
        assert_eq!(row.get::<Option<i64>, _>("scoring_config_version"), config_version);
        assert_eq!(row.get::<String, _>("search_id"), stored[0].get::<String, _>("search_id"));
        assert_eq!(row.get::<String, _>("at"), stored[0].get::<String, _>("at"));
        chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("at")).unwrap();
    }
}

async fn tracked(pool: &SqlitePool, source: &str, candidate: &str, rule_id: &str, status: MatchStatus) -> String {
    MatchRepo::create(pool, source, candidate, None, rule_id, 1, None, None, status, &json!({"eligible": true})).await.unwrap()
}

async fn historical_exposure(pool: &SqlitePool, search: &str, source: &str, candidate: &str, shown: bool, timestamp: &str) {
    sqlx::query("INSERT INTO mm_exposure (search_id, profile_id, candidate_id, rank, shown, at) VALUES (?, ?, ?, 0, ?, ?)")
        .bind(search).bind(source).bind(candidate).bind(shown).bind(timestamp).execute(pool).await.unwrap();
}

async fn outcome_at(pool: &SqlitePool, match_id: &str, timestamp: &str) {
    MatchRepo::set_outcome(pool, match_id, Outcome::Unknown).await.unwrap();
    sqlx::query("UPDATE mm_match_events SET at = ? WHERE match_id = ? AND kind = 'outcome_recorded'")
        .bind(timestamp).bind(match_id).execute(pool).await.unwrap();
    sqlx::query("UPDATE mm_matches SET updated_at = ? WHERE id = ?").bind(timestamp).bind(match_id).execute(pool).await.unwrap();
}

#[tokio::test]
async fn record_persists_every_field_with_shared_search_and_timestamp() {
    let pool = test_pool().await;
    for id in ["source", "one", "two", "three"] { profile(&pool, id, 30.0).await; }
    let rule_id = rule_set(&pool).await;
    let config_id = config(&pool, &rule_id).await;
    let candidates = vec![
        exposure("one", 0),
        ExposureRow { candidate_id: "two".into(), rank: 1, shown: false, score: Some(0.0), confidence: Some(0.0), eligible: Some(false) },
        ExposureRow { candidate_id: "three".into(), rank: 2, shown: true, score: None, confidence: None, eligible: None },
    ];
    let started = chrono::Utc::now();
    ExposureRepo::record(&pool, "search", "source", &rule_id, 7, Some(&config_id), Some(9), &candidates).await.unwrap();
    let finished = chrono::Utc::now();
    let stored = rows(&pool).await;
    assert_eq!(stored.len(), 3);
    for (row, candidate) in stored.iter().zip(&candidates) {
        assert!(row.get::<i64, _>("id") > 0);
        assert_eq!(row.get::<String, _>("search_id"), "search");
        assert_eq!(row.get::<String, _>("profile_id"), "source");
        assert_eq!(row.get::<String, _>("candidate_id"), candidate.candidate_id);
        assert_eq!(row.get::<String, _>("rule_set_id"), rule_id);
        assert_eq!(row.get::<i64, _>("rule_set_version"), 7);
        assert_eq!(row.get::<String, _>("scoring_config_id"), config_id);
        assert_eq!(row.get::<i64, _>("scoring_config_version"), 9);
        assert_eq!(row.get::<i64, _>("rank"), candidate.rank);
        assert_eq!(row.get::<i64, _>("shown"), i64::from(candidate.shown));
        assert_eq!(row.get::<Option<f64>, _>("score"), candidate.score);
        assert_eq!(row.get::<Option<f64>, _>("confidence"), candidate.confidence);
        assert_eq!(row.get::<Option<i64>, _>("eligible"), candidate.eligible.map(i64::from));
        assert_eq!(row.get::<String, _>("at"), stored[0].get::<String, _>("at"));
        let timestamp = chrono::DateTime::parse_from_rfc3339(&row.get::<String, _>("at")).unwrap();
        assert!(timestamp >= started && timestamp <= finished);
    }
    record(&pool, "nullable-config", "source", &rule_id, &[exposure("one", 0)]).await;
    let stored = search_rows(&pool, "nullable-config").await;
    assert_eq!(stored[0].get::<Option<String>, _>("scoring_config_id"), None);
    assert_eq!(stored[0].get::<Option<i64>, _>("scoring_config_version"), None);
}

#[tokio::test]
async fn empty_batches_write_nothing_and_invalid_batches_roll_back() {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    profile(&pool, "candidate", 30.0).await;
    let rule_id = rule_set(&pool).await;
    record(&pool, "empty", "source", &rule_id, &[]).await;
    assert!(rows(&pool).await.is_empty());
    record(&pool, "existing", "source", &rule_id, &[exposure("candidate", 0)]).await;
    let result = ExposureRepo::record(&pool, "invalid", "source", &rule_id, 1, None, None, &[exposure("candidate", 0), exposure("missing", 1)]).await;
    assert!(result.is_err());
    assert!(search_rows(&pool, "invalid").await.is_empty());
    assert_eq!(rows(&pool).await.len(), 1);
}

#[tokio::test]
async fn repeated_batches_retain_separate_rows() {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    profile(&pool, "candidate", 30.0).await;
    let rule_id = rule_set(&pool).await;
    for search in ["first", "second"] { record(&pool, search, "source", &rule_id, &[exposure("candidate", 0)]).await; }
    let stored = rows(&pool).await;
    assert_eq!(stored.len(), 2);
    assert_ne!(stored[0].get::<i64, _>("id"), stored[1].get::<i64, _>("id"));
    assert_ne!(stored[0].get::<String, _>("search_id"), stored[1].get::<String, _>("search_id"));
}

#[tokio::test]
async fn deleting_either_profile_cascades_only_related_exposures() {
    for deleted in ["source", "candidate"] {
        let pool = test_pool().await;
        for id in ["source", "candidate", "other-source", "other-candidate"] { profile(&pool, id, 30.0).await; }
        let rule_id = rule_set(&pool).await;
        record(&pool, "related", "source", &rule_id, &[exposure("candidate", 0)]).await;
        record(&pool, "reverse", "candidate", &rule_id, &[exposure("source", 0)]).await;
        record(&pool, "unrelated", "other-source", &rule_id, &[exposure("other-candidate", 0)]).await;
        assert_eq!(MmRepository::delete_profile(&pool, deleted).await.unwrap(), 1);
        let remaining = rows(&pool).await;
        assert_eq!(remaining.len(), 1, "deleting {deleted}");
        assert_eq!(remaining[0].get::<String, _>("search_id"), "unrelated");
    }
}

#[tokio::test]
async fn searches_log_exactly_the_returned_candidates_and_filter_before_logging() {
    let pool = test_pool().await;
    for (id, age) in [("source", 40.0), ("best", 40.0), ("middle", 32.0), ("last", 22.0), ("excluded", 16.0), ("inactive", 40.0)] {
        profile(&pool, id, age).await;
    }
    MmRepository::set_status(&pool, "inactive", "deactivated").await.unwrap();
    let rule_id = rule_set(&pool).await;
    let config_id = config(&pool, &rule_id).await;
    let returned = find_matches(&pool, "source", &rule_id, Some(2), false).await.unwrap();
    assert_eq!(returned.iter().map(|candidate| candidate.summary.id.as_str()).collect::<Vec<_>>(), ["best", "middle"]);
    assert_candidates(&rows(&pool).await, &returned, "source", &rule_id, 1, Some(&config_id), Some(1));
    let included = find_matches(&pool, "source", &rule_id, Some(10), true).await.unwrap();
    assert_eq!(included.iter().map(|candidate| candidate.summary.id.as_str()).collect::<Vec<_>>(), ["best", "middle", "last", "excluded"]);
    assert!(!included[3].eligible);
    let stored = rows(&pool).await;
    let search = stored.last().unwrap().get::<String, _>("search_id");
    assert_candidates(&search_rows(&pool, &search).await, &included, "source", &rule_id, 1, Some(&config_id), Some(1));
    assert_eq!(stored.len(), 6);
}

#[tokio::test]
async fn search_limits_cover_zero_explicit_default_and_maximum() {
    for (population, limit, expected) in [(6, Some(3), 3), (6, Some(0), 0), (55, None, 50), (505, Some(500), 500), (505, Some(999), 500)] {
        let pool = test_pool().await;
        profile(&pool, "source", 40.0).await;
        for index in 0..population { profile(&pool, &format!("candidate-{index}"), 40.0).await; }
        let rule_id = rule_set(&pool).await;
        let returned = find_matches(&pool, "source", &rule_id, limit, false).await.unwrap();
        assert_eq!(returned.len(), expected, "population={population}, limit={limit:?}");
        assert_candidates(&rows(&pool).await, &returned, "source", &rule_id, 1, None, None);
    }
}

#[tokio::test]
async fn empty_results_and_missing_inputs_add_no_exposures() {
    let pool = test_pool().await;
    profile(&pool, "source", 40.0).await;
    let rule_id = rule_set(&pool).await;
    assert!(find_matches(&pool, "source", &rule_id, None, false).await.unwrap().is_empty());
    profile(&pool, "excluded", 16.0).await;
    assert!(find_matches(&pool, "source", &rule_id, None, false).await.unwrap().is_empty());
    profile(&pool, "candidate", 40.0).await;
    find_matches(&pool, "source", &rule_id, None, false).await.unwrap();
    let before = rows(&pool).await.len();
    assert_eq!(find_matches(&pool, "missing", &rule_id, None, false).await.err().as_deref(), Some("Profile not found"));
    assert_eq!(find_matches(&pool, "source", "missing", None, false).await.err().as_deref(), Some("Rule set not found"));
    assert_eq!(rows(&pool).await.len(), before);
}

#[tokio::test]
async fn identical_searches_keep_order_and_get_distinct_search_ids() {
    let pool = test_pool().await;
    for (id, age) in [("source", 40.0), ("best", 40.0), ("middle", 32.0), ("last", 22.0)] { profile(&pool, id, age).await; }
    let rule_id = rule_set(&pool).await;
    let first = find_matches(&pool, "source", &rule_id, None, false).await.unwrap();
    let first_rows = rows(&pool).await;
    let second = find_matches(&pool, "source", &rule_id, None, false).await.unwrap();
    assert_eq!(serde_json::to_value(&first).unwrap(), serde_json::to_value(&second).unwrap());
    assert!(first.windows(2).all(|pair| pair[0].score.unwrap() > pair[1].score.unwrap()));
    let stored = rows(&pool).await;
    assert_eq!(stored.len(), 6);
    let first_id = first_rows[0].get::<String, _>("search_id");
    let second_id = stored[3].get::<String, _>("search_id");
    uuid::Uuid::parse_str(&first_id).unwrap();
    uuid::Uuid::parse_str(&second_id).unwrap();
    assert_ne!(first_id, second_id);
    assert_candidates(&search_rows(&pool, &first_id).await, &first, "source", &rule_id, 1, None, None);
    assert_candidates(&search_rows(&pool, &second_id).await, &second, "source", &rule_id, 1, None, None);
}

#[tokio::test]
async fn searches_record_current_versions_without_rewriting_history() {
    let pool = test_pool().await;
    profile(&pool, "source", 40.0).await;
    profile(&pool, "candidate", 32.0).await;
    let rule_id = rule_set(&pool).await;
    let config_id = config(&pool, &rule_id).await;
    let first = find_matches(&pool, "source", &rule_id, None, false).await.unwrap();
    let first_id = rows(&pool).await[0].get::<String, _>("search_id");
    assert_eq!(MmRepository::save_rule_set_version(&pool, &rule_id, "Exposure rules", "", &rules(), None).await.unwrap(), 2);
    let updated = ScoringConfig { prior_score: 20.0, ..ScoringConfig::default() };
    assert_eq!(ScoringConfigRepo::save_version(&pool, &config_id, "Exposure config", "", &updated, None).await.unwrap(), 2);
    let second = find_matches(&pool, "source", &rule_id, None, false).await.unwrap();
    let stored = rows(&pool).await;
    assert_eq!(stored.len(), 2);
    let second_id = stored[1].get::<String, _>("search_id");
    assert_candidates(&search_rows(&pool, &first_id).await, &first, "source", &rule_id, 1, Some(&config_id), Some(1));
    assert_candidates(&search_rows(&pool, &second_id).await, &second, "source", &rule_id, 2, Some(&config_id), Some(2));
}

#[tokio::test]
async fn search_insert_failure_returns_error_and_rolls_back_the_batch() {
    let pool = test_pool().await;
    for (id, age) in [("source", 40.0), ("best", 40.0), ("middle", 32.0), ("last", 22.0)] { profile(&pool, id, age).await; }
    let rule_id = rule_set(&pool).await;
    find_matches(&pool, "source", &rule_id, None, false).await.unwrap();
    sqlx::query("CREATE TRIGGER fail_exposure BEFORE INSERT ON mm_exposure WHEN NEW.rank = 1 BEGIN SELECT RAISE(ABORT, 'injected exposure failure'); END")
        .execute(&pool).await.unwrap();
    let error = find_matches(&pool, "source", &rule_id, None, false).await.err().unwrap();
    assert!(error.contains("injected exposure failure"), "{error}");
    let stored = rows(&pool).await;
    assert_eq!(stored.len(), 3);
    assert!(stored.iter().all(|row| row.get::<String, _>("search_id") == stored[0].get::<String, _>("search_id")));
}

#[tokio::test]
async fn empty_summary_has_zero_counts_and_no_timestamp_bounds() {
    let pool = test_pool().await;
    let summary = ExposureRepo::summary(&pool).await.unwrap();
    assert_eq!((summary.exposures, summary.searches, summary.pairs_shown, summary.pairs_with_outcome), (0, 0, 0, 0));
    assert_eq!((summary.first_at, summary.last_at), (None, None));
}

#[tokio::test]
async fn hidden_only_summary_has_zero_counts_and_no_timestamp_bounds() {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    profile(&pool, "candidate", 30.0).await;
    historical_exposure(&pool, "hidden", "source", "candidate", false, AT).await;
    let summary = ExposureRepo::summary(&pool).await.unwrap();
    assert_eq!((summary.exposures, summary.searches, summary.pairs_shown, summary.pairs_with_outcome), (0, 0, 0, 0));
    assert_eq!((summary.first_at, summary.last_at), (None, None));
}

#[tokio::test]
async fn summary_counts_only_shown_rows_and_deduplicates_repeated_pairs() {
    let pool = test_pool().await;
    for id in ["source", "one", "two", "hidden"] { profile(&pool, id, 30.0).await; }
    historical_exposure(&pool, "hidden-before", "source", "hidden", false, BEFORE).await;
    historical_exposure(&pool, "first", "source", "one", true, AT).await;
    historical_exposure(&pool, "first", "source", "two", true, AT).await;
    historical_exposure(&pool, "second", "source", "one", true, AFTER).await;
    historical_exposure(&pool, "hidden-after", "source", "hidden", false, "2026-01-04T00:00:00+00:00").await;
    let summary = ExposureRepo::summary(&pool).await.unwrap();
    assert_eq!((summary.exposures, summary.searches, summary.pairs_shown, summary.pairs_with_outcome), (3, 2, 2, 0));
    assert_eq!(summary.first_at.as_deref(), Some(AT));
    assert_eq!(summary.last_at.as_deref(), Some(AFTER));
}

#[tokio::test]
async fn searches_in_both_directions_count_as_one_unordered_pair() {
    let pool = test_pool().await;
    profile(&pool, "one", 30.0).await;
    profile(&pool, "two", 30.0).await;
    historical_exposure(&pool, "first", "one", "two", true, BEFORE).await;
    historical_exposure(&pool, "second", "two", "one", true, AT).await;
    let summary = ExposureRepo::summary(&pool).await.unwrap();
    assert_eq!(summary.pairs_shown, 1, "both search directions represent one pair");
    assert_eq!((summary.exposures, summary.searches), (2, 2));
    assert_eq!(summary.pairs_with_outcome, 0);
    assert_eq!(summary.first_at.as_deref(), Some(BEFORE));
    assert_eq!(summary.last_at.as_deref(), Some(AT));
}

#[tokio::test]
async fn outcomes_for_searches_in_both_directions_count_once() {
    let pool = test_pool().await;
    profile(&pool, "one", 30.0).await;
    profile(&pool, "two", 30.0).await;
    let rule_id = rule_set(&pool).await;
    historical_exposure(&pool, "first", "one", "two", true, BEFORE).await;
    historical_exposure(&pool, "second", "two", "one", true, AT).await;
    let match_id = tracked(&pool, "two", "one", &rule_id, MatchStatus::Recommended).await;
    outcome_at(&pool, &match_id, AFTER).await;
    assert_eq!(ExposureRepo::summary(&pool).await.unwrap().pairs_with_outcome, 1);
}

#[tokio::test]
async fn later_outcomes_count_shown_pairs_once_and_ignore_unshown_pairs() {
    let pool = test_pool().await;
    for id in ["source", "shown", "hidden", "never-shown"] { profile(&pool, id, 30.0).await; }
    let rule_id = rule_set(&pool).await;
    historical_exposure(&pool, "first", "source", "shown", true, BEFORE).await;
    historical_exposure(&pool, "second", "source", "shown", true, AT).await;
    historical_exposure(&pool, "hidden", "source", "hidden", false, BEFORE).await;
    for candidate in ["shown", "hidden", "never-shown"] {
        let match_id = tracked(&pool, "source", candidate, &rule_id, MatchStatus::Recommended).await;
        outcome_at(&pool, &match_id, AFTER).await;
    }
    let summary = ExposureRepo::summary(&pool).await.unwrap();
    assert_eq!((summary.exposures, summary.searches, summary.pairs_shown, summary.pairs_with_outcome), (2, 2, 1, 1));
}

async fn assert_chronology(outcome_timestamp: &str, expected: i64) {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    profile(&pool, "candidate", 30.0).await;
    let rule_id = rule_set(&pool).await;
    let match_id = tracked(&pool, "source", "candidate", &rule_id, MatchStatus::Recommended).await;
    historical_exposure(&pool, "search", "source", "candidate", true, AT).await;
    outcome_at(&pool, &match_id, outcome_timestamp).await;
    assert_eq!(ExposureRepo::summary(&pool).await.unwrap().pairs_with_outcome, expected, "exposure at {AT}, outcome at {outcome_timestamp}");
}

#[tokio::test]
async fn exposure_before_outcome_qualifies() {
    assert_chronology(AFTER, 1).await;
}

#[tokio::test]
async fn exposure_after_outcome_does_not_qualify() {
    assert_chronology(BEFORE, 0).await;
}

#[tokio::test]
async fn exposure_simultaneous_with_outcome_does_not_qualify() {
    assert_chronology(AT, 0).await;
}

#[tokio::test]
async fn missing_outcome_event_and_later_status_or_updated_at_do_not_establish_chronology() {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    profile(&pool, "candidate", 30.0).await;
    let rule_id = rule_set(&pool).await;
    let match_id = tracked(&pool, "source", "candidate", &rule_id, MatchStatus::Recommended).await;
    outcome_at(&pool, &match_id, BEFORE).await;
    sqlx::query("DELETE FROM mm_match_events WHERE match_id = ? AND kind = 'outcome_recorded'").bind(&match_id).execute(&pool).await.unwrap();
    historical_exposure(&pool, "search", "source", "candidate", true, AT).await;
    MatchRepo::record_transition(&pool, &match_id, MatchStatus::Recommended, MatchStatus::Stopped, None, None, None).await.unwrap();
    sqlx::query("UPDATE mm_match_events SET at = ? WHERE match_id = ? AND kind = 'status_changed'").bind(AFTER).bind(&match_id).execute(&pool).await.unwrap();
    sqlx::query("UPDATE mm_matches SET updated_at = ? WHERE id = ?").bind(AFTER).bind(&match_id).execute(&pool).await.unwrap();
    assert_eq!(ExposureRepo::summary(&pool).await.unwrap().pairs_with_outcome, 0);
}

#[tokio::test]
async fn later_edits_do_not_make_an_earlier_outcome_qualify() {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    profile(&pool, "candidate", 30.0).await;
    let rule_id = rule_set(&pool).await;
    let match_id = tracked(&pool, "source", "candidate", &rule_id, MatchStatus::Recommended).await;
    outcome_at(&pool, &match_id, BEFORE).await;
    historical_exposure(&pool, "search", "source", "candidate", true, AT).await;
    MatchRepo::add_note(&pool, &match_id, "Later edit").await.unwrap();
    sqlx::query("UPDATE mm_matches SET updated_at = ? WHERE id = ?").bind(AFTER).bind(&match_id).execute(&pool).await.unwrap();
    assert_eq!(ExposureRepo::summary(&pool).await.unwrap().pairs_with_outcome, 0);
}

#[tokio::test]
async fn readiness_gate_counts_recorded_outcomes_at_zero_99_100_and_101() {
    for finished in [0, 99, 100, 101] {
        let pool = test_pool().await;
        profile(&pool, "source", 30.0).await;
        let rule_id = rule_set(&pool).await;
        for index in 0..finished {
            let candidate = format!("finished-{index}");
            profile(&pool, &candidate, 30.0).await;
            let match_id = tracked(&pool, "source", &candidate, &rule_id, MatchStatus::Stopped).await;
            MatchRepo::set_outcome(&pool, &match_id, if index % 2 == 0 { Outcome::Unknown } else { Outcome::FirstMeeting }).await.unwrap();
        }
        for (candidate, status) in [("recommended", MatchStatus::Recommended), ("approved", MatchStatus::Approved), ("closed-no-outcome", MatchStatus::Stopped)] {
            profile(&pool, candidate, 30.0).await;
            tracked(&pool, "source", candidate, &rule_id, status).await;
        }
        let readiness = ExposureRepo::ranking_readiness(&pool).await.unwrap();
        assert_eq!(readiness.minimum_finished, 100);
        assert_eq!(readiness.finished, finished);
        assert_eq!(readiness.with_outcome, finished);
        assert_eq!(readiness.ready, finished >= 100);
        let expected_state = if finished >= 100 {
            format!("{finished} finished matches, at or above the 100 needed to compare a learned ranking.")
        } else {
            format!("Not enough outcome data to compare a learned ranking: {finished} of 100 finished matches. Exposures are being recorded meanwhile, so the history keeps building.")
        };
        assert_eq!(readiness.state, expected_state);
        assert_eq!(readiness.outcome_counts.values().sum::<i64>(), finished);
        if finished == 0 { assert!(readiness.outcome_counts.is_empty()); } else {
            assert_eq!(readiness.outcome_counts.get("unknown"), Some(&((finished + 1) / 2)));
            assert_eq!(readiness.outcome_counts.get("first_meeting"), Some(&(finished / 2)));
        }
        assert_eq!(readiness.open_by_status, [("recommended".into(), 1), ("approved".into(), 1), ("stopped".into(), 1)].into_iter().collect());
        assert_eq!(readiness.with_snapshot, finished + 3);
        assert_eq!((readiness.exposures, readiness.pairs_shown, readiness.pairs_with_outcome), (0, 0, 0));
    }
}

async fn assert_mixed_snapshots(first_has_snapshot: bool) {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    let rule_id = rule_set(&pool).await;
    for (candidate, has_snapshot) in [("first", first_has_snapshot), ("second", !first_has_snapshot)] {
        profile(&pool, candidate, 30.0).await;
        let match_id = tracked(&pool, "source", candidate, &rule_id, MatchStatus::Recommended).await;
        if has_snapshot {
            MatchRepo::add_snapshot(&pool, &match_id, &rule_id, 1, None, None, "rescore", &json!({"eligible": true}), None).await.unwrap();
            assert_eq!(MatchRepo::snapshot_count(&pool, &match_id).await.unwrap(), 2);
        } else {
            sqlx::query("DELETE FROM mm_match_snapshots WHERE match_id = ?").bind(&match_id).execute(&pool).await.unwrap();
        }
    }
    assert_eq!(ExposureRepo::ranking_readiness(&pool).await.unwrap().with_snapshot, 1, "first_has_snapshot={first_has_snapshot}: count matches, not groups or snapshot rows");
}

#[tokio::test]
async fn mixed_snapshot_group_with_snapshot_first_counts_one_match() {
    assert_mixed_snapshots(true).await;
}

#[tokio::test]
async fn mixed_snapshot_group_without_snapshot_first_counts_one_match() {
    assert_mixed_snapshots(false).await;
}

#[tokio::test]
async fn multiple_snapshots_count_each_match_once() {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    let rule_id = rule_set(&pool).await;
    for (candidate, status) in [("open", MatchStatus::Recommended), ("finished", MatchStatus::Stopped)] {
        profile(&pool, candidate, 30.0).await;
        let match_id = tracked(&pool, "source", candidate, &rule_id, status).await;
        MatchRepo::add_snapshot(&pool, &match_id, &rule_id, 1, None, None, "rescore", &json!({"eligible": true}), None).await.unwrap();
        if status == MatchStatus::Stopped {
            MatchRepo::set_outcome(&pool, &match_id, Outcome::Unknown).await.unwrap();
        }
        assert_eq!(MatchRepo::snapshot_count(&pool, &match_id).await.unwrap(), 2);
    }
    assert_eq!(ExposureRepo::ranking_readiness(&pool).await.unwrap().with_snapshot, 2);
}

#[tokio::test]
async fn readiness_matches_exposure_summary_and_excludes_other_organizations() {
    let pool = test_pool().await;
    for id in ["source", "candidate", "foreign", "foreign-open"] { profile(&pool, id, 30.0).await; }
    let rule_id = rule_set(&pool).await;
    let own_match = tracked(&pool, "source", "candidate", &rule_id, MatchStatus::Recommended).await;
    historical_exposure(&pool, "shown", "source", "candidate", true, BEFORE).await;
    outcome_at(&pool, &own_match, AFTER).await;
    let foreign_match = tracked(&pool, "source", "foreign", &rule_id, MatchStatus::Approved).await;
    MatchRepo::set_outcome(&pool, &foreign_match, Outcome::FirstMeeting).await.unwrap();
    sqlx::query("UPDATE mm_matches SET org_id = 'other' WHERE id = ?").bind(&foreign_match).execute(&pool).await.unwrap();
    let foreign_open = tracked(&pool, "source", "foreign-open", &rule_id, MatchStatus::Approved).await;
    sqlx::query("UPDATE mm_matches SET org_id = 'other' WHERE id = ?").bind(&foreign_open).execute(&pool).await.unwrap();
    let summary = ExposureRepo::summary(&pool).await.unwrap();
    let readiness = ExposureRepo::ranking_readiness(&pool).await.unwrap();
    assert_eq!((readiness.exposures, readiness.pairs_shown, readiness.pairs_with_outcome), (summary.exposures, summary.pairs_shown, summary.pairs_with_outcome));
    assert_eq!((readiness.finished, readiness.with_outcome, readiness.with_snapshot), (1, 1, 1));
    assert_eq!(readiness.outcome_counts, [("unknown".into(), 1)].into_iter().collect());
    assert!(readiness.open_by_status.is_empty());
}

#[tokio::test]
async fn repeated_reporting_calls_do_not_change_database_rows() {
    let pool = test_pool().await;
    profile(&pool, "source", 30.0).await;
    profile(&pool, "candidate", 30.0).await;
    let rule_id = rule_set(&pool).await;
    let match_id = tracked(&pool, "source", "candidate", &rule_id, MatchStatus::Recommended).await;
    historical_exposure(&pool, "shown", "source", "candidate", true, BEFORE).await;
    outcome_at(&pool, &match_id, AFTER).await;
    let changes_before: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&pool).await.unwrap();
    let expected_summary = serde_json::to_value(ExposureRepo::summary(&pool).await.unwrap()).unwrap();
    let expected_readiness = serde_json::to_value(ExposureRepo::ranking_readiness(&pool).await.unwrap()).unwrap();
    for _ in 0..3 {
        assert_eq!(serde_json::to_value(ExposureRepo::summary(&pool).await.unwrap()).unwrap(), expected_summary);
        assert_eq!(serde_json::to_value(ExposureRepo::ranking_readiness(&pool).await.unwrap()).unwrap(), expected_readiness);
    }
    let changes_after: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&pool).await.unwrap();
    assert_eq!(changes_after, changes_before);
}
