//! At-rest encryption for the application database (SQLCipher).
//!
//! * A random 256-bit key is generated once and stored in the OS credential store
//!   (Windows Credential Manager, macOS Keychain, Linux Secret Service).
//! * `MATCHWISE_DB_KEY` (64 hex chars) overrides the credential store. Use it on headless
//!   machines and on WSL without a running Secret Service daemon.
//! * An existing plaintext database (for example one created before encryption existed, or
//!   an imported legacy database) is converted in place; the plaintext original is kept as
//!   `<name>.plaintext.bak` until you delete it yourself.

use rand::RngCore;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{ConnectOptions, Connection, SqliteConnection};
use std::fs;
use std::io::Read;
use std::path::Path;

const KEYRING_SERVICE: &str = "com.matchwise.desktop";
const KEYRING_ACCOUNT: &str = "database-encryption-key";
const KEY_ENV: &str = "MATCHWISE_DB_KEY";
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

fn is_valid_key(k: &str) -> bool {
    k.len() == 64 && k.bytes().all(|b| b.is_ascii_hexdigit())
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

/// Fetch the database key. A new key is created only if `allow_create` is true; the caller must
/// pass false when an encrypted database already exists, so a lost key never silently
/// produces a second, unrelated key.
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

/// Convert a plaintext SQLite file to an encrypted one, keeping the original as `<path>.plaintext.bak`.
pub async fn encrypt_in_place(path: &Path, key_hex: &str) -> Result<(), sqlx::Error> {
    let tmp = path.with_extension("enc.tmp");
    let bak = {
        let mut s = path.as_os_str().to_owned();
        s.push(".plaintext.bak");
        std::path::PathBuf::from(s)
    };
    if bak.exists() {
        return Err(sqlx::Error::Protocol(format!(
            "{} already exists; remove or move it before encrypting again",
            bak.display()
        )));
    }
    let _ = fs::remove_file(&tmp);

    // create_if_missing(true) is required: ATTACH of the new encrypted file inherits the connection's
    // open flags and fails with "unable to open database" without SQLITE_OPEN_CREATE. The caller has
    // already checked that `path` exists, so this never creates the plaintext file.
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Delete);
    let mut conn: SqliteConnection = opts.connect().await?;

    let user_version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&mut conn).await?;
    let plain_tables: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master").fetch_one(&mut conn).await?;

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

    // Verify the encrypted copy opens with the key and has the same schema object count.
    let vopts = SqliteConnectOptions::new()
        .filename(&tmp)
        .create_if_missing(false)
        .pragma("key", key_pragma_value(key_hex));
    let mut vconn: SqliteConnection = vopts.connect().await?;
    let enc_tables: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_master").fetch_one(&mut vconn).await?;
    vconn.close().await?;
    if enc_tables != plain_tables {
        let _ = fs::remove_file(&tmp);
        return Err(sqlx::Error::Protocol(format!(
            "encryption verification failed: {plain_tables} schema objects before, {enc_tables} after"
        )));
    }

    // Swap files. Stale journal files belong to the plaintext database.
    for ext in ["-wal", "-shm", "-journal"] {
        let mut p = path.as_os_str().to_owned();
        p.push(ext);
        let _ = fs::remove_file(std::path::PathBuf::from(p));
    }
    fs::rename(path, &bak).map_err(sqlx::Error::Io)?;
    fs::rename(&tmp, path).map_err(sqlx::Error::Io)?;
    log::warn!(
        "Database encrypted. The plaintext original was kept at {} -- delete it once you have verified the app works.",
        bak.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn key_validation() {
        assert!(is_valid_key(KEY));
        assert!(!is_valid_key("short"));
        assert!(!is_valid_key(&"g".repeat(64)));
    }

    #[tokio::test]
    async fn converts_plaintext_database_and_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        {
            let mut c = SqliteConnectOptions::new().filename(&path).create_if_missing(true).connect().await.unwrap();
            sqlx::query("CREATE TABLE t (v TEXT)").execute(&mut c).await.unwrap();
            sqlx::query("INSERT INTO t VALUES ('hello-secret')").execute(&mut c).await.unwrap();
            sqlx::query("PRAGMA user_version = 5").execute(&mut c).await.unwrap();
            c.close().await.unwrap();
        }
        assert!(is_plaintext_sqlite(&path).unwrap());

        encrypt_in_place(&path, KEY).await.unwrap();

        assert!(!is_plaintext_sqlite(&path).unwrap());
        assert!(dir.path().join("app.sqlite.plaintext.bak").exists());
        let raw = fs::read(&path).unwrap();
        assert!(!raw.windows(12).any(|w| w == b"hello-secret"));

        let mut c = SqliteConnectOptions::new().filename(&path).pragma("key", key_pragma_value(KEY)).connect().await.unwrap();
        let v: String = sqlx::query_scalar("SELECT v FROM t").fetch_one(&mut c).await.unwrap();
        let uv: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&mut c).await.unwrap();
        assert_eq!((v.as_str(), uv), ("hello-secret", 5));
    }

    #[tokio::test]
    async fn wrong_key_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.sqlite");
        {
            let mut c = SqliteConnectOptions::new().filename(&path).create_if_missing(true).connect().await.unwrap();
            sqlx::query("CREATE TABLE t (v TEXT)").execute(&mut c).await.unwrap();
            c.close().await.unwrap();
        }
        encrypt_in_place(&path, KEY).await.unwrap();
        let other = "f".repeat(64);
        let res = async {
            let mut c = SqliteConnectOptions::new().filename(&path).pragma("key", key_pragma_value(&other)).connect().await?;
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM sqlite_master").fetch_one(&mut c).await
        }.await;
        assert!(res.is_err());
    }
}
