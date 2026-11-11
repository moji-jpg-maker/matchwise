//! Storage tests for profile photos: what is kept, where, and what is refused.
//!
//! Every test uses a temporary folder as the app data directory, so nothing is written to the real profile store
//! and the same test can run in parallel with the others.
use super::photo_repo::{add_paths, image_kind, AddOutcome, PhotoRepo, PhotoStore, MAX_PHOTOS_PER_PROFILE, MAX_PHOTO_BYTES};
use super::repository::MmRepository;
use matchmaking_core::{Profile, Provenance, Value};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations").run(&pool).await.unwrap();
    pool
}

async fn profile(pool: &SqlitePool, id: &str) {
    let mut person = Profile::new(id);
    person.set("age", Value::Num(31.0), Provenance::Matchmaker);
    MmRepository::insert_profile(pool, &person).await.unwrap();
}

/// A minimal but real JPEG: the three-byte signature infer checks. Unique filler makes each one distinct.
fn jpeg(tag: u8) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0];
    bytes.extend(std::iter::repeat(tag).take(32));
    bytes
}

fn png(tag: u8) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend(std::iter::repeat(tag).take(32));
    bytes
}

fn webp(tag: u8) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBP".to_vec();
    bytes.extend(std::iter::repeat(tag).take(32));
    bytes
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, bytes).unwrap();
    path
}

struct Fixture {
    _tmp: TempDir,
    store: PhotoStore,
    source: PathBuf,
    pool: SqlitePool,
}

async fn fixture() -> Fixture {
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("incoming");
    fs::create_dir_all(&source).unwrap();
    Fixture {
        store: PhotoStore::new(tmp.path().join("mm_photos")),
        source,
        pool: test_pool().await,
        _tmp: tmp,
    }
}

impl Fixture {
    fn input(&self, name: &str, bytes: &[u8]) -> PathBuf {
        write(&self.source, name, bytes)
    }

    async fn add(&self, profile_id: &str, paths: &[PathBuf]) -> AddOutcome {
        add_paths(&self.pool, &self.store, profile_id, paths).await.unwrap()
    }
}

fn codes(outcome: &AddOutcome) -> Vec<&'static str> {
    let mut v: Vec<&'static str> = outcome.rejected.iter().map(|(_, reason)| reason.code()).collect();
    v.sort();
    v
}

#[test]
fn only_jpeg_png_and_webp_are_images_here() {
    assert_eq!(image_kind(&jpeg(1)).map(|k| k.0), Some("image/jpeg"));
    assert_eq!(image_kind(&png(1)).map(|k| k.0), Some("image/png"));
    assert_eq!(image_kind(&webp(1)).map(|k| k.0), Some("image/webp"));
    assert_eq!(image_kind(b"GIF89a not accepted"), None);
    assert_eq!(image_kind(b"plain text, not an image at all"), None);
    assert_eq!(image_kind(&[]), None);
}

#[tokio::test]
async fn a_file_is_copied_byte_for_byte_into_the_profiles_folder() {
    let f = fixture().await;
    profile(&f.pool, "p1").await;
    let bytes = jpeg(7);
    let path = f.input("portrait.jpg", &bytes);

    let outcome = f.add("p1", &[path]).await;
    assert!(outcome.rejected.is_empty());
    assert_eq!(outcome.photos.len(), 1);
    let photo = &outcome.photos[0];
    assert_eq!(photo.status, "ready");
    assert_eq!(photo.source, "upload");
    assert_eq!(photo.mime, "image/jpeg");
    assert_eq!(photo.size_bytes, bytes.len() as i64);
    assert_eq!(photo.position, 0);
    assert_eq!(photo.original_name.as_deref(), Some("portrait.jpg"));

    let name = photo.stored_name.clone().unwrap();
    let stored = f.store.profile_dir("p1").unwrap().join(&name);
    assert_eq!(fs::read(&stored).unwrap(), bytes, "no re-encoding, no trimming");
    assert!(name.ends_with(".jpg"));
    assert_eq!(photo.sha256.as_deref(), Some(super::photo_repo::sha256_hex(&bytes).as_str()));
}

