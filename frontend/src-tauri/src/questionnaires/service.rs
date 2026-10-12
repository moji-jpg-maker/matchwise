//! Recording answers and turning a finished questionnaire into profile fields. Used by the app and by the Telegram bot.

use super::repo::{self, ResponseRow};
use crate::mm::repository::MmRepository;
use matchmaking_core::{apply_scores, score_answers, validate_answer, Instrument};
use sqlx::SqlitePool;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub async fn instrument_for(pool: &SqlitePool, r: &ResponseRow) -> Result<Instrument, String> {
    Ok(repo::get_instrument(pool, &r.instrument_key, Some(r.instrument_version)).await.map_err(err)?.ok_or("The questionnaire this response belongs to no longer exists")?.instrument)
}

#[derive(Debug)]
pub struct AnswerOutcome {
    pub completed: bool,
    /// Fields that received a score, and whether the write was applied (a person's own entry is never overwritten).
    pub fields: Vec<(String, bool)>,
}

/// Save one answer. When it is the last one the scores are computed and written to the profile with questionnaire provenance.
pub async fn record_answer(pool: &SqlitePool, response_id: i64, item_id: &str, value: i32) -> Result<AnswerOutcome, String> {
    let mut r = repo::get_response(pool, response_id).await.map_err(err)?.ok_or("Response not found")?;
    if r.status == "complete" {
        return Err("This questionnaire is already finished".into());
    }
    let inst = instrument_for(pool, &r).await?;
    validate_answer(&inst, item_id, value)?;
    r.answers.insert(item_id.to_string(), value);
    let scores = score_answers(&inst, &r.answers);
    if !scores.complete() {
        repo::save_answers(pool, response_id, &r.answers).await.map_err(err)?;
        return Ok(AnswerOutcome { completed: false, fields: vec![] });
    }
    let mut p = MmRepository::get_profile(pool, &r.profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    let fields = apply_scores(&mut p.profile, &inst, &scores);
    MmRepository::update_profile_data(pool, &p.profile).await.map_err(err)?;
    repo::complete(pool, response_id, &r.answers, &scores.subscales).await.map_err(err)?;
    // The audit log names the questionnaire, never the answers or scores.
    MmRepository::audit(pool, "questionnaire_completed", "profile", &r.profile_id, Some(&inst.key)).await;
    Ok(AnswerOutcome { completed: true, fields })
}
