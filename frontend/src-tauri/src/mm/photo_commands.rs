//! Commands over a person's photos: list, add, delete.
//!
//! The bytes are read here and handed to `photo_repo`, which decides what is an image and where it goes. A profile
//! must exist before a photo can be attached to it. The audit log records how many photos changed, never a file
//! name, a path or a hash: a photo is sensitive content and its name can be identifying.
use super::photo_repo::{self, PhotoRepo, PhotoRow, PhotoStore};
use super::repository::MmRepository;
use crate::state::AppState;
use matchmaking_core::Locale;
use serde::Serialize;
use std::path::PathBuf;
use tauri::{Manager, State};

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn profile_not_found(locale: Locale) -> String {
    locale.pick("Profile not found", "پروفایل پیدا نشد").to_string()
}

/// Where photos live for this installation. `None` only if the platform gives no app data directory at all.
fn store<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> Result<PhotoStore, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(PhotoStore::new(photo_repo::photos_dir(&dir)))
}

/// One photo as the UI sees it. `path` is the absolute file path; the UI shows it through the asset protocol.
#[derive(Debug, Clone, Serialize)]
pub struct PhotoView {
    pub id: String,
    pub profile_id: String,
    pub path: Option<String>,
    pub original_name: Option<String>,
    pub mime: String,
    pub size_bytes: i64,
    pub position: i64,
    pub source: String,
    pub status: String,
    pub created_at: String,
}

fn view(photo: PhotoRow, store: &PhotoStore) -> PhotoView {
    let path = photo
        .stored_name
        .as_deref()
        .and_then(|name| store.path_for(&photo.profile_id, name))
        .map(|p| p.to_string_lossy().to_string());
    PhotoView {
        id: photo.id,
        profile_id: photo.profile_id,
        path,
        original_name: photo.original_name,
        mime: photo.mime,
        size_bytes: photo.size_bytes,
        position: photo.position,
        source: photo.source,
        status: photo.status,
        created_at: photo.created_at,
    }
}

/// A file that was not stored, and why.
#[derive(Debug, Clone, Serialize)]
pub struct RejectedPhoto {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddPhotosResult {
    pub photos: Vec<PhotoView>,
    pub rejected: Vec<RejectedPhoto>,
}

async fn ensure_profile(state: &AppState, profile_id: &str, locale: Locale) -> Result<(), String> {
    let pool = state.db_manager.pool();
    match MmRepository::get_profile(pool, profile_id).await.map_err(err)? {
        Some(_) => Ok(()),
        None => Err(profile_not_found(locale)),
    }
}

#[tauri::command]
pub async fn mm_profile_photos(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    profile_id: String,
    locale: Option<String>,
) -> Result<Vec<PhotoView>, String> {
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    ensure_profile(&state, &profile_id, locale).await?;
    let store = store(&app)?;
    let rows = PhotoRepo::list(state.db_manager.pool(), &profile_id).await.map_err(err)?;
    Ok(rows.into_iter().map(|p| view(p, &store)).collect())
}

/// Store the files the matchmaker picked. Files that are not JPEG, PNG or WebP, that are over 10 MB, that repeat a
/// photo the person already has, or that would be an eleventh photo are reported back instead of stored.
#[tauri::command]
pub async fn mm_add_profile_photos(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    profile_id: String,
    paths: Vec<String>,
    locale: Option<String>,
) -> Result<AddPhotosResult, String> {
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    ensure_profile(&state, &profile_id, locale).await?;
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    if paths.is_empty() {
        return Ok(AddPhotosResult { photos: vec![], rejected: vec![] });
    }
    let store = store(&app)?;
    let pool = state.db_manager.pool();
    let outcome = photo_repo::add_paths(pool, &store, &profile_id, &paths).await?;
    if !outcome.photos.is_empty() {
        // Count only: a file name can be identifying.
        MmRepository::audit(pool, "add_photos", "profile", &profile_id, Some(&format!("{} photos", outcome.photos.len())))
            .await;
    }
    Ok(AddPhotosResult {
        photos: outcome.photos.into_iter().map(|p| view(p, &store)).collect(),
        rejected: outcome
            .rejected
            .into_iter()
            .map(|(name, reason)| RejectedPhoto { name, reason: reason.code().to_string() })
            .collect(),
    })
}

#[tauri::command]
pub async fn mm_delete_profile_photo(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    profile_id: String,
    photo_id: String,
    locale: Option<String>,
) -> Result<(), String> {
    let locale = Locale::parse_or_default(locale.as_deref().unwrap_or_default());
    ensure_profile(&state, &profile_id, locale).await?;
    let store = store(&app)?;
    let pool = state.db_manager.pool();
    let removed = PhotoRepo::delete(pool, &profile_id, &photo_id).await.map_err(err)?;
    let Some(photo) = removed else {
        return Err(locale.pick("Photo not found", "عکس پیدا نشد").to_string());
    };
    if let Some(name) = photo.stored_name.as_deref() {
        store.remove(&profile_id, name).map_err(err)?;
    }
    MmRepository::audit(pool, "delete_photo", "profile", &profile_id, Some("1 photo")).await;
    Ok(())
}

/// Remove the profile's photo folder after its rows are gone (or with them). Used by `mm_delete_profile`.
pub async fn delete_profile_photos<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    pool: &sqlx::SqlitePool,
    profile_id: &str,
) -> Result<(), String> {
    PhotoRepo::delete_rows(pool, profile_id).await.map_err(err)?;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    PhotoStore::new(photo_repo::photos_dir(&dir))
        .remove_profile(profile_id)
        .map_err(err)
}

