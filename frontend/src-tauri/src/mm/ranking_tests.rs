//! Ranking v1 comparison tests: which matches make it into the set, what the comparison reports, and the one property
//! the whole harness rests on -- that a set crafted so one ordering tracks the outcomes makes that ordering win.

use super::ranking_commands::{build_comparison, gate_note, CLASSIFICATION};
use super::ranking_repo::{ExclusionCounts, RankingRepo};
use super::repository::MmRepository;
use super::match_repo::MatchRepo;
use matchmaking_core::{MatchStatus, Outcome, Profile, Provenance, Value, RANKING_MINIMUM};
use serde_json::json;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn profile(pool: &SqlitePool, id: &str) {
    let mut person = Profile::new(id);
    person.set("full_name", Value::Text(format!("Person {id}")), Provenance::Matchmaker);
    MmRepository::insert_profile(pool, &person).await.unwrap();
}

/// A scorecard carrying exactly the two numbers under comparison. The pair of them is set by hand because the question
/// is which ordering the outcomes follow, and the stored card is what the comparison reads -- it is not recomputed.
fn card(overall: f64, ranking_score: f64) -> serde_json::Value {
    json!({"overall": overall, "confidence": 0.6, "ranking_score": ranking_score, "eligible": true, "dimensions": []})
}

/// Track a pair, record its outcome, and return the match id.
async fn decided(pool: &SqlitePool, a: &str, b: &str, c: serde_json::Value, outcome: Outcome) -> String {
    let id = tracked(pool, a, b, c).await;
    MatchRepo::set_outcome(pool, &id, outcome).await.unwrap();
    id
}

async fn tracked(pool: &SqlitePool, a: &str, b: &str, c: serde_json::Value) -> String {
    MmRepository::ensure_default_rule_set(pool).await.unwrap();
    let (set, _) = MmRepository::list_rule_sets(pool, false).await.unwrap().into_iter().next().unwrap();
    MatchRepo::create(pool, a, b, None, &set.id, set.current_version, None, None, MatchStatus::Recommended, &c).await.unwrap()
}

#[tokio::test]
async fn a_match_with_no_outcome_an_unknown_outcome_or_no_snapshot_is_left_out() {
    let pool = test_pool().await;
    for id in ["p1", "p2", "p3", "p4", "p5"] {
        profile(&pool, id).await;
    }
    // In flight: tracked, no outcome yet.
    tracked(&pool, "p1", "p2", card(70.0, 60.0)).await;
    // Recorded but directionless.
    decided(&pool, "p1", "p3", card(70.0, 60.0), Outcome::Unknown).await;
    // Decided and scored: the only row that can be compared.
    decided(&pool, "p1", "p4", card(70.0, 60.0), Outcome::FirstMeeting).await;
    // Decided but nothing was ever stored to compare the outcome against.
    let no_snapshot = decided(&pool, "p2", "p5", card(50.0, 50.0), Outcome::Stopped).await;
    sqlx::query("DELETE FROM mm_match_snapshots WHERE match_id = ?").bind(&no_snapshot).execute(&pool).await.unwrap();

    let (rows, excluded) = RankingRepo::comparison_rows(&pool).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].overall, Some(70.0));
    assert!(rows[0].progressed);
    assert_eq!(excluded, ExclusionCounts { no_outcome: 1, unknown_outcome: 1, no_snapshot: 1, no_score: 0 });
    assert_eq!(excluded.total(), 3);

    // The readiness view still counts every one of them: the comparison narrows the set, the gate does not.
    let readiness = super::exposure_repo::ExposureRepo::ranking_readiness(&pool).await.unwrap();
    assert_eq!(readiness.finished, 3, "every recorded outcome counts for the gate");
    assert_eq!(readiness.with_snapshot, 3, "and every match whose snapshot was not deleted");

    // The command reports the same set and the exclusions that produced it.
    let comparison = build_comparison(&pool).await.unwrap();
    assert_eq!(comparison.matches_compared, 1);
    assert_eq!(comparison.matches_total, 4, "kept plus excluded is every match on record");
    assert_eq!(comparison.progressed, 1);
    assert_eq!(comparison.not_progressed, 0);
    assert!(comparison.ready == false);
}

