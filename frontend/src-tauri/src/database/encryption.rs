//! At-rest encryption for the application database (SQLCipher).
//!
//! * A random 256-bit key is generated once and stored in the OS credential store
//!   (Windows Credential Manager, macOS Keychain, Linux Secret Service).
//! * `MATCHWISE_DB_KEY` (64 hex chars) overrides the credential store. Use it on headless
//!   machines and on WSL without a running Secret Service daemon.
//! * [`open_database`] is the single entry point. It never modifies an existing database unless it
//!   can prove the key opens it, and it converts a plaintext database in a crash-safe order.
//!
//! Conversion order (the original database file is intact at every step):
//!   1. export to `<db>.enc.tmp`, verify it (key opens it, integrity check, per-table row counts)
//!   2. copy the plaintext original to `<db>.plaintext.bak` and verify the copy is byte-identical
//!   3. atomically rename `<db>.enc.tmp` over `<db>` (rename replaces the target in one step)
//!
//! A crash before step 3 leaves the plaintext original untouched, and the next start resumes
//! safely (a stale `.enc.tmp` is discarded; an identical `.bak` is reused). A crash after step 3
//! leaves the finished encrypted database plus the backup. The database path is never missing.

use rand::RngCore;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{ConnectOptions, Connection, SqliteConnection, SqlitePool};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const KEYRING_SERVICE: &str = "com.matchwise.desktop";
const KEYRING_ACCOUNT: &str = "database-encryption-key";
const KEY_ENV: &str = "MATCHWISE_DB_KEY";
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

fn is_valid_key(k: &str) -> bool {
    k.len() == 64 && k.bytes().all(|b| b.is_ascii_hexdigit())
}

fn protocol(msg: impl Into<String>) -> sqlx::Error {
    sqlx::Error::Protocol(msg.into())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// True if `path` exists and starts with the plain SQLite header (i.e. is NOT encrypted).
pub fn is_plaintext_sqlite(path: &Path) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut header = [0u8; 16];
    let mut f = fs::File::open(path)?;
    match f.read_exact(&mut header) {
        Ok(()) => Ok(&header == SQLITE_MAGIC),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false), // empty/tiny file
        Err(e) => Err(e),
    }
}

/// Fetch the database key from the environment override or the OS credential store. A new key is
/// created only if `allow_create` is true; callers pass false when an encrypted database already
/// exists, so a lost key never silently produces a second, unrelated key.
pub fn database_key(allow_create: bool) -> Result<String, String> {
    if let Ok(v) = std::env::var(KEY_ENV) {
        let v = v.trim().to_lowercase();
        return if is_valid_key(&v) {
            Ok(v)
        } else {
            Err(format!("{KEY_ENV} must be exactly 64 hexadecimal characters"))
        };
    }

    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("OS credential store unavailable: {e}. {}", hint()))?;

    match entry.get_password() {
        Ok(k) if is_valid_key(&k) => Ok(k),
        Ok(_) => Err("The stored database key is malformed; refusing to continue".to_string()),
        Err(keyring::Error::NoEntry) => {
            if !allow_create {
                return Err(format!(
                    "An encrypted database exists but its key was not found in the OS credential store. \
                     If you have the key, set {KEY_ENV}. Without it the data cannot be recovered."
                ));
            }
            let mut bytes = [0u8; 32];
            rand::rngs::OsRng.fill_bytes(&mut bytes);
            let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            entry
                .set_password(&key)
                .map_err(|e| format!("Could not store the database key in the OS credential store: {e}. {}", hint()))?;
            Ok(key)
        }
        Err(e) => Err(format!("OS credential store unavailable: {e}. {}", hint())),
    }
}

fn hint() -> String {
    format!(
        "On Linux/WSL without a Secret Service (gnome-keyring/KWallet) running, set {KEY_ENV} to a 64-character hex string \
         (for example: export {KEY_ENV}=$(openssl rand -hex 32)) and keep a copy of it safe."
    )
}

/// The `key` pragma value for a raw 256-bit key.
pub fn key_pragma_value(key_hex: &str) -> String {
    format!("\"x'{key_hex}'\"")
}

fn sql_quote(s: &str) -> String {
    s.replace('\'', "''")
}

fn ident_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn files_identical(a: &Path, b: &Path) -> std::io::Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let (mut fa, mut fb) = (fs::File::open(a)?, fs::File::open(b)?);
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let na = fa.read(&mut ba)?;
        if na == 0 {
            return Ok(fb.read(&mut bb)? == 0);
        }
        let mut got = 0;
        while got < na {
            let n = fb.read(&mut bb[got..na])?;
            if n == 0 {
                return Ok(false);
            }
            got += n;
        }
        if ba[..na] != bb[..na] {
            return Ok(false);
        }
    }
}

