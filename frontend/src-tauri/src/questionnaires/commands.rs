//! Tauri commands for the questionnaire and evidence registry.

use super::repo::{self, ResponseRow};
use super::service;
use crate::mm::repository::MmRepository;
use crate::state::AppState;
use crate::telegram::{notify, repo as tg_repo};
use matchmaking_core::{missing_fields, validate_instrument, EvidenceLevel, Instrument};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::BTreeMap;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Debug, Serialize)]
pub struct InstrumentView {
    pub instrument: Instrument,
    pub archived: bool,
    pub created_at: String,
    pub evidence_label: String,
    /// Older versions of the same questionnaire (kept so earlier responses stay readable).
    pub is_latest: bool,
}

#[tauri::command]
pub async fn q_list_instruments(state: State<'_, AppState>, include_archived: Option<bool>) -> Result<Vec<InstrumentView>, String> {
    let pool = state.db_manager.pool();
    repo::ensure_builtin(pool).await.map_err(err)?;
    let rows = repo::list_instruments(pool, include_archived.unwrap_or(false)).await.map_err(err)?;
    let mut seen = std::collections::BTreeSet::new();
    Ok(rows
        .into_iter()
        .map(|r| {
            let latest = seen.insert(r.instrument.key.clone());
            InstrumentView { evidence_label: r.instrument.evidence.level.label().to_string(), instrument: r.instrument, archived: r.archived, created_at: r.created_at, is_latest: latest }
        })
        .collect())
}

/// Create a custom questionnaire, or a new version of an existing custom one. Instruments shipped with the app cannot be edited,
/// and a custom one can never claim to be a validated instrument.
#[tauri::command]
pub async fn q_save_instrument(state: State<'_, AppState>, instrument: Instrument) -> Result<InstrumentView, String> {
    let pool = state.db_manager.pool();
    let mut i = instrument;
    i.kind = "custom".into();
    if i.evidence.level == EvidenceLevel::ValidatedInstrument {
        return Err("Only questionnaires shipped with Matchwise are marked as validated. If you changed a published questionnaire, mark yours as 'adapted' and cite the original.".into());
    }
    if i.evidence.level == EvidenceLevel::AdaptedInstrument && i.evidence.sources.iter().all(|s| s.citation.trim().is_empty()) {
        return Err("An adapted questionnaire must cite the instrument it was adapted from".into());
    }
    let existing = repo::get_instrument(pool, &i.key, None).await.map_err(err)?;
    i.version = match &existing {
        Some(e) if e.instrument.kind == "standard" => return Err("This questionnaire ships with Matchwise and cannot be changed. Create your own with a different key.".into()),
        Some(e) => e.instrument.version + 1,
        None => 1,
    };
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let issues = validate_instrument(&i, &reg);
    if !issues.is_empty() {
        return Err(format!("Not saved. {}", issues.into_iter().take(5).collect::<Vec<_>>().join("; ")));
    }
    // Scores need numeric profile fields; create the ones that do not exist yet (as sensitive: they are psychological information).
    for def in missing_fields(&i, &reg) {
        let order = MmRepository::next_field_order(pool).await.map_err(err)?;
        MmRepository::save_field(pool, &def, order).await.map_err(err)?;
    }
    repo::insert_instrument(pool, &i).await.map_err(err)?;
    MmRepository::audit(pool, "questionnaire_saved", "instrument", &i.key, Some(&format!("v{}", i.version))).await;
    Ok(InstrumentView { evidence_label: i.evidence.level.label().to_string(), instrument: i, archived: false, created_at: chrono::Utc::now().to_rfc3339(), is_latest: true })
}