/// The acceptance property, in both directions: when progressers carry the higher raw overall, the candidate ordering
/// agrees with the outcomes and the adjusted one does not; swap the two score columns and the verdict follows the
/// swap. Without this the comparison would be reporting noise in both columns.
#[tokio::test]
async fn the_ordering_that_tracks_the_outcomes_is_the_one_that_wins() {
    let pool = test_pool().await;
    for id in ["p1", "p2", "p3", "p4"] {
        profile(&pool, id).await;
    }
    // Progressers score high raw but low adjusted (thin evidence); the pairs that stopped are the other way round.
    decided(&pool, "p1", "p2", card(90.0, 40.0), Outcome::FirstMeeting).await;
    decided(&pool, "p1", "p3", card(80.0, 30.0), Outcome::Continued).await;
    decided(&pool, "p1", "p4", card(20.0, 90.0), Outcome::NotInterested).await;
    decided(&pool, "p2", "p3", card(10.0, 80.0), Outcome::Stopped).await;

    let c = build_comparison(&pool).await.unwrap();
    assert_eq!((c.progressed, c.not_progressed), (2, 2));
    assert_eq!(c.candidate.concordance, Some(1.0), "raw overall follows the outcomes");
    assert_eq!(c.baseline.concordance, Some(0.0), "the adjusted score inverts them");
    assert!(c.candidate.concordance > c.baseline.concordance);
    assert_eq!((c.candidate.concordant, c.candidate.discordant), (4, 0));
    assert_eq!((c.baseline.concordant, c.baseline.discordant), (0, 4));
}

#[tokio::test]
async fn the_same_set_with_the_two_score_columns_swapped_inverts_the_verdict() {
    let pool = test_pool().await;
    for id in ["p1", "p2", "p3", "p4"] {
        profile(&pool, id).await;
    }
    // Now the adjusted score is the one that follows the outcomes.
    decided(&pool, "p1", "p2", card(40.0, 90.0), Outcome::FirstMeeting).await;
    decided(&pool, "p1", "p3", card(30.0, 80.0), Outcome::Married).await;
    decided(&pool, "p1", "p4", card(90.0, 20.0), Outcome::NotInterested).await;
    decided(&pool, "p2", "p3", card(80.0, 10.0), Outcome::Stopped).await;

    let c = build_comparison(&pool).await.unwrap();
    assert_eq!(c.baseline.concordance, Some(1.0));
    assert_eq!(c.candidate.concordance, Some(0.0));
    assert!(c.baseline.concordance > c.candidate.concordance);
    assert_eq!((c.baseline.top_k, c.candidate.top_k), (2, 0), "the top two by the winning score are both progressers");
}

#[tokio::test]
async fn an_empty_history_compares_to_nothing_rather_than_being_refused() {
    let pool = test_pool().await;
    let c = build_comparison(&pool).await.unwrap();
    assert_eq!(c.matches_total, 0);
    assert_eq!(c.matches_compared, 0);
    assert_eq!(c.progressed, 0);
    assert_eq!(c.baseline.concordance, None, "no comparisons is not a rate of zero");
    assert_eq!(c.candidate.concordance, None);
    assert!(!c.ready);
    assert!(c.gate_note.contains("Shown for inspection only"));
}

/// The comparison is shown below the gate with the wording attached, and the wording changes once the gate is reached.
#[tokio::test]
async fn the_comparison_is_marked_for_inspection_only_below_the_gate() {
    let below = gate_note(RANKING_MINIMUM - 1);
    assert!(below.contains("Below the gate"));
    assert!(below.contains(&format!("{} of {}", RANKING_MINIMUM - 1, RANKING_MINIMUM)));
    assert!(below.contains("for inspection only"));
    assert!(below.contains("still not used to order anything"));

    let above = gate_note(RANKING_MINIMUM);
    assert!(!above.contains("inspection only"));
    assert!(above.contains("finding"));
    assert_ne!(below, above);
}

