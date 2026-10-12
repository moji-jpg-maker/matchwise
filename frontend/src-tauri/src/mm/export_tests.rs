//! Export tests: what the decision dataset contains, what it deliberately leaves out, and the counts the ranking gate
//! will read.

use super::export_commands;
use super::match_repo::MatchRepo;
use super::repository::MmRepository;
use matchmaking_core::{Interest, MatchStatus, Outcome, Profile, Provenance, Value};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn profile(pool: &SqlitePool, id: &str, name: &str, phone: &str) {
    let mut p = Profile::new(id);
    p.set("full_name", Value::Text(name.into()), Provenance::Matchmaker);
    p.set("phone", Value::Text(phone.into()), Provenance::Matchmaker);
    MmRepository::insert_profile(pool, &p).await.unwrap();
    MmRepository::registry(pool).await.unwrap();
}

/// A scorecard shaped like the real one, so the export reads the fields it will actually see.
fn card(overall: f64, ranking: f64, eligible: bool, dims: &[(&str, f64)]) -> serde_json::Value {
    json!({
        "rule_set": "default",
        "rule_set_version": 1,
        "eligible": eligible,
        "overall": overall,
        "confidence": 0.6,
        "ranking_score": ranking,
        "meets_threshold": true,
        "dimensions": dims.iter().map(|(k, s)| json!({"key": k, "score": s})).collect::<Vec<_>>(),
        "strengths": [],
        "concerns": [],
        "unknowns": [],
    })
}

async fn tracked_pair(pool: &SqlitePool, a: &str, b: &str, c: serde_json::Value) -> String {
    MmRepository::ensure_default_rule_set(pool).await.unwrap();
    let (set, _) = MmRepository::list_rule_sets(pool, false).await.unwrap().into_iter().next().unwrap();
    MatchRepo::create(pool, a, b, None, &set.id, set.current_version, None, None, MatchStatus::Recommended, &c).await.unwrap()
}

#[tokio::test]
async fn export_has_one_row_per_match_with_the_stored_scorecard_columns() {
    let pool = test_pool().await;
    profile(&pool, "p1", "Sara Karimi", "09120000000").await;
    profile(&pool, "p2", "Reza Ahmadi", "09120000001").await;
    tracked_pair(&pool, "p2", "p1", card(72.5, 63.5, true, &[("age", 80.0), ("location", 55.0)])).await;

    let doc = export_commands::build_export(&pool).await.unwrap();
    assert_eq!(doc.row_count, 1);
    assert_eq!(doc.with_outcome, 0);

    let lines: Vec<&str> = doc.csv.trim_end_matches('\n').split('\n').collect();
    assert_eq!(lines.len(), 2);
    // the pair is stored in a fixed order, so the export shows p1 first whatever order tracking used
    let first = lines[1].split(',').next().unwrap();
    assert_eq!(first.len(), 36, "first field is the match id (a uuid): {first}");
    assert!(lines[1].contains("p1|p2"), "pair key keeps the fixed match order");
    assert!(doc.columns.contains(&"dim_age".to_string()));
    assert!(doc.columns.contains(&"dim_location".to_string()));
    assert!(lines[1].ends_with(",80,55"), "dimension scores land in their own columns: {}", lines[1]);
    assert!(lines[1].contains("72.5") && lines[1].contains("63.5"));
}

#[tokio::test]
async fn export_carries_no_names_phones_or_notes() {
    let pool = test_pool().await;
    profile(&pool, "p1", "Sara Karimi", "09120000000").await;
    profile(&pool, "p2", "Reza Ahmadi", "09120000001").await;
    let id = tracked_pair(&pool, "p1", "p2", card(70.0, 60.0, true, &[("age", 70.0)])).await;
    MatchRepo::add_note(&pool, &id, "Sara asked about his family").await.unwrap();

    let doc = export_commands::build_export(&pool).await.unwrap();
    for forbidden in ["Sara", "Karimi", "Reza", "Ahmadi", "09120000000", "asked about"] {
        assert!(!doc.csv.contains(forbidden), "the export must not contain {forbidden}");
    }
}

