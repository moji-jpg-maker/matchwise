//! Where the raw text of a bulk intake entry is kept.
//!
//! One row per profile created from a pasted corpus: the entry exactly as the matchmaker supplied it, plus the
//! batch it belongs to and the label it was given. It is deliberately not a profile field, so it is never scored,
//! never exported as field data, and a model can only ever read it through the redacted intake-note path. The row
//! is written in the same transaction as its profile, so an entry either becomes a profile with its source text or
//! nothing at all, and it is deleted with the profile.

use chrono::Utc;
use matchmaking_core::Profile;
use sqlx::{Row, SqlitePool};

use super::repository::{MmRepository, ORG};

/// One stored intake note.
#[derive(Debug, Clone)]
pub struct IntakeNote {
    pub profile_id: String,
    pub batch_id: String,
    pub note_text: String,
    pub label: String,
    pub created_at: String,
}

pub struct IntakeRepo;

impl IntakeRepo {
    /// Create a profile and keep the raw entry it was created from, atomically. Nothing is parsed here: the note
    /// is stored as written, and how much of it a model may read is decided later, per call, by the AI gate.
    pub async fn insert_profile_with_note(
        pool: &SqlitePool,
        profile: &Profile,
        batch_id: &str,
        note_text: &str,
        label: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        MmRepository::insert_profile(&mut *tx, profile).await?;
        sqlx::query("INSERT INTO mm_intake_notes (profile_id, org_id, batch_id, note_text, label, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&profile.id)
            .bind(ORG)
            .bind(batch_id)
            .bind(note_text)
            .bind(label)
            .bind(Utc::now().to_rfc3339())
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }

    /// The raw text a profile was created from, if it was created by an intake run. Read by the extraction path
    /// and shown read-only on the profile; it never becomes a field.
    pub async fn note_for_profile(
        pool: &SqlitePool,
        profile_id: &str,
    ) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(
            "SELECT note_text FROM mm_intake_notes WHERE profile_id = ? AND org_id = ?",
        )
        .bind(profile_id)
        .bind(ORG)
        .fetch_optional(pool)
        .await
    }

    /// Every note written by one batch, in creation order, for the run report.
    pub async fn notes_for_batch(
        pool: &SqlitePool,
        batch_id: &str,
    ) -> Result<Vec<IntakeNote>, sqlx::Error> {
        let rows = sqlx::query("SELECT profile_id, batch_id, note_text, label, created_at FROM mm_intake_notes WHERE org_id = ? AND batch_id = ? ORDER BY created_at, profile_id")
            .bind(ORG)
            .bind(batch_id)
            .fetch_all(pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok(IntakeNote {
                    profile_id: r.get("profile_id"),
                    batch_id: r.get("batch_id"),
                    note_text: r.get("note_text"),
                    label: r.get("label"),
                    created_at: r.get("created_at"),
                })
            })
            .collect()
    }

    /// Drop the note of one profile. `delete_profile` calls this itself; it is public for the intake commands,
    /// which discard an entry whose profile could not be created.
    pub async fn delete_note(pool: &SqlitePool, profile_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM mm_intake_notes WHERE profile_id = ?")
            .bind(profile_id)
            .execute(pool)
            .await?;
        Ok(())
    }
}
