//! Winthorpe's own credential store — SQLite-backed, DPAPI-encrypted at rest
//! on Windows, plain rows on Unix (encryption layer for Unix is Phase 9+).
//!
//! ## Why SQLite over the OS keystore
//!
//! Per maintainer directive: "sqlite for credentials and application settings
//! etc". Rationale (paraphrased):
//!   - Single-file portable across machines (user can copy ~/.../winthorpe.db
//!     and have everything they need).
//!   - Inspectable (sqlite3 CLI works against the file directly).
//!   - One backup story for app data + secrets.
//!   - Avoids OS-keystore prompts that interrupt the agent loop.
//!
//! At-rest encryption stays platform-native:
//!   - **Windows:** DPAPI per-user (`CryptProtectData` / `CryptUnprotectData`).
//!     Encrypted blob is bound to the current Windows user account; another
//!     user on the same machine cannot decrypt.
//!   - **macOS / Linux:** stored unencrypted for now. The data dir lives under
//!     the user's home with default file-mode 0700, so OS-level isolation does
//!     the job. A libsodium/age layer can be added in a Phase 9 follow-up if
//!     the maintainer wants stronger guarantees against accidental backup
//!     exposure.
//!
//! ## Schema
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS credentials (
//!     namespace   TEXT NOT NULL,         -- e.g. "github", "anthropic_api"
//!     name        TEXT NOT NULL,         -- e.g. "default", "personal"
//!     ciphertext  BLOB NOT NULL,         -- DPAPI(plaintext) on Windows; plaintext on Unix
//!     metadata    TEXT,                  -- optional JSON sidecar (expires_at, scopes, ...)
//!     created_at  INTEGER NOT NULL,
//!     updated_at  INTEGER NOT NULL,
//!     PRIMARY KEY (namespace, name)
//! );
//! ```
//!
//! ## API
//!
//! - `CredentialStore::new(conn)`        — get a handle bound to the app DB
//! - `store.put(ns, name, secret, meta)` — upsert
//! - `store.get(ns, name)`               — read + decrypt
//! - `store.list(ns)`                    — names + metadata only (no plaintext)
//! - `store.delete(ns, name)`            — wipe
//!
//! All operations take `&self` so the store is freely cloneable / shareable.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use std::sync::Arc;

pub struct CredentialStore {
    pool: Arc<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>>,
}

#[derive(Debug, Clone)]
pub struct CredentialRecord {
    pub namespace: String,
    pub name: String,
    pub secret: Vec<u8>,
    pub metadata: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct CredentialSummary {
    pub namespace: String,
    pub name: String,
    pub metadata: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl CredentialStore {
    /// Wrap an existing connection pool. The schema is initialised lazily on
    /// first use; callers don't need to run migrations explicitly.
    pub fn new(pool: Arc<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>>) -> Result<Self> {
        let conn = pool.get().context("Failed to acquire SQLite connection")?;
        conn.execute_batch(SCHEMA_SQL)
            .context("Failed to initialise credentials schema")?;
        Ok(Self { pool })
    }

    pub fn put(
        &self,
        namespace: &str,
        name: &str,
        secret: &[u8],
        metadata: Option<&str>,
    ) -> Result<()> {
        let ciphertext = encrypt_at_rest(secret)?;
        let now = Utc::now().timestamp_millis();
        let conn = self.pool.get().context("Failed to acquire SQLite connection")?;
        conn.execute(
            r#"
            INSERT INTO credentials (namespace, name, ciphertext, metadata, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            ON CONFLICT(namespace, name) DO UPDATE SET
                ciphertext = excluded.ciphertext,
                metadata   = excluded.metadata,
                updated_at = excluded.updated_at
            "#,
            params![namespace, name, ciphertext, metadata, now],
        )
        .with_context(|| format!("Failed to upsert credential {namespace}/{name}"))?;
        Ok(())
    }

    pub fn get(&self, namespace: &str, name: &str) -> Result<Option<CredentialRecord>> {
        let conn = self.pool.get().context("Failed to acquire SQLite connection")?;
        let row = conn
            .query_row(
                r#"
                SELECT ciphertext, metadata, created_at, updated_at
                  FROM credentials
                 WHERE namespace = ?1 AND name = ?2
                "#,
                params![namespace, name],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;

        let Some((ciphertext, metadata, created_at, updated_at)) = row else {
            return Ok(None);
        };

        let secret = decrypt_at_rest(&ciphertext)?;
        Ok(Some(CredentialRecord {
            namespace: namespace.to_string(),
            name: name.to_string(),
            secret,
            metadata,
            created_at,
            updated_at,
        }))
    }

    pub fn list(&self, namespace: &str) -> Result<Vec<CredentialSummary>> {
        let conn = self.pool.get().context("Failed to acquire SQLite connection")?;
        let mut stmt = conn.prepare(
            r#"
            SELECT name, metadata, created_at, updated_at
              FROM credentials
             WHERE namespace = ?1
             ORDER BY updated_at DESC
            "#,
        )?;
        let rows = stmt
            .query_map(params![namespace], |row| {
                Ok(CredentialSummary {
                    namespace: namespace.to_string(),
                    name: row.get::<_, String>(0)?,
                    metadata: row.get::<_, Option<String>>(1)?,
                    created_at: row.get::<_, i64>(2)?,
                    updated_at: row.get::<_, i64>(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn delete(&self, namespace: &str, name: &str) -> Result<bool> {
        let conn = self.pool.get().context("Failed to acquire SQLite connection")?;
        let affected = conn.execute(
            "DELETE FROM credentials WHERE namespace = ?1 AND name = ?2",
            params![namespace, name],
        )?;
        Ok(affected > 0)
    }
}

const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS credentials (
    namespace   TEXT NOT NULL,
    name        TEXT NOT NULL,
    ciphertext  BLOB NOT NULL,
    metadata    TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (namespace, name)
);
CREATE INDEX IF NOT EXISTS credentials_namespace_idx ON credentials(namespace);
"#;

// ---------------------------------------------------------------------------
// At-rest encryption
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn encrypt_at_rest(plaintext: &[u8]) -> Result<Vec<u8>> {
    dpapi::protect(plaintext)
}

#[cfg(windows)]
fn decrypt_at_rest(ciphertext: &[u8]) -> Result<Vec<u8>> {
    dpapi::unprotect(ciphertext)
}

#[cfg(not(windows))]
fn encrypt_at_rest(plaintext: &[u8]) -> Result<Vec<u8>> {
    // No-op on Unix; rely on data-dir filesystem permissions. Phase 9+ can
    // add libsodium/age here.
    Ok(plaintext.to_vec())
}

#[cfg(not(windows))]
fn decrypt_at_rest(ciphertext: &[u8]) -> Result<Vec<u8>> {
    Ok(ciphertext.to_vec())
}

#[cfg(windows)]
mod dpapi {
    //! Thin wrapper over Windows DPAPI (`CryptProtectData` / `CryptUnprotectData`).
    //!
    //! `CRYPTPROTECT_LOCAL_MACHINE` is **not** set, so the encryption is bound
    //! to the current Windows user account. Another local user on the same
    //! machine cannot decrypt; nor can a roaming/imaged copy of the same
    //! account on a different machine (DPAPI uses machine-bound key material).
    //!
    //! No additional entropy / "salt" parameter is used. A future hardening
    //! pass could derive a per-app secondary entropy from the data-dir path
    //! to defeat cross-app attacks within the same user account, but that
    //! also makes credential migration painful and is over-rotated for our
    //! threat model (single-user dev machine, not a multi-tenant service).

    use anyhow::{anyhow, Context, Result};
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB,
    };

    pub fn protect(plaintext: &[u8]) -> Result<Vec<u8>> {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: plaintext.len() as u32,
            pbData: plaintext.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &in_blob,
                None,
                None,
                None,
                None,
                0,
                &mut out_blob,
            )
            .context("CryptProtectData failed")?;
        }
        Ok(blob_to_vec_freeing(out_blob))
    }

    pub fn unprotect(ciphertext: &[u8]) -> Result<Vec<u8>> {
        let in_blob = CRYPT_INTEGER_BLOB {
            cbData: ciphertext.len() as u32,
            pbData: ciphertext.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &in_blob,
                None,
                None,
                None,
                None,
                0,
                &mut out_blob,
            )
            .context("CryptUnprotectData failed")?;
        }
        Ok(blob_to_vec_freeing(out_blob))
    }

    /// Copy a CRYPT_INTEGER_BLOB into a Vec and free the kernel-owned buffer.
    /// DPAPI mandates LocalFree on the returned pbData.
    fn blob_to_vec_freeing(blob: CRYPT_INTEGER_BLOB) -> Vec<u8> {
        if blob.pbData.is_null() || blob.cbData == 0 {
            return Vec::new();
        }
        let slice = unsafe { std::slice::from_raw_parts(blob.pbData, blob.cbData as usize) };
        let owned = slice.to_vec();
        unsafe {
            let _ = LocalFree(Some(HLOCAL(blob.pbData as *mut _)));
        }
        owned
    }

    /// Self-test entry used by integration tests.
    #[allow(dead_code)]
    pub fn self_test() -> Result<()> {
        let plaintext = b"winthorpe-dpapi-self-test-payload";
        let ct = protect(plaintext)?;
        if ct == plaintext {
            return Err(anyhow!("DPAPI ciphertext equals plaintext (encryption did nothing)"));
        }
        let pt = unprotect(&ct)?;
        if pt != plaintext {
            return Err(anyhow!(
                "DPAPI roundtrip mismatch: expected {plaintext:?}, got {pt:?}"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use r2d2_sqlite::SqliteConnectionManager;
    use std::sync::Arc;

    fn in_memory_pool() -> Arc<r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>> {
        let manager = SqliteConnectionManager::memory();
        Arc::new(r2d2::Pool::builder().max_size(1).build(manager).unwrap())
    }

    #[test]
    fn put_then_get_roundtrips_secret() {
        let store = CredentialStore::new(in_memory_pool()).unwrap();
        store.put("github", "default", b"my-token", Some(r#"{"scope":"repo"}"#)).unwrap();
        let got = store.get("github", "default").unwrap().unwrap();
        assert_eq!(got.secret, b"my-token");
        assert_eq!(got.metadata.as_deref(), Some(r#"{"scope":"repo"}"#));
    }

    #[test]
    fn put_upserts_existing_record() {
        let store = CredentialStore::new(in_memory_pool()).unwrap();
        store.put("anthropic", "default", b"v1", None).unwrap();
        store.put("anthropic", "default", b"v2", None).unwrap();
        let got = store.get("anthropic", "default").unwrap().unwrap();
        assert_eq!(got.secret, b"v2");
    }

    #[test]
    fn get_returns_none_for_missing() {
        let store = CredentialStore::new(in_memory_pool()).unwrap();
        assert!(store.get("noone", "nothing").unwrap().is_none());
    }

    #[test]
    fn list_is_filtered_by_namespace_and_excludes_secret() {
        let store = CredentialStore::new(in_memory_pool()).unwrap();
        store.put("github", "a", b"sa", None).unwrap();
        store.put("github", "b", b"sb", None).unwrap();
        store.put("anthropic", "default", b"sk", None).unwrap();

        let github = store.list("github").unwrap();
        assert_eq!(github.len(), 2);
        let names: Vec<_> = github.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"a"));
        assert!(names.contains(&"b"));
    }

    #[test]
    fn delete_removes_and_returns_true_when_present() {
        let store = CredentialStore::new(in_memory_pool()).unwrap();
        store.put("ns", "n", b"v", None).unwrap();
        assert!(store.delete("ns", "n").unwrap());
        assert!(!store.delete("ns", "n").unwrap());
        assert!(store.get("ns", "n").unwrap().is_none());
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip_works() {
        super::dpapi::self_test().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn put_get_uses_dpapi_ciphertext_at_rest() {
        // After put(), the on-disk row should NOT equal plaintext on Windows.
        let pool = in_memory_pool();
        let store = CredentialStore::new(pool.clone()).unwrap();
        store.put("ns", "n", b"plaintext-marker-12345", None).unwrap();

        let conn = pool.get().unwrap();
        let raw: Vec<u8> = conn
            .query_row(
                "SELECT ciphertext FROM credentials WHERE namespace='ns' AND name='n'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_ne!(raw, b"plaintext-marker-12345", "DPAPI did not encrypt");

        // Round-trip via the store API still returns plaintext.
        let got = store.get("ns", "n").unwrap().unwrap();
        assert_eq!(got.secret, b"plaintext-marker-12345");
    }
}
