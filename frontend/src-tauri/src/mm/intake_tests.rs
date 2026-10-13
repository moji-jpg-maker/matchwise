//! Storage tests for bulk-intake notes: the row is written atomically with its profile, is read back only on the
//! paths that qualify it, and goes away with the person.

use super::intake_repo::IntakeRepo;
use super::repository::MmRepository;
use matchmaking_core::{Profile, Provenance, Value};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;

/// The app has no `PRAGMA foreign_keys` turned on and neither do the other suites, so the explicit-delete path is
/// what has to hold. Kept off deliberately, to test the code as it actually runs.
async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

fn empty_profile(id: &str) -> Profile {
    let mut p = Profile::new(id);
    assert!(p.set("age", Value::Num(31.0), Provenance::Matchmaker));
    p
}

async fn note_count(pool: &SqlitePool, batch: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM mm_intake_notes WHERE batch_id = ?")
        .bind(batch)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_profile_and_its_note_are_written_together_and_read_back() {
    let pool = test_pool().await;
    let mut p = Profile::new("p1");
    p.set("age", Value::Num(31.0), Provenance::Matchmaker);
    p.set("full_name", Value::Text("Sara".into()), Provenance::User);
    IntakeRepo::insert_profile_with_note(&pool, &p, "b1", "raw text about Sara", "Entry 1")
        .await
        .unwrap();

    let stored = MmRepository::get_profile(&pool, "p1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.profile.get("age"), Some(&Value::Num(31.0)));
    let note = IntakeRepo::note_for_profile(&pool, "p1").await.unwrap();
    assert_eq!(note.as_deref(), Some("raw text about Sara"));

    let notes = IntakeRepo::notes_for_batch(&pool, "b1").await.unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].label, "Entry 1");
    assert_eq!(notes[0].profile_id, "p1");
    assert!(!notes[0].created_at.is_empty());
}

#[tokio::test]
async fn the_raw_text_is_not_a_profile_field_and_never_enters_its_data() {
    let pool = test_pool().await;
    let p = empty_profile("p1");
    IntakeRepo::insert_profile_with_note(
        &pool,
        &p,
        "b1",
        "Sara Karimi, +98 912 345 6789",
        "Entry 1",
    )
    .await
    .unwrap();

    let data: String = sqlx::query_scalar("SELECT data FROM mm_profiles WHERE id = 'p1'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!data.contains("Karimi"));
    assert!(!data.contains("+98"));
    let stored = MmRepository::get_profile(&pool, "p1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.profile.fields.len(),
        1,
        "the note is stored beside the profile, not inside it"
    );
}

#[tokio::test]
async fn a_batch_is_scoped_to_its_own_notes_and_a_second_batch_does_not_see_them() {
    let pool = test_pool().await;
    IntakeRepo::insert_profile_with_note(&pool, &empty_profile("p1"), "b1", "one", "Entry 1")
        .await
        .unwrap();
    IntakeRepo::insert_profile_with_note(&pool, &empty_profile("p2"), "b1", "two", "Entry 2")
        .await
        .unwrap();
    IntakeRepo::insert_profile_with_note(&pool, &empty_profile("p3"), "b2", "three", "Entry 1")
        .await
        .unwrap();

    let first = IntakeRepo::notes_for_batch(&pool, "b1").await.unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(
        first
            .iter()
            .map(|n| n.note_text.as_str())
            .collect::<Vec<_>>(),
        vec!["one", "two"],
        "creation order"
    );
    assert_eq!(note_count(&pool, "b2").await, 1);
    assert!(IntakeRepo::note_for_profile(&pool, "p3")
        .await
        .unwrap()
        .is_some());
}

/// One note per profile: a second write for the same profile must not silently answer with a stale note.
#[tokio::test]
async fn a_profile_cannot_carry_two_intake_notes() {
    let pool = test_pool().await;
    IntakeRepo::insert_profile_with_note(&pool, &empty_profile("p1"), "b1", "first", "Entry 1")
        .await
        .unwrap();
    let again = IntakeRepo::insert_profile_with_note(
        &pool,
        &empty_profile("p1"),
        "b1",
        "second",
        "Entry 1",
    )
    .await;
    assert!(again.is_err());
    assert_eq!(
        IntakeRepo::note_for_profile(&pool, "p1")
            .await
            .unwrap()
            .as_deref(),
        Some("first")
    );
    assert_eq!(note_count(&pool, "b1").await, 1);
}

/// A profile that cannot be inserted must leave no note behind, and vice versa: the entry is all or nothing.
#[tokio::test]
async fn a_failed_profile_insert_leaves_no_note() {
    let pool = test_pool().await;
    IntakeRepo::insert_profile_with_note(&pool, &empty_profile("dup"), "b1", "kept", "Entry 1")
        .await
        .unwrap();
    let again =
        IntakeRepo::insert_profile_with_note(&pool, &empty_profile("dup"), "b2", "lost", "Entry 1")
            .await;
    assert!(again.is_err(), "the profile id is already taken");
    assert_eq!(
        note_count(&pool, "b2").await,
        0,
        "the note of the failed entry was rolled back with it"
    );
    assert_eq!(
        IntakeRepo::note_for_profile(&pool, "dup")
            .await
            .unwrap()
            .as_deref(),
        Some("kept")
    );
}

#[tokio::test]
async fn deleting_a_profile_deletes_its_note() {
    let pool = test_pool().await;
    IntakeRepo::insert_profile_with_note(&pool, &empty_profile("p1"), "b1", "raw one", "Entry 1")
        .await
        .unwrap();
    IntakeRepo::insert_profile_with_note(&pool, &empty_profile("p2"), "b1", "raw two", "Entry 2")
        .await
        .unwrap();

    MmRepository::delete_profile(&pool, "p1").await.unwrap();
    assert!(IntakeRepo::note_for_profile(&pool, "p1")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        note_count(&pool, "b1").await,
        1,
        "the other entry is untouched"
    );
    assert_eq!(
        IntakeRepo::note_for_profile(&pool, "p2")
            .await
            .unwrap()
            .as_deref(),
        Some("raw two")
    );

    MmRepository::delete_profile(&pool, "p2").await.unwrap();
    assert_eq!(note_count(&pool, "b1").await, 0);
    let all: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_intake_notes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(all, 0);
}

/// Nothing about a note may appear in the audit trail: only field names and actions are ever written there.
#[tokio::test]
async fn no_note_text_or_note_lookup_reaches_the_audit_log() {
    let pool = test_pool().await;
    IntakeRepo::insert_profile_with_note(
        &pool,
        &empty_profile("p1"),
        "b1",
        "Sara Karimi, +98 912 345 6789",
        "Entry 1",
    )
    .await
    .unwrap();
    MmRepository::audit(&pool, "create", "profile", "p1", Some("age")).await;
    MmRepository::delete_profile(&pool, "p1").await.unwrap();

    let details: Vec<Option<String>> = sqlx::query_scalar("SELECT detail FROM mm_audit_log")
        .fetch_all(&pool)
        .await
        .unwrap();
    for d in details {
        let d = d.unwrap_or_default();
        assert!(
            !d.contains("Karimi") && !d.contains("+98") && !d.contains("mm_intake_notes"),
            "audit detail held something it must not: {d}"
        );
    }
}
