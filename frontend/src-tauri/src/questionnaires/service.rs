//! Recording answers and turning a finished questionnaire into profile fields. Used by the app and by the Telegram bot.

use super::repo::{self, ResponseRow};
use crate::mm::repository::MmRepository;
use matchmaking_core::{apply_scores, score_answers, validate_answer_for, Instrument, Locale};
use sqlx::SqlitePool;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

pub async fn instrument_for(pool: &SqlitePool, r: &ResponseRow) -> Result<Instrument, String> {
    instrument_for_locale(pool, r, Locale::En).await
}

pub async fn instrument_for_locale(pool: &SqlitePool, r: &ResponseRow, locale: Locale) -> Result<Instrument, String> {
    let missing = locale.pick("The questionnaire this response belongs to no longer exists", "پرسشنامه‌ای که این پاسخ به آن تعلق دارد دیگر وجود ندارد");
    Ok(repo::get_instrument(pool, &r.instrument_key, Some(r.instrument_version)).await.map_err(err)?.ok_or(missing)?.instrument.localized(locale))
}

#[derive(Debug)]
pub struct AnswerOutcome {
    pub completed: bool,
    /// Fields that received a score, and whether the write was applied (a person's own entry is never overwritten).
    pub fields: Vec<(String, bool)>,
}

/// Save one answer. When it is the last one the scores are computed and written to the profile with questionnaire provenance.
pub async fn record_answer(pool: &SqlitePool, response_id: i64, item_id: &str, value: i32) -> Result<AnswerOutcome, String> {
    record_answer_for(pool, response_id, item_id, value, Locale::En).await
}

pub async fn record_answer_for(pool: &SqlitePool, response_id: i64, item_id: &str, value: i32, locale: Locale) -> Result<AnswerOutcome, String> {
    let mut r = repo::get_response(pool, response_id).await.map_err(err)?.ok_or(locale.pick("Response not found", "پاسخ پیدا نشد"))?;
    if r.status == "complete" {
        return Err(locale.pick("This questionnaire is already finished", "این پرسشنامه قبلاً تکمیل شده است").into());
    }
    let inst = instrument_for_locale(pool, &r, locale).await?;
    validate_answer_for(&inst, item_id, value, locale)?;
    r.answers.insert(item_id.to_string(), value);
    let scores = score_answers(&inst, &r.answers);
    if !scores.complete() {
        repo::save_answers(pool, response_id, &r.answers).await.map_err(err)?;
        return Ok(AnswerOutcome { completed: false, fields: vec![] });
    }
    let mut p = MmRepository::get_profile(pool, &r.profile_id).await.map_err(err)?.ok_or(locale.pick("Profile not found", "پروفایل پیدا نشد"))?;
    let fields = apply_scores(&mut p.profile, &inst, &scores);
    MmRepository::update_profile_data(pool, &p.profile).await.map_err(err)?;
    repo::complete(pool, response_id, &r.answers, &scores.subscales).await.map_err(err)?;
    // The audit log names the questionnaire, never the answers or scores.
    MmRepository::audit(pool, "questionnaire_completed", "profile", &r.profile_id, Some(&inst.key)).await;
    Ok(AnswerOutcome { completed: true, fields })
}