/// A small set is still compared rather than hidden: below the gate the figures are shown, with the warning as prose
/// next to them. And the label rule is reported by the command itself, not left for the page to restate.
#[tokio::test]
async fn a_below_the_gate_set_is_still_compared_and_says_what_it_counted_as_progress() {
    let pool = test_pool().await;
    for id in ["p1", "p2", "p3"] {
        profile(&pool, id).await;
    }
    decided(&pool, "p1", "p2", card(80.0, 70.0), Outcome::Interested).await;
    decided(&pool, "p1", "p3", card(20.0, 30.0), Outcome::NotInterested).await;

    let c = build_comparison(&pool).await.unwrap();
    assert!(!c.ready);
    assert_eq!(c.matches_compared, 2);
    assert_eq!(c.candidate.concordance, Some(1.0), "the small set is still read, not withheld");
    assert_eq!(c.classification, CLASSIFICATION);
    assert!(c.classification.contains("unknown outcome"));
    assert_eq!(c.enforced, "ranking_score", "the page says which ordering orders searches today");
    assert_eq!(c.minimum_finished, RANKING_MINIMUM);
}

/// The payload is consumed by a TypeScript interface, so the field names and nullability are pinned here rather than
/// left to be discovered by a page that silently reads `undefined`.
#[tokio::test]
async fn the_payload_matches_the_typescript_shape_field_for_field() {
    let pool = test_pool().await;
    profile(&pool, "p1").await;
    profile(&pool, "p2").await;
    decided(&pool, "p1", "p2", card(80.0, 70.0), Outcome::FirstMeeting).await;

    let value = serde_json::to_value(build_comparison(&pool).await.unwrap()).unwrap();
    let mut keys: Vec<&str> = value.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "baseline", "candidate", "classification", "enforced", "excluded", "gate_note", "matches_compared",
            "matches_total", "minimum_finished", "not_progressed", "progressed", "ready",
        ]
    );
    let mut excluded: Vec<&str> = value["excluded"].as_object().unwrap().keys().map(|k| k.as_str()).collect();
    excluded.sort();
    assert_eq!(excluded, vec!["no_outcome", "no_score", "no_snapshot", "unknown_outcome"]);
    // Both orderings carry the same shape, and an absent rate is `null` -- never 0, which would read as "always
    // wrong", and never NaN, which would not serialise into JSON at all.
    for ordering in ["baseline", "candidate"] {
        let mut fields: Vec<&str> = value[ordering].as_object().unwrap().keys().map(|k| k.as_str()).collect();
        fields.sort();
        assert_eq!(
            fields,
            vec![
                "comparable_pairs", "concordance", "concordant", "discordant", "not_progressed", "progressed", "tied",
                "top_k", "top_k_rate", "usable_rows",
            ],
            "{ordering}"
        );
        // One match, one label: nothing to compare against, so the rate is null rather than 0 or 1. The one row is
        // still placed (R-precision over k = 1), and it did progress, so that rate is 1.
        assert_eq!(value[ordering]["concordance"], json!(null), "one row only, nothing to compare: {ordering}");
        assert_eq!(value[ordering]["comparable_pairs"], json!(0));
        assert_eq!(value[ordering]["top_k_rate"], json!(1.0), "{ordering}");
    }
}

/// Reading the comparison is a read: repeating it must not add a row anywhere, the same guarantee the other report
/// commands make.
#[tokio::test]
async fn comparing_repeatedly_writes_nothing() {
    let pool = test_pool().await;
    for id in ["p1", "p2", "p3"] {
        profile(&pool, id).await;
    }
    decided(&pool, "p1", "p2", card(80.0, 70.0), Outcome::FirstMeeting).await;
    decided(&pool, "p1", "p3", card(20.0, 30.0), Outcome::Stopped).await;

    let before: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&pool).await.unwrap();
    let expected = serde_json::to_value(build_comparison(&pool).await.unwrap()).unwrap();
    for _ in 0..3 {
        assert_eq!(serde_json::to_value(build_comparison(&pool).await.unwrap()).unwrap(), expected);
    }
    let after: i64 = sqlx::query_scalar("SELECT total_changes()").fetch_one(&pool).await.unwrap();
    assert_eq!(after, before);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_audit_log").fetch_one(&pool).await.unwrap();
    assert_eq!(audit, 0, "an aggregate read is not an export: no rows and no content leave the database");
}