#[tokio::test]
async fn a_finished_match_shows_its_outcome_and_an_overridden_one_is_marked() {
    let pool = test_pool().await;
    profile(&pool, "p1", "A", "1").await;
    profile(&pool, "p2", "B", "2").await;
    let id = tracked_pair(&pool, "p1", "p2", card(40.0, 45.0, false, &[("values", 40.0)])).await;

    // Walk the pair to a finished state. The rules exclude it, so approving needs the written override the lifecycle
    // demands -- and only the fact that an override happened is exported, never its text.
    MatchRepo::record_transition(&pool, &id, MatchStatus::Recommended, MatchStatus::Approved, None, Some("known each other for years"), None)
        .await
        .unwrap();
    MatchRepo::record_transition(&pool, &id, MatchStatus::Approved, MatchStatus::IntroductionProposed, None, None, None).await.unwrap();
    MatchRepo::record_responses(
        &pool,
        &id,
        MatchStatus::IntroductionProposed,
        Interest::Interested,
        Interest::Interested,
        Some(MatchStatus::BothInterested),
        "test",
    )
    .await
    .unwrap();
    MatchRepo::record_transition(&pool, &id, MatchStatus::BothInterested, MatchStatus::ContactExchanged, None, None, None).await.unwrap();
    MatchRepo::set_outcome(&pool, &id, Outcome::FirstMeeting).await.unwrap();
    MatchRepo::record_transition(&pool, &id, MatchStatus::ContactExchanged, MatchStatus::Stopped, None, None, None).await.unwrap();

    let doc = export_commands::build_export(&pool).await.unwrap();
    assert_eq!(doc.with_outcome, 1);
    let row = doc.csv.lines().nth(1).unwrap();
    assert!(row.contains("first_meeting"), "{row}");
    assert!(row.contains("true"), "the override is visible in the row: {row}");
    assert!(!row.contains("known each other"), "the override reason itself is not exported: {row}");

    let s = export_commands::summary(&pool).await.unwrap();
    assert_eq!(s.matches, 1);
    assert_eq!(s.finished, 1);
    assert_eq!(s.with_outcome, 1);
    assert_eq!(s.outcome_counts.get("first_meeting"), Some(&1));
    assert_eq!(s.with_snapshot, 1);
    assert_eq!(s.overridden, 1);
    assert_eq!(s.rule_set_versions.len(), 1, "one rule-set version was in force: {:?}", s.rule_set_versions);
    assert!(s.rule_set_versions[0].ends_with("@1"), "{:?}", s.rule_set_versions);
}

#[tokio::test]
async fn a_match_still_in_progress_is_counted_by_status_and_not_as_an_outcome() {
    let pool = test_pool().await;
    profile(&pool, "p1", "A", "1").await;
    profile(&pool, "p2", "B", "2").await;
    tracked_pair(&pool, "p1", "p2", card(60.0, 55.0, true, &[("age", 60.0)])).await;

    let s = export_commands::summary(&pool).await.unwrap();
    assert_eq!(s.matches, 1);
    assert_eq!(s.with_outcome, 0);
    assert!(s.outcome_counts.is_empty());
    assert_eq!(s.open_by_status.get("recommended"), Some(&1));
}

#[tokio::test]
async fn an_export_with_no_matches_is_just_the_header() {
    let pool = test_pool().await;
    let doc = export_commands::build_export(&pool).await.unwrap();
    assert_eq!(doc.row_count, 0);
    assert_eq!(doc.csv.lines().count(), 1, "header only");
    assert!(doc.columns.contains(&"match_id".to_string()));
}

#[tokio::test]
async fn a_dimension_scored_in_one_match_but_not_another_leaves_a_blank_not_a_zero() {
    let pool = test_pool().await;
    profile(&pool, "p1", "A", "1").await;
    profile(&pool, "p2", "B", "2").await;
    profile(&pool, "p3", "C", "3").await;
    tracked_pair(&pool, "p1", "p2", card(70.0, 65.0, true, &[("age", 70.0), ("values", 66.0)])).await;
    tracked_pair(&pool, "p1", "p3", card(50.0, 50.0, true, &[("age", 50.0)])).await;

    let doc = export_commands::build_export(&pool).await.unwrap();
    assert_eq!(doc.row_count, 2);
    let values_col = doc.columns.iter().position(|c| c == "dim_values").unwrap();
    let blanks = doc
        .csv
        .lines()
        .skip(1)
        .filter(|l| l.split(',').nth(values_col) == Some(""))
        .count();
    assert_eq!(blanks, 1, "only the pair that never scored 'values' is blank");
}

#[tokio::test]
async fn the_export_is_recorded_in_the_audit_log_without_its_content() {
    let pool = test_pool().await;
    profile(&pool, "p1", "Sara Karimi", "09120000000").await;
    profile(&pool, "p2", "Reza Ahmadi", "09120000001").await;
    tracked_pair(&pool, "p1", "p2", card(70.0, 60.0, true, &[("age", 70.0)])).await;
    assert_eq!(export_commands::export_audited(&pool).await.unwrap(), 0);

    export_commands::build_export(&pool).await.unwrap();
    assert_eq!(export_commands::export_audited(&pool).await.unwrap(), 1);

    let detail: Option<String> = sqlx::query_scalar("SELECT detail FROM mm_audit_log WHERE action = 'matchmaking_export' LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let detail = detail.unwrap();
    assert!(detail.contains("1 rows"));
    assert!(!detail.contains("Sara") && !detail.contains("09120000000"), "the audit log records counts, never content");
}