#[tokio::test]
async fn the_same_picture_is_kept_once_even_with_another_name_or_format() {
    let f = fixture().await;
    profile(&f.pool, "p1").await;
    let bytes = png(3);
    let first = f.input("one.png", &bytes);
    let second = f.input("copy-of-one.png", &bytes);
    let outcome = f.add("p1", &[first, second]).await;

    assert_eq!(outcome.photos.len(), 1);
    assert_eq!(codes(&outcome), vec!["duplicate"]);
    assert_eq!(PhotoRepo::list(&f.pool, "p1").await.unwrap().len(), 1);
    // Only one file was written.
    let names: Vec<_> = fs::read_dir(f.store.profile_dir("p1").unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(names.len(), 1);
}

#[tokio::test]
async fn a_file_that_is_not_an_accepted_image_is_refused_even_named_as_one() {
    let f = fixture().await;
    profile(&f.pool, "p1").await;
    let named_jpg = f.input("pretend.jpg", b"https://example.test/portrait.jpg");
    let gif = f.input("animation.gif", b"GIF89a.......");
    let empty = f.input("empty.png", b"");
    let missing = f.source.join("never-written.jpg");

    let outcome = f.add("p1", &[named_jpg, gif, empty, missing]).await;
    assert!(outcome.photos.is_empty());
    assert_eq!(codes(&outcome), vec!["not_image", "not_image", "not_image", "unreadable"]);
    assert!(!f.store.profile_dir("p1").unwrap().exists(), "nothing is written for a refused file");
}

#[tokio::test]
async fn a_file_over_ten_megabytes_is_refused_before_it_is_read() {
    let f = fixture().await;
    profile(&f.pool, "p1").await;
    let mut big = jpeg(1);
    big.resize(MAX_PHOTO_BYTES as usize + 1, 0);
    let big_path = f.input("huge.jpg", &big);

    let outcome = f.add("p1", &[big_path]).await;
    assert!(outcome.photos.is_empty());
    assert_eq!(codes(&outcome), vec!["too_large"]);
    assert!(PhotoRepo::list(&f.pool, "p1").await.unwrap().is_empty());

    // Exactly at the limit is still accepted.
    let mut at_limit = jpeg(2);
    at_limit.resize(MAX_PHOTO_BYTES as usize, 0);
    let path = f.input("at-limit.jpg", &at_limit);
    assert_eq!(f.add("p1", &[path]).await.photos.len(), 1);
}

#[tokio::test]
async fn the_eleventh_photo_is_refused_and_the_first_ten_are_kept_in_order() {
    let f = fixture().await;
    profile(&f.pool, "p1").await;
    let mut paths = Vec::new();
    for index in 0..12u8 {
        paths.push(f.input(&format!("photo-{index}.jpg"), &jpeg(index)));
    }
    let outcome = f.add("p1", &paths).await;
    assert_eq!(outcome.photos.len() as i64, MAX_PHOTOS_PER_PROFILE);
    assert_eq!(outcome.rejected.len(), 2);
    assert!(outcome.rejected.iter().all(|(_, reason)| reason.code() == "limit"));

    let stored = PhotoRepo::list(&f.pool, "p1").await.unwrap();
    assert_eq!(stored.len() as i64, MAX_PHOTOS_PER_PROFILE);
    assert_eq!(stored.iter().map(|p| p.position).collect::<Vec<_>>(), (0..10).collect::<Vec<_>>());
    assert_eq!(f.store.profile_dir("p1").unwrap().read_dir().unwrap().count() as i64, MAX_PHOTOS_PER_PROFILE);

    // Deleting one makes room for another.
    PhotoRepo::delete(&f.pool, "p1", &stored[0].id).await.unwrap();
    f.store.remove("p1", stored[0].stored_name.as_deref().unwrap()).unwrap();
    let last = f.input("after-delete.jpg", &jpeg(99));
    let outcome = f.add("p1", &[last]).await;
    assert_eq!(outcome.photos.len(), 1);
    assert_eq!(outcome.photos[0].position, MAX_PHOTOS_PER_PROFILE, "appended after the highest slot");
}

#[tokio::test]
async fn deleting_a_photo_removes_its_file_but_not_the_others() {
    let f = fixture().await;
    profile(&f.pool, "p1").await;
    let a = f.input("a.png", &png(1));
    let b = f.input("b.jpg", &jpeg(2));
    let outcome = f.add("p1", &[a, b]).await;
    let (first, second) = (&outcome.photos[0], &outcome.photos[1]);
    let first_path = f.store.profile_dir("p1").unwrap().join(first.stored_name.clone().unwrap());
    let second_path = f.store.profile_dir("p1").unwrap().join(second.stored_name.clone().unwrap());
    assert!(first_path.exists() && second_path.exists());

    let removed = PhotoRepo::delete(&f.pool, "p1", &first.id).await.unwrap().unwrap();
    f.store.remove("p1", removed.stored_name.as_deref().unwrap()).unwrap();
    assert!(!first_path.exists());
    assert!(second_path.exists());
    assert_eq!(PhotoRepo::list(&f.pool, "p1").await.unwrap().len(), 1);
    // A photo of another profile is not reachable by id.
    assert!(PhotoRepo::delete(&f.pool, "someone-else", &second.id).await.unwrap().is_none());
}

#[tokio::test]
async fn deleting_the_profile_removes_its_photo_folder() {
    let f = fixture().await;
    profile(&f.pool, "p1").await;
    let path = f.input("only.webp", &webp(4));
    f.add("p1", &[path]).await;
    let dir = f.store.profile_dir("p1").unwrap();
    assert!(dir.exists());

    assert_eq!(MmRepository::delete_profile(&f.pool, "p1").await.unwrap(), 1);
    PhotoRepo::delete_rows(&f.pool, "p1").await.unwrap();
    f.store.remove_profile("p1").unwrap();
    assert!(!dir.exists());
    assert!(PhotoRepo::list(&f.pool, "p1").await.unwrap().is_empty());
    // Removing a folder that is already gone is fine, so a retried delete cannot fail.
    f.store.remove_profile("p1").unwrap();
}

#[test]
fn a_name_cannot_escape_the_photo_folder() {
    let store = PhotoStore::new(PathBuf::from("/tmp/mm_photos_guard"));
    assert!(store.path_for("p1", "../../etc/passwd").is_none());
    assert!(store.path_for("..", "a.jpg").is_none());
    assert!(store.path_for("p1", "..").is_none());
    assert!(store.profile_dir("p1").is_ok());
}
