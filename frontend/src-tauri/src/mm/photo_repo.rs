//! Where a person's photos live.
//!
//! The bytes are files, not database blobs: one folder per profile under `app_data_dir/mm_photos/<profile_id>/`,
//! and a row in `mm_profile_photos` that says what is in the folder. Photos are stored exactly as received — no
//! resizing, no re-encoding, no EXIF stripping — so what the matchmaker sees on disk is what arrived.
//!
//! Only three formats are accepted, and the format is decided by the bytes rather than by the file name: a `.jpg`
//! that is not a JPEG is refused. A person may have at most ten photos, each at most 10 MB, and the same picture
//! is kept once. Rows with no file yet (a Telegram photo still waiting to download) hold no `stored_name` and no
//! hash, so they do not collide with each other or with a finished photo.
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use super::repository::ORG;
/// A single photo may not be larger than this.
pub const MAX_PHOTO_BYTES: u64 = 10 * 1024 * 1024;
/// A person may have at most this many photos, pending ones included.
pub const MAX_PHOTOS_PER_PROFILE: i64 = 10;
/// The folder that holds every profile's photos, under the app data directory.
pub fn photos_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("mm_photos")
}
/// The format of an image, decided by its leading bytes. `None` means the bytes are not one of the three formats
/// the app accepts.
pub fn image_kind(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    match infer::get(bytes).map(|kind| kind.mime_type()) {
        Some("image/jpeg") => Some(("image/jpeg", "jpg")),
        Some("image/png") => Some(("image/png", "png")),
        Some("image/webp") => Some(("image/webp", "webp")),
        _ => None,
    }
}
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
/// One stored profile photo, as the database knows it.
#[derive(Debug, Clone)]
pub struct PhotoRow {
    pub id: String,
    pub profile_id: String,
    /// `None` while a Telegram photo is still waiting to download.
    pub stored_name: Option<String>,
    pub original_name: Option<String>,
    pub mime: String,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub position: i64,
    pub source: String,
    pub status: String,
    pub telegram_file_id: Option<String>,
    /// Times the Telegram downloader has tried this row. A new row starts at 0.
    pub download_attempts: i64,
    pub created_at: String,
}
/// A file that was written for a photo.
#[derive(Debug, Clone)]
pub struct StoredFile {
    pub stored_name: String,
    pub size_bytes: i64,
    pub sha256: String,
}
/// The photo folder of one installation.
#[derive(Debug, Clone)]
pub struct PhotoStore {
    base: PathBuf,
}
impl PhotoStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        PhotoStore { base: base.into() }
    }
    pub fn base(&self) -> &Path {
        &self.base
    }
    /// `<base>/<profile_id>`. The profile id is a UUID, but it still must not be able to leave the folder.
    pub fn profile_dir(&self, profile_id: &str) -> io::Result<PathBuf> {
        if !is_safe_component(profile_id) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid profile id"));
        }
        Ok(self.base.join(profile_id))
    }
    pub fn path_for(&self, profile_id: &str, stored_name: &str) -> Option<PathBuf> {
        if !is_safe_component(stored_name) {
            return None;
        }
        self.profile_dir(profile_id).ok().map(|dir| dir.join(stored_name))
    }
    /// Write the bytes as they are and report what was written. A short write cannot leave a truncated photo:
    /// the bytes go to a temporary name first and are renamed into place.
    pub fn save(&self, profile_id: &str, bytes: &[u8], mime: &str) -> io::Result<StoredFile> {
        let extension = match mime {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/webp" => "webp",
            other => return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("unsupported image type {other}"))),
        };
        let dir = self.profile_dir(profile_id)?;
        fs::create_dir_all(&dir)?;
        let stored_name = format!("{}.{extension}", uuid::Uuid::new_v4());
        let tmp = dir.join(format!("{stored_name}.tmp"));
        fs::write(&tmp, bytes)?;
        if let Err(e) = fs::rename(&tmp, dir.join(&stored_name)) {
            let _ = fs::remove_file(&tmp);
            return Err(e);
        }
        Ok(StoredFile {
            stored_name,
            size_bytes: bytes.len() as i64,
            sha256: sha256_hex(bytes),
        })
    }
    pub fn remove(&self, profile_id: &str, stored_name: &str) -> io::Result<()> {
        let path = self
            .path_for(profile_id, stored_name)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid photo name"))?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
    /// Remove the whole folder. A folder that is already gone is not an error.
    pub fn remove_profile(&self, profile_id: &str) -> io::Result<()> {
        let dir = self.profile_dir(profile_id)?;
        match fs::remove_dir_all(dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}
/// A folder or file name that stays inside its parent: no separators, no `.` or `..`.
fn is_safe_component(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}
/// Why a file was not stored. Kept as a short code: the UI translates it, and no file name reaches the audit log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhotoRejection {
    Unreadable,
    TooLarge,
    NotImage,
    Duplicate,
    Limit,
}
impl PhotoRejection {
    pub fn code(self) -> &'static str {
        match self {
            PhotoRejection::Unreadable => "unreadable",
            PhotoRejection::TooLarge => "too_large",
            PhotoRejection::NotImage => "not_image",
            PhotoRejection::Duplicate => "duplicate",
            PhotoRejection::Limit => "limit",
        }
    }
}
pub struct AddOutcome {
    pub photos: Vec<PhotoRow>,
    pub rejected: Vec<(String, PhotoRejection)>,
}
pub struct PhotoRepo;
fn bad(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}
const COLS: &str = "id, profile_id, stored_name, original_name, mime, size_bytes, sha256, position, source, status, telegram_file_id, download_attempts, created_at";
fn from_row(r: &sqlx::sqlite::SqliteRow) -> PhotoRow {
    PhotoRow {
        id: r.get("id"),
        profile_id: r.get("profile_id"),
        stored_name: r.get("stored_name"),
        original_name: r.get("original_name"),
        mime: r.get("mime"),
        size_bytes: r.get("size_bytes"),
        sha256: r.get("sha256"),
        position: r.get("position"),
        source: r.get("source"),
        status: r.get("status"),
        telegram_file_id: r.get("telegram_file_id"),
        download_attempts: r.get("download_attempts"),
        created_at: r.get("created_at"),
    }
}
impl PhotoRepo {
    pub async fn list(pool: &SqlitePool, profile_id: &str) -> Result<Vec<PhotoRow>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM mm_profile_photos WHERE profile_id = ? AND org_id = ? ORDER BY position, created_at"
        ))
        .bind(profile_id)
        .bind(ORG)
        .fetch_all(pool)
        .await?;
        Ok(rows.iter().map(from_row).collect())
    }
    pub async fn get(pool: &SqlitePool, profile_id: &str, id: &str) -> Result<Option<PhotoRow>, sqlx::Error> {
        let row = sqlx::query(&format!(
            "SELECT {COLS} FROM mm_profile_photos WHERE profile_id = ? AND id = ? AND org_id = ?"
        ))
        .bind(profile_id)
        .bind(id)
        .bind(ORG)
        .fetch_optional(pool)
        .await?;
        Ok(row.as_ref().map(from_row))
    }
    pub async fn count(pool: &SqlitePool, profile_id: &str) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT count(*) FROM mm_profile_photos WHERE profile_id = ? AND org_id = ?")
            .bind(profile_id)
            .bind(ORG)
            .fetch_one(pool)
            .await
    }
    /// Append a photo in the next free slot. Returns `None` when the profile already has ten photos; the limit and
    /// the position are read inside one transaction so two additions cannot take the same slot.
    pub async fn append(pool: &SqlitePool, photo: &PhotoRow) -> Result<Option<PhotoRow>, sqlx::Error> {
        let mut tx = pool.begin().await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM mm_profile_photos WHERE profile_id = ? AND org_id = ?")
            .bind(&photo.profile_id)
            .bind(ORG)
            .fetch_one(&mut *tx)
            .await?;
        if count >= MAX_PHOTOS_PER_PROFILE {
            return Ok(None);
        }
        let position: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM mm_profile_photos WHERE profile_id = ? AND org_id = ?",
        )
        .bind(&photo.profile_id)
        .bind(ORG)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(&format!(
            "INSERT INTO mm_profile_photos (org_id, {COLS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(ORG)
        .bind(&photo.id)
        .bind(&photo.profile_id)
        .bind(&photo.stored_name)
        .bind(&photo.original_name)
        .bind(&photo.mime)
        .bind(photo.size_bytes)
        .bind(&photo.sha256)
        .bind(position)
        .bind(&photo.source)
        .bind(&photo.status)
        .bind(&photo.telegram_file_id)
        .bind(photo.download_attempts)
        .bind(&photo.created_at)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(
            PhotoRepo::get(pool, &photo.profile_id, &photo.id)
                .await?
                .ok_or_else(|| bad("photo row missing after insert"))?,
        ))
    }
    /// Remove one photo's row and return it, so the caller can remove the file it names.
    pub async fn delete(pool: &SqlitePool, profile_id: &str, id: &str) -> Result<Option<PhotoRow>, sqlx::Error> {
        let photo = Self::get(pool, profile_id, id).await?;
        if photo.is_none() {
            return Ok(None);
        }
        sqlx::query("DELETE FROM mm_profile_photos WHERE id = ? AND profile_id = ? AND org_id = ?")
            .bind(id)
            .bind(profile_id)
            .bind(ORG)
            .execute(pool)
            .await?;
        Ok(photo)
    }
    /// Rows only. The folder is removed by the caller, which knows where photos are stored.
    pub async fn delete_rows(pool: &SqlitePool, profile_id: &str) -> Result<u64, sqlx::Error> {
        let r = sqlx::query("DELETE FROM mm_profile_photos WHERE profile_id = ? AND org_id = ?")
            .bind(profile_id)
            .bind(ORG)
            .execute(pool)
            .await?;
        Ok(r.rows_affected())
    }
    /// Photos that arrived through Telegram and still have no bytes, oldest first. A row is only handed out while it
    /// is `pending`: a `failed` row is done with, and a `ready` one already has its file.
    pub async fn pending_downloads(pool: &SqlitePool, limit: i64) -> Result<Vec<PhotoRow>, sqlx::Error> {
        let rows = sqlx::query(&format!(
            "SELECT {COLS} FROM mm_profile_photos
             WHERE org_id = ? AND source = 'telegram' AND status = 'pending' AND telegram_file_id IS NOT NULL
             ORDER BY created_at, id LIMIT ?"
        ))
        .bind(ORG)
        .bind(limit)
        .fetch_all(pool)
        .await?;
        Ok(rows.iter().map(from_row).collect())
    }
    /// Give one pending row its bytes. The hash decides duplicates across the whole profile, not just across
    /// uploads: a photo sent twice by different routes is still one picture. Returns `false` when the row was
    /// already filled in by a racing worker, in which case the caller removes the file it just wrote.
    pub async fn complete_download(
        pool: &SqlitePool,
        profile_id: &str,
        id: &str,
        file: &StoredFile,
        mime: &str,
    ) -> Result<bool, sqlx::Error> {
        let r = sqlx::query(
            "UPDATE mm_profile_photos
             SET stored_name = ?, mime = ?, size_bytes = ?, sha256 = ?, status = 'ready'
             WHERE id = ? AND profile_id = ? AND org_id = ? AND status = 'pending'",
        )
        .bind(&file.stored_name)
        .bind(mime)
        .bind(file.size_bytes)
        .bind(&file.sha256)
        .bind(id)
        .bind(profile_id)
        .bind(ORG)
        .execute(pool)
        .await?
        .rows_affected();
        Ok(r == 1)
    }
    /// Delete a `pending` row outright. Used when the bytes turn out to repeat a photo the person already has, so
    /// the duplicate does not hold a slot. Returns whether a row was removed.
    pub async fn delete_pending(pool: &SqlitePool, profile_id: &str, id: &str) -> Result<bool, sqlx::Error> {
        let r = sqlx::query("DELETE FROM mm_profile_photos WHERE id = ? AND profile_id = ? AND org_id = ? AND status = 'pending'")
            .bind(id)
            .bind(profile_id)
            .bind(ORG)
            .execute(pool)
            .await?
            .rows_affected();
        Ok(r == 1)
    }
    /// Count one failed attempt and give up after `max_attempts`. Returns the attempt number just recorded.
    pub async fn record_download_failure(
        pool: &SqlitePool,
        profile_id: &str,
        id: &str,
        max_attempts: i64,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query(
            "UPDATE mm_profile_photos
             SET download_attempts = download_attempts + 1,
                 status = CASE WHEN download_attempts + 1 >= ? THEN 'failed' ELSE 'pending' END
             WHERE id = ? AND profile_id = ? AND org_id = ? AND status = 'pending'",
        )
        .bind(max_attempts)
        .bind(id)
        .bind(profile_id)
        .bind(ORG)
        .execute(pool)
        .await?;
        sqlx::query_scalar("SELECT download_attempts FROM mm_profile_photos WHERE id = ? AND org_id = ?")
            .bind(id)
            .bind(ORG)
            .fetch_optional(pool)
            .await
            .map(|n: Option<i64>| n.unwrap_or(max_attempts))
    }
    /// Every hash already on the profile. A download is checked against these before it takes a slot.
    pub async fn hashes(pool: &SqlitePool, profile_id: &str) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT sha256 FROM mm_profile_photos WHERE profile_id = ? AND org_id = ? AND sha256 IS NOT NULL")
            .bind(profile_id)
            .bind(ORG)
            .fetch_all(pool)
            .await
    }
}
/// Read files from disk, keep the ones that are JPEG, PNG or WebP within the limits, and store them in order. The
/// caller decides where the folder is; nothing here touches Tauri.
pub async fn add_paths(
    pool: &SqlitePool,
    store: &PhotoStore,
    profile_id: &str,
    paths: &[PathBuf],
) -> Result<AddOutcome, String> {
    let mut existing: Vec<String> = sqlx::query_scalar(
        "SELECT sha256 FROM mm_profile_photos WHERE profile_id = ? AND org_id = ? AND sha256 IS NOT NULL",
    )
    .bind(profile_id)
    .bind(ORG)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut rejected = Vec::new();
    let mut added = Vec::new();
    let mut count = PhotoRepo::count(pool, profile_id).await.map_err(|e| e.to_string())?;
    for path in paths {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        if count >= MAX_PHOTOS_PER_PROFILE {
            rejected.push((name, PhotoRejection::Limit));
            continue;
        }
        let Ok(meta) = fs::metadata(path) else {
            rejected.push((name, PhotoRejection::Unreadable));
            continue;
        };
        if meta.len() > MAX_PHOTO_BYTES {
            rejected.push((name, PhotoRejection::TooLarge));
            continue;
        }
        let Ok(bytes) = fs::read(path) else {
            rejected.push((name, PhotoRejection::Unreadable));
            continue;
        };
        let Some((mime, _)) = image_kind(&bytes) else {
            rejected.push((name, PhotoRejection::NotImage));
            continue;
        };
        let digest = sha256_hex(&bytes);
        if existing.contains(&digest) {
            rejected.push((name, PhotoRejection::Duplicate));
            continue;
        }
        let file = store.save(profile_id, &bytes, mime).map_err(|e| e.to_string())?;
        let photo = PhotoRow {
            id: uuid::Uuid::new_v4().to_string(),
            profile_id: profile_id.to_string(),
            stored_name: Some(file.stored_name.clone()),
            original_name: Some(name.clone()),
            mime: mime.to_string(),
            size_bytes: file.size_bytes,
            sha256: Some(file.sha256.clone()),
            position: 0,
            source: "upload".to_string(),
            status: "ready".to_string(),
            telegram_file_id: None,
            download_attempts: 0,
            created_at: Utc::now().to_rfc3339(),
        };
        match PhotoRepo::append(pool, &photo).await {
            Ok(Some(stored)) => {
                existing.push(file.sha256);
                count += 1;
                added.push(stored);
            }
            Ok(None) => {
                let _ = store.remove(profile_id, &file.stored_name);
                rejected.push((name, PhotoRejection::Limit));
            }
            // Two additions at once can both pass the check above; the unique rule then decides, and the second
            // one is the duplicate it is.
            Err(e) if e.as_database_error().is_some_and(|d| d.is_unique_violation()) => {
                let _ = store.remove(profile_id, &file.stored_name);
                rejected.push((name, PhotoRejection::Duplicate));
            }
            Err(e) => {
                let _ = store.remove(profile_id, &file.stored_name);
                return Err(e.to_string());
            }
        }
    }
    Ok(AddOutcome { photos: added, rejected })
}