async fn table_counts(conn: &mut SqliteConnection) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let names: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .fetch_all(&mut *conn)
            .await?;
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        let c: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}", ident_quote(&n))).fetch_one(&mut *conn).await?;
        out.push((n, c));
    }
    Ok(out)
}

/// Open `path` with `key_hex` on a single connection and read its schema. Fails (without writing
/// anything) if the key is wrong or the file is not a database.
async fn verify_key(path: &Path, key_hex: &str) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .read_only(true)
        .pragma("key", key_pragma_value(key_hex));
    let mut conn: SqliteConnection = opts.connect().await?;
    let res = table_counts(&mut conn).await;
    let _ = conn.close().await;
    res
}

/// Convert a plaintext SQLite file to an encrypted one, keeping the original as `<path>.plaintext.bak`.
/// See the module docs for the crash-safe ordering.
pub async fn encrypt_in_place(path: &Path, key_hex: &str) -> Result<(), sqlx::Error> {
    if !path.exists() {
        return Err(protocol(format!("{} does not exist", path.display())));
    }
    let tmp = with_suffix(path, ".enc.tmp");
    let bak = with_suffix(path, ".plaintext.bak");

    // A leftover backup is only reusable if it is exactly the file we are about to convert.
    let bak_reusable = if bak.exists() {
        if files_identical(path, &bak).map_err(sqlx::Error::Io)? {
            true
        } else {
            return Err(protocol(format!(
                "{} already exists and differs from the database being converted; nothing was changed. \
                 Move or delete the backup yourself once you know which copy is correct.",
                bak.display()
            )));
        }
    } else {
        false
    };
    let _ = fs::remove_file(&tmp); // stale output of an interrupted run

    // create_if_missing(true): ATTACH of the new encrypted file inherits the connection's open flags and
    // needs SQLITE_OPEN_CREATE. The file exists (checked above), so this never creates the plaintext db.
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut conn: SqliteConnection = opts.connect().await?;

    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&mut conn).await?;
    let plain_counts = table_counts(&mut conn).await?;

    sqlx::query(&format!(
        "ATTACH DATABASE '{}' AS encrypted KEY \"x'{}'\"",
        sql_quote(&tmp.to_string_lossy()),
        key_hex
    ))
    .execute(&mut conn)
    .await?;
    sqlx::query("SELECT sqlcipher_export('encrypted')").execute(&mut conn).await?;
    sqlx::query(&format!("PRAGMA encrypted.user_version = {user_version}")).execute(&mut conn).await?;
    sqlx::query("DETACH DATABASE encrypted").execute(&mut conn).await?;
    conn.close().await?;

    // 1. Verify the encrypted copy before anything else is touched.
    let verified: Result<(), sqlx::Error> = async {
        let enc_counts = verify_key(&tmp, key_hex).await?;
        if enc_counts != plain_counts {
            return Err(protocol(format!(
                "encryption verification failed: tables/row counts differ ({} tables before, {} after)",
                plain_counts.len(),
                enc_counts.len()
            )));
        }
        let opts = SqliteConnectOptions::new()
            .filename(&tmp)
            .create_if_missing(false)
            .read_only(true)
            .pragma("key", key_pragma_value(key_hex));
        let mut c: SqliteConnection = opts.connect().await?;
        let integrity: String = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(&mut c).await?;
        let uv: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&mut c).await?;
        let _ = c.close().await;
        if integrity != "ok" || uv != user_version {
            return Err(protocol(format!("encryption verification failed: integrity '{integrity}', user_version {uv}")));
        }
        Ok(())
    }
    .await;
    if let Err(e) = verified {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    // 2. Keep a verified plaintext backup (copy, not rename: the original stays in place).
    if !bak_reusable {
        let backup_result = (|| -> std::io::Result<()> {
            fs::copy(path, &bak)?;
            fs::File::open(&bak)?.sync_all()?;
            if !files_identical(path, &bak)? {
                return Err(std::io::Error::new(std::io::ErrorKind::Other, "backup copy does not match the original"));
            }
            Ok(())
        })();
        if let Err(e) = backup_result {
            let _ = fs::remove_file(&bak);
            let _ = fs::remove_file(&tmp);
            return Err(sqlx::Error::Io(e));
        }
    }
    fs::File::open(&tmp).and_then(|f| f.sync_all()).map_err(sqlx::Error::Io)?;

    // 3. Stale sidecar files belong to the plaintext database and must not meet the new file.
    //    (The connection above used journal_mode=DELETE and closed cleanly, so none are expected.)
    for ext in ["-wal", "-shm", "-journal"] {
        let _ = fs::remove_file(with_suffix(path, ext));
    }
    // Atomic replace: the database path always refers to a complete database.
    fs::rename(&tmp, path).map_err(sqlx::Error::Io)?;

    log::warn!(
        "Database encrypted. The plaintext original was kept at {} -- delete it once you have verified the app works.",
        bak.display()
    );
    Ok(())
}

