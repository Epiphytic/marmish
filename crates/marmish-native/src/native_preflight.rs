//! Read-only compatibility checks before any upstream migration or engine recovery.
use crate::identity::{IdentityMaterial, ProfileLease};
use crate::{Error, Result};
use hkdf::Hkdf;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::Sha256;
use std::fs;
use std::path::Path;
use zeroize::Zeroizing;

// Exact account migration registry in pinned MDK8abc51befbf71a1ae3bdbb25eb0860c731e75db2.
// Upstream exposes no read-only registry API. Updating the pin requires reviewing this bound.
const MDK_MAX_MIGRATION: i64 = 166;
fn db(_: rusqlite::Error) -> Error {
    Error::Storage("state_preflight")
}
fn open(path: &Path, key: &[u8; 32]) -> Result<Option<Connection>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let file = crate::identity::open_private(path, false)?;
    if file.metadata()?.len() == 0 {
        return Ok(None);
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(db)?;
    let mut raw = Zeroizing::new(Vec::with_capacity(67));
    raw.extend_from_slice(b"x'");
    let mut hexkey = Zeroizing::new([0u8; 64]);
    hex::encode_to_slice(key, hexkey.as_mut()).map_err(|_| Error::Storage("state_preflight"))?;
    raw.extend_from_slice(hexkey.as_ref());
    raw.push(b'\'');
    if unsafe { rusqlite::ffi::sqlite3_key(conn.handle(), raw.as_ptr().cast(), raw.len() as i32) }
        != rusqlite::ffi::SQLITE_OK
    {
        return Err(Error::Storage("state_preflight"));
    }
    conn.execute_batch("PRAGMA cipher_compatibility=4; PRAGMA cipher_memory_security=ON; PRAGMA cipher_page_size=4096; PRAGMA query_only=ON;").map_err(db)?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .map_err(db)?;
    Ok(Some(conn))
}
fn user_version(conn: &Connection, supported: &[i64]) -> Result<()> {
    let n: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db)?;
    if !supported.contains(&n) {
        return Err(Error::Storage("incompatible_state"));
    }
    Ok(())
}
pub(crate) fn check(profile: &ProfileLease, material: &IdentityMaterial) -> Result<()> {
    let hk = Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), material.sqlcipher_root());
    let mut mdk = Zeroizing::new([0; 32]);
    let mut journal = Zeroizing::new([0; 32]);
    hk.expand(b"mdk", mdk.as_mut())
        .map_err(|_| Error::Storage("state_preflight"))?;
    hk.expand(b"journal", journal.as_mut())
        .map_err(|_| Error::Storage("state_preflight"))?;
    if let Some(conn) = open(profile.mdk_database(), &mdk)? {
        let table: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='cgka_schema_migrations'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        if table.is_some() {
            let mut rows = conn
                .prepare("SELECT version FROM cgka_schema_migrations ORDER BY version")
                .map_err(db)?;
            for row in rows.query_map([], |r| r.get::<_, i64>(0)).map_err(db)? {
                let n = row.map_err(db)?;
                if !(1..=MDK_MAX_MIGRATION).contains(&n) {
                    return Err(Error::Storage("incompatible_state"));
                }
            }
        } else {
            let tables: i64 = conn
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
                .map_err(db)?;
            if tables != 0 {
                return Err(Error::Storage("state_preflight"));
            }
        }
    }
    // Command ledger shares the journal schema/migration boundary; there is no separate database.
    if let Some(conn) = open(profile.journal_database(), &journal)? {
        user_version(&conn, crate::journal::JOURNAL_SUPPORTED_FORMATS)?;
    }
    let engine = crate::command_identity::derive(
        profile.profile_id(),
        material.public_key().to_bytes(),
        material.sqlcipher_root(),
    )?;
    let mut state = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish-storage-hkdf-v1"), engine.master.as_ref())
        .expand(b"marmish-storage-key-v1\0state\0", state.as_mut())
        .map_err(|_| Error::Storage("state_preflight"))?;
    let engine_dir = profile.root().join("commands");
    match fs::symlink_metadata(&engine_dir) {
        Ok(_) => crate::identity::check_dir(&engine_dir)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    if let Some(conn) = open(&engine_dir.join("engine.sqlite3"), &state)? {
        user_version(&conn, &[0, marmish_engine::ENGINE_FORMAT])?;
    }
    let config = profile.root().join("config.json");
    match fs::symlink_metadata(config) {
        Ok(_) => match crate::command_config::CommandConfig::load(profile.root()) {
            Err(Error::Invalid("config_version")) => {
                return Err(Error::Storage("incompatible_state"));
            }
            result => {
                result?;
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn version_mismatch_is_distinct_from_corruption_and_readonly() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA user_version=99;").unwrap();
        assert!(matches!(
            user_version(&conn, &[0, 1]),
            Err(Error::Storage("incompatible_state"))
        ));
        assert!(user_version(&conn, &[99]).is_ok());
    }
    #[test]
    fn encrypted_future_and_corruption_probe_preserves_bytes_and_files() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::Builder::new()
            .prefix("marmish-preflight-test-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::var_os("HOME").unwrap())
            .unwrap();
        let path = root.path().join("state.sqlite3");
        let key = [37u8; 32];
        let conn = Connection::open(&path).unwrap();
        let raw = format!("x'{}'", hex::encode(key));
        assert_eq!(
            unsafe {
                rusqlite::ffi::sqlite3_key(conn.handle(), raw.as_ptr().cast(), raw.len() as i32)
            },
            rusqlite::ffi::SQLITE_OK
        );
        conn.execute_batch("PRAGMA cipher_compatibility=4; PRAGMA user_version=99; CREATE TABLE proof(value INTEGER);").unwrap();
        drop(conn);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let before = fs::read(&path).unwrap();
        let conn = open(&path, &key).unwrap().unwrap();
        assert!(matches!(
            user_version(&conn, &[0, 1]),
            Err(Error::Storage("incompatible_state"))
        ));
        drop(conn);
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        fs::write(&path, b"corrupted encrypted database").unwrap();
        assert!(matches!(
            open(&path, &key),
            Err(Error::Storage("state_preflight"))
        ));
        assert_eq!(fs::read(&path).unwrap(), b"corrupted encrypted database");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
