use super::repository::{MmRepository, StoredProfile};
use crate::state::AppState;
use matchmaking_core::{
    conditions_to_expr, search, Condition, FieldDef, FieldKind, Profile, Provenance, ValidationIssue, Value,
};
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::State;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn validate_field_def(def: &FieldDef) -> Result<(), String> {
    let ok_key = !def.key.is_empty()
        && def.key.len() <= 64
        && def.key.chars().next().map_or(false, |c| c.is_ascii_lowercase())
        && def.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !ok_key {
        return Err("Field key must start with a lowercase letter and contain only a-z, 0-9 and _ (max 64 characters)".into());
    }
    if def.label.trim().is_empty() {
        return Err("Field label is required".into());
    }
    if let FieldKind::Choice(o) | FieldKind::MultiChoice(o) = &def.kind {
        if o.is_empty() || o.iter().any(|s| s.trim().is_empty()) {
            return Err("Choice fields need at least one non-empty option".into());
        }
    }
    Ok(())
}

fn parse_source(s: Option<&str>) -> Result<Provenance, String> {
    match s.unwrap_or("matchmaker") {
        "matchmaker" => Ok(Provenance::Matchmaker),
        "user" => Ok(Provenance::User),
        "questionnaire" => Ok(Provenance::Questionnaire),
        "ai_inferred" => Ok(Provenance::AiInferred),
        other => Err(format!("unknown source '{other}'")),
    }
}

#[derive(Serialize)]
pub struct ProfileView {
    pub id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub fields: BTreeMap<String, matchmaking_core::profile::Entry>,
    pub completeness: Option<f64>,
    pub missing_required: Vec<String>,
    pub issues: Vec<ValidationIssue>,
}

#[derive(Serialize)]
pub struct ProfileSummary {
    pub id: String,
    pub status: String,
    pub updated_at: String,
    /// Only non-sensitive fields are shown in lists.
    pub name: Option<String>,
    pub age: Option<f64>,
    pub city: Option<String>,
    pub completeness: Option<f64>,
}

#[derive(Serialize)]
pub struct SearchResultRow {
    #[serde(flatten)]
    pub summary: ProfileSummary,
    /// "true" = matches; "unknown" = a filtered field is missing.
    pub match_result: String,
}

fn view(sp: StoredProfile, reg: &matchmaking_core::FieldRegistry) -> ProfileView {
    ProfileView {
        completeness: sp.profile.completeness(reg),
        missing_required: sp.profile.missing_required(reg),
        issues: sp.profile.validate(reg),
        id: sp.profile.id,
        status: sp.status,
        created_at: sp.created_at,
        updated_at: sp.updated_at,
        fields: sp.profile.fields,
    }
}

fn summary(sp: &StoredProfile, reg: &matchmaking_core::FieldRegistry) -> ProfileSummary {
    // The display name is a sensitive field by default; show it in the matchmaker's own list but never
    // use sensitive fields anywhere that could leave the device.
    let text = |k: &str| match sp.profile.get(k) {
        Some(Value::Text(t)) => Some(t.clone()),
        _ => None,
    };
    ProfileSummary {
        id: sp.profile.id.clone(),
        status: sp.status.clone(),
        updated_at: sp.updated_at.clone(),
        name: text("full_name"),
        age: match sp.profile.get("age") {
            Some(Value::Num(n)) => Some(*n),
            _ => None,
        },
        city: text("city"),
        completeness: sp.profile.completeness(reg),
    }
}

#[tauri::command]
pub async fn mm_list_fields(state: State<'_, AppState>) -> Result<Vec<FieldDef>, String> {
    let reg = MmRepository::registry(state.db_manager.pool()).await.map_err(err)?;
    Ok(reg.defs().cloned().collect())
}

#[tauri::command]
pub async fn mm_save_field(state: State<'_, AppState>, def: FieldDef) -> Result<(), String> {
    validate_field_def(&def)?;
    let pool = state.db_manager.pool();
    let order = MmRepository::next_field_order(pool).await.map_err(err)?;
    MmRepository::save_field(pool, &def, order).await.map_err(err)?;
    MmRepository::audit(pool, "save_field", "field", &def.key, None).await;
    Ok(())
}

#[tauri::command]
pub async fn mm_create_profile(
    state: State<'_, AppState>,
    values: Option<BTreeMap<String, Value>>,
) -> Result<ProfileView, String> {
    let pool = state.db_manager.pool();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let mut profile = Profile::new(uuid::Uuid::new_v4().to_string());
    let mut keys = vec![];
    for (k, v) in values.unwrap_or_default() {
        reg.validate_value(&k, &v).map_err(|e| format!("{k}: {e}"))?;
        profile.set(&k, v, Provenance::Matchmaker);
        keys.push(k);
    }
    MmRepository::insert_profile(pool, &profile).await.map_err(err)?;
    MmRepository::audit(pool, "create", "profile", &profile.id, Some(&keys.join(","))).await;
    let sp = MmRepository::get_profile(pool, &profile.id).await.map_err(err)?.ok_or("profile vanished")?;
    Ok(view(sp, &reg))
}