#[tauri::command]
pub async fn q_archive_instrument(state: State<'_, AppState>, key: String, archived: bool) -> Result<(), String> {
    let pool = state.db_manager.pool();
    if repo::set_archived(pool, &key, archived).await.map_err(err)? == 0 {
        return Err("Questionnaire not found".into());
    }
    MmRepository::audit(pool, if archived { "questionnaire_archived" } else { "questionnaire_restored" }, "instrument", &key, None).await;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ScoreView {
    pub subscale: String,
    pub label: String,
    pub field: String,
    pub value: f64,
}

#[derive(Debug, Serialize)]
pub struct ResponseView {
    pub id: i64,
    pub instrument_key: String,
    pub instrument_name: String,
    pub instrument_version: u32,
    pub dimension: String,
    pub evidence_label: String,
    pub limitations: String,
    pub interpretation: String,
    pub status: String,
    pub answered: usize,
    pub total: usize,
    pub assigned_at: String,
    pub completed_at: Option<String>,
    pub scores: Vec<ScoreView>,
}

async fn view(pool: &SqlitePool, r: &ResponseRow) -> Result<ResponseView, String> {
    let inst = service::instrument_for(pool, r).await?;
    let scores = r
        .scores
        .as_ref()
        .map(|m| inst.subscales.iter().filter_map(|s| m.get(&s.key).map(|v| ScoreView { subscale: s.key.clone(), label: s.label.clone(), field: s.field.clone(), value: *v })).collect())
        .unwrap_or_default();
    Ok(ResponseView {
        id: r.id,
        instrument_key: inst.key.clone(),
        instrument_name: inst.name.clone(),
        instrument_version: inst.version,
        dimension: inst.dimension.clone(),
        evidence_label: inst.evidence.level.label().to_string(),
        limitations: inst.evidence.limitations.clone(),
        interpretation: inst.evidence.interpretation.clone(),
        status: r.status.clone(),
        answered: r.answers.len(),
        total: inst.items.len(),
        assigned_at: r.assigned_at.clone(),
        completed_at: r.completed_at.clone(),
        scores,
    })
}

#[tauri::command]
pub async fn q_profile_responses(state: State<'_, AppState>, profile_id: String) -> Result<Vec<ResponseView>, String> {
    let pool = state.db_manager.pool();
    let mut out = vec![];
    for r in repo::responses_for_profile(pool, &profile_id).await.map_err(err)? {
        out.push(view(pool, &r).await?);
    }
    Ok(out)
}

#[derive(Debug, Serialize)]
pub struct AssignResult {
    pub response: ResponseView,
    /// The person was told on Telegram.
    pub notified: bool,
}

/// Give a person a questionnaire to complete (in the app with you, or on Telegram). Latest version of the questionnaire is used.
#[tauri::command]
pub async fn q_assign(state: State<'_, AppState>, profile_id: String, instrument_key: String) -> Result<AssignResult, String> {
    let pool = state.db_manager.pool();
    MmRepository::get_profile(pool, &profile_id).await.map_err(err)?.ok_or("Profile not found")?;
    let inst = repo::get_instrument(pool, &instrument_key, None).await.map_err(err)?.ok_or("Questionnaire not found")?;
    if inst.archived {
        return Err("This questionnaire is archived".into());
    }
    if repo::open_responses(pool, &profile_id).await.map_err(err)?.iter().any(|r| r.instrument_key == instrument_key) {
        return Err("This person already has this questionnaire open".into());
    }
    let id = repo::assign(pool, &profile_id, &instrument_key, inst.instrument.version).await.map_err(err)?;
    MmRepository::audit(pool, "questionnaire_assigned", "profile", &profile_id, Some(&instrument_key)).await;
    let mut notified = false;
    if notify::reachable_link(pool, &profile_id).await?.is_some() {
        let text = format!("Your matchmaker would like you to answer a short questionnaire: <b>{}</b>. Send /questionnaires to start; you can stop and continue later.", crate::telegram::render::esc(&inst.instrument.name));
        tg_repo::enqueue(pool, &profile_id, "questionnaire_assigned", &text, None, None).await.map_err(err)?;
        notified = true;
    }
    let r = repo::get_response(pool, id).await.map_err(err)?.ok_or("Response vanished")?;
    Ok(AssignResult { response: view(pool, &r).await?, notified })
}

/// The questions of an open response with the answers so far (for entering answers in the app).
#[derive(Debug, Serialize)]
pub struct ResponseDetail {
    pub response: ResponseView,
    pub instrument: Instrument,
    pub answers: BTreeMap<String, i32>,
}

#[tauri::command]
pub async fn q_get_response(state: State<'_, AppState>, id: i64) -> Result<ResponseDetail, String> {
    let pool = state.db_manager.pool();
    let r = repo::get_response(pool, id).await.map_err(err)?.ok_or("Response not found")?;
    let instrument = service::instrument_for(pool, &r).await?;
    Ok(ResponseDetail { response: view(pool, &r).await?, instrument, answers: r.answers })
}

#[derive(Debug, Serialize)]
pub struct AnswerResult {
    pub response: ResponseView,
    pub completed: bool,
    /// Fields that could not be updated because a person's own entry outranks a questionnaire score.
    pub not_applied: Vec<String>,
}

/// Record one answer on a person's behalf (for example during an interview). The last answer completes the questionnaire.
#[tauri::command]
pub async fn q_answer(state: State<'_, AppState>, id: i64, item_id: String, value: i32) -> Result<AnswerResult, String> {
    let pool = state.db_manager.pool();
    let out = service::record_answer(pool, id, &item_id, value).await?;
    let r = repo::get_response(pool, id).await.map_err(err)?.ok_or("Response not found")?;
    Ok(AnswerResult { response: view(pool, &r).await?, completed: out.completed, not_applied: out.fields.into_iter().filter(|(_, ok)| !ok).map(|(f, _)| f).collect() })
}

/// Delete a response. With `clear_scores` the profile fields this questionnaire filled in (and that still carry
/// questionnaire provenance) are removed too.
#[tauri::command]
pub async fn q_delete_response(state: State<'_, AppState>, id: i64, clear_scores: bool) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let r = repo::get_response(pool, id).await.map_err(err)?.ok_or("Response not found")?;
    if clear_scores {
        let inst = service::instrument_for(pool, &r).await?;
        let mut p = MmRepository::get_profile(pool, &r.profile_id).await.map_err(err)?.ok_or("Profile not found")?;
        for s in &inst.subscales {
            if p.profile.fields.get(&s.field).map_or(false, |e| e.source == matchmaking_core::Provenance::Questionnaire) {
                p.profile.fields.remove(&s.field);
            }
        }
        MmRepository::update_profile_data(pool, &p.profile).await.map_err(err)?;
    }
    repo::delete_response(pool, id).await.map_err(err)?;
    MmRepository::audit(pool, "questionnaire_response_deleted", "profile", &r.profile_id, Some(&r.instrument_key)).await;
    Ok(())
}