/// Key source: `Sync` so the `open_database` future stays `Send` and can run on Tauri's async runtime.
pub type KeyProvider = dyn Fn(bool) -> Result<String, String> + Sync;

/// Open (creating or converting if needed) the encrypted application database.
///
/// `key_provider(allow_create)` supplies the key; production code passes [`database_key`]. It is called
/// with `allow_create = false` whenever an encrypted database already exists.
pub async fn open_database(
    path: &Path,
    key_provider: &KeyProvider,
) -> Result<SqlitePool, sqlx::Error> {
    let cfg = |m: String| sqlx::Error::Configuration(m.into());
    let bak = with_suffix(path, ".plaintext.bak");

    // A stale temp file is always garbage from an interrupted conversion.
    let _ = fs::remove_file(with_suffix(path, ".enc.tmp"));

    let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let plaintext = is_plaintext_sqlite(path).map_err(sqlx::Error::Io)?;

    if len == 0 && bak.exists() {
        // Never silently start a new empty database next to a backup of real data.
        return Err(cfg(format!(
            "No database found at {} but a backup exists at {}. Restore the backup manually before starting.",
            path.display(),
            bak.display()
        )));
    }

    let (key, existing) = if len == 0 {
        (key_provider(true).map_err(cfg)?, false)
    } else if plaintext {
        let key = key_provider(true).map_err(cfg)?;
        log::warn!("Plaintext database found; converting to an encrypted database");
        encrypt_in_place(path, &key).await?;
        (key, true)
    } else {
        // Looks encrypted: the key MUST already exist. Prove it opens the file before using it for anything.
        let key = key_provider(false).map_err(cfg)?;
        (key, true)
    };

    if existing {
        verify_key(path, &key).await.map_err(|e| {
            cfg(format!(
                "The database at {} could not be opened with the available encryption key ({e}). \
                 The file was not modified. If the key was lost, the data cannot be recovered.",
                path.display()
            ))
        })?;
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(!existing)
        .pragma("key", key_pragma_value(&key));
    SqlitePool::connect_with(options).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicI8, Ordering};
    use std::sync::Arc;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const OTHER: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    async fn make_plain(path: &Path, secret: &str) {
        let mut c = SqliteConnectOptions::new().filename(path).create_if_missing(true).connect().await.unwrap();
        sqlx::query("CREATE TABLE t (v TEXT)").execute(&mut c).await.unwrap();
        sqlx::query("INSERT INTO t VALUES (?)").bind(secret).execute(&mut c).await.unwrap();
        sqlx::query("PRAGMA user_version = 5").execute(&mut c).await.unwrap();
        c.close().await.unwrap();
    }

    fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (p.file_name().unwrap().to_string_lossy().to_string(), fs::read(&p).unwrap())
            })
            .collect();
        v.sort();
        v
    }

    async fn read_back(path: &Path, key: &str) -> (String, i64) {
        let mut c = SqliteConnectOptions::new().filename(path).pragma("key", key_pragma_value(key)).connect().await.unwrap();
        let v: String = sqlx::query_scalar("SELECT v FROM t").fetch_one(&mut c).await.unwrap();
        let uv: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&mut c).await.unwrap();
        (v, uv)
    }

    fn assert_send<T: Send>(_: &T) {}

    #[tokio::test]
    async fn open_database_future_is_send() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        let fut = open_database(&path, &database_key);
        assert_send(&fut); // compile-time check: usable from tauri::async_runtime::spawn
        drop(fut);
    }

    #[test]
    fn key_validation() {
        assert!(is_valid_key(KEY));
        assert!(!is_valid_key("short"));
        assert!(!is_valid_key(&"g".repeat(64)));
    }

    #[tokio::test]
    async fn fresh_install_creates_encrypted_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        let asked = Arc::new(AtomicI8::new(-1));
        let a2 = asked.clone();
        let pool = open_database(&path, &move |allow| { a2.store(allow as i8, Ordering::SeqCst); Ok(KEY.to_string()) }).await.unwrap();
        sqlx::query("CREATE TABLE t (v TEXT)").execute(&pool).await.unwrap();
        pool.close().await;
        assert_eq!(asked.load(Ordering::SeqCst), 1);
        assert!(!is_plaintext_sqlite(&path).unwrap());
        assert!(!with_suffix(&path, ".plaintext.bak").exists());
    }

    #[tokio::test]
    async fn plaintext_database_is_converted_with_verified_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        make_plain(&path, "hello-secret").await;
        let original = fs::read(&path).unwrap();

        let pool = open_database(&path, &|_| Ok(KEY.to_string())).await.unwrap();
        pool.close().await;

        assert!(!is_plaintext_sqlite(&path).unwrap());
        assert_eq!(fs::read(with_suffix(&path, ".plaintext.bak")).unwrap(), original);
        assert!(!fs::read(&path).unwrap().windows(12).any(|w| w == b"hello-secret"));
        assert!(!with_suffix(&path, ".enc.tmp").exists());
        assert_eq!(read_back(&path, KEY).await, ("hello-secret".to_string(), 5));
    }

    #[tokio::test]
    async fn wrong_key_is_refused_and_nothing_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        make_plain(&path, "s").await;
        open_database(&path, &|_| Ok(KEY.to_string())).await.unwrap().close().await;
        fs::remove_file(with_suffix(&path, ".plaintext.bak")).unwrap();
        let before = snapshot(dir.path());

        let asked = Arc::new(AtomicI8::new(-1));
        let a2 = asked.clone();
        let res = open_database(&path, &move |allow| { a2.store(allow as i8, Ordering::SeqCst); Ok(OTHER.to_string()) }).await;
        let err = res.err().expect("wrong key must be rejected").to_string();

        assert_eq!(asked.load(Ordering::SeqCst), 0, "an existing encrypted db must never trigger key creation");
        assert!(err.contains("could not be opened"), "{err}");
        assert_eq!(snapshot(dir.path()), before, "files must be byte-for-byte unchanged and no new files created");
    }

    #[tokio::test]
    async fn missing_key_for_existing_encrypted_database_fails_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        make_plain(&path, "s").await;
        open_database(&path, &|_| Ok(KEY.to_string())).await.unwrap().close().await;
        fs::remove_file(with_suffix(&path, ".plaintext.bak")).unwrap();
        let before = snapshot(dir.path());

        let res = open_database(&path, &|_| Err("key not found".to_string())).await;
        assert!(res.is_err());
        assert_eq!(snapshot(dir.path()), before);
    }

    #[tokio::test]
    async fn interrupted_conversion_resumes_safely() {
        // State left by a crash after the backup was written but before the final rename:
        // plaintext original in place, identical backup, and a half-written temp file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        make_plain(&path, "keep-me").await;
        fs::copy(&path, with_suffix(&path, ".plaintext.bak")).unwrap();
        fs::write(with_suffix(&path, ".enc.tmp"), b"partial garbage").unwrap();

        open_database(&path, &|_| Ok(KEY.to_string())).await.unwrap().close().await;

        assert!(!is_plaintext_sqlite(&path).unwrap());
        assert!(!with_suffix(&path, ".enc.tmp").exists());
        assert_eq!(read_back(&path, KEY).await.0, "keep-me");
    }

    #[tokio::test]
    async fn conflicting_backup_blocks_conversion_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        make_plain(&path, "current").await;
        fs::write(with_suffix(&path, ".plaintext.bak"), b"some other file").unwrap();
        let before = snapshot(dir.path());

        let res = open_database(&path, &|_| Ok(KEY.to_string())).await;
        assert!(res.is_err());
        assert_eq!(snapshot(dir.path()), before);
        assert!(is_plaintext_sqlite(&path).unwrap());
    }

    #[tokio::test]
    async fn missing_database_next_to_backup_is_not_silently_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        fs::write(with_suffix(&path, ".plaintext.bak"), b"precious").unwrap();
        let res = open_database(&path, &|_| Ok(KEY.to_string())).await;
        assert!(res.is_err());
        assert!(!path.exists(), "no new empty database may be created");
    }

    #[tokio::test]
    async fn conversion_verifies_row_counts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        {
            let mut c = SqliteConnectOptions::new().filename(&path).create_if_missing(true).connect().await.unwrap();
            sqlx::query("CREATE TABLE a (x INTEGER)").execute(&mut c).await.unwrap();
            sqlx::query("CREATE TABLE \"weird name\" (y INTEGER)").execute(&mut c).await.unwrap();
            for i in 0..50 { sqlx::query("INSERT INTO a VALUES (?)").bind(i).execute(&mut c).await.unwrap(); }
            sqlx::query("INSERT INTO \"weird name\" VALUES (1)").execute(&mut c).await.unwrap();
            c.close().await.unwrap();
        }
        encrypt_in_place(&path, KEY).await.unwrap();
        let counts = verify_key(&path, KEY).await.unwrap();
        assert_eq!(counts, vec![("a".to_string(), 50), ("weird name".to_string(), 1)]);
    }
}