/// Set or clear fields. A `null` value clears the field. Returns the updated profile plus the keys
/// that were refused because a higher-trust source already owns them (e.g. AI cannot overwrite a human).
#[derive(Serialize)]
pub struct UpdateResult {
    pub profile: ProfileView,
    pub rejected: Vec<String>,
}

#[tauri::command]
pub async fn mm_update_profile_fields(
    state: State<'_, AppState>,
    id: String,
    values: BTreeMap<String, Option<Value>>,
    source: Option<String>,
) -> Result<UpdateResult, String> {
    let pool = state.db_manager.pool();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let source = parse_source(source.as_deref())?;
    let mut sp = MmRepository::get_profile(pool, &id).await.map_err(err)?.ok_or("Profile not found")?;
    let (mut changed, mut rejected) = (vec![], vec![]);
    for (k, v) in values {
        match v {
            Some(v) => {
                reg.validate_value(&k, &v).map_err(|e| format!("{k}: {e}"))?;
                if sp.profile.set(&k, v, source) { changed.push(k) } else { rejected.push(k) }
            }
            None => {
                // Clearing is a human action; AI-inferred sources may not erase human-entered data.
                let owner_rank = sp.profile.fields.get(&k).map(|e| e.source.rank());
                if owner_rank.map_or(true, |r| source.rank() >= r) {
                    if sp.profile.fields.remove(&k).is_some() { changed.push(k) }
                } else {
                    rejected.push(k)
                }
            }
        }
    }
    if !changed.is_empty() {
        MmRepository::update_profile_data(pool, &sp.profile).await.map_err(err)?;
        MmRepository::audit(pool, "update", "profile", &id, Some(&changed.join(","))).await;
    }
    let sp = MmRepository::get_profile(pool, &id).await.map_err(err)?.ok_or("Profile not found")?;
    Ok(UpdateResult { profile: view(sp, &reg), rejected })
}

#[tauri::command]
pub async fn mm_get_profile(state: State<'_, AppState>, id: String) -> Result<ProfileView, String> {
    let pool = state.db_manager.pool();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let sp = MmRepository::get_profile(pool, &id).await.map_err(err)?.ok_or("Profile not found")?;
    Ok(view(sp, &reg))
}

#[tauri::command]
pub async fn mm_list_profiles(
    state: State<'_, AppState>,
    include_deactivated: Option<bool>,
) -> Result<Vec<ProfileSummary>, String> {
    let pool = state.db_manager.pool();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    let all = MmRepository::list_profiles(pool, include_deactivated.unwrap_or(false)).await.map_err(err)?;
    Ok(all.iter().map(|sp| summary(sp, &reg)).collect())
}

#[tauri::command]
pub async fn mm_search_profiles(
    state: State<'_, AppState>,
    conditions: Vec<Condition>,
    include_unknown: Option<bool>,
) -> Result<Vec<SearchResultRow>, String> {
    let pool = state.db_manager.pool();
    let reg = MmRepository::registry(pool).await.map_err(err)?;
    for c in &conditions {
        if reg.get(&c.field).is_none() {
            return Err(format!("unknown field '{}'", c.field));
        }
    }
    let filter = conditions_to_expr(&conditions)?;
    let stored = MmRepository::list_profiles(pool, false).await.map_err(err)?;
    let profiles: Vec<Profile> = stored.iter().map(|s| s.profile.clone()).collect();
    let hits = search(&profiles, &filter, include_unknown.unwrap_or(false));
    let by_id: BTreeMap<&str, &StoredProfile> = stored.iter().map(|s| (s.profile.id.as_str(), s)).collect();
    Ok(hits
        .into_iter()
        .filter_map(|h| {
            by_id.get(h.profile_id.as_str()).map(|sp| SearchResultRow {
                summary: summary(sp, &reg),
                match_result: format!("{:?}", h.result).to_lowercase(),
            })
        })
        .collect())
}

/// Deactivate (hide from search/matching, keep data) or reactivate a profile.
#[tauri::command]
pub async fn mm_set_profile_active(state: State<'_, AppState>, id: String, active: bool) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let n = MmRepository::set_status(pool, &id, if active { "active" } else { "deactivated" }).await.map_err(err)?;
    if n == 0 {
        return Err("Profile not found".into());
    }
    MmRepository::audit(pool, if active { "reactivate" } else { "deactivate" }, "profile", &id, None).await;
    Ok(())
}

/// Permanently delete a profile.
#[tauri::command]
pub async fn mm_delete_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let pool = state.db_manager.pool();
    let n = MmRepository::delete_profile(pool, &id).await.map_err(err)?;
    if n == 0 {
        return Err("Profile not found".into());
    }
    MmRepository::audit(pool, "delete", "profile", &id, None).await;
    Ok(())
}
