//! Versioned local storage keys and one fail-closed SQLCipher open boundary.
use crate::model::*;
use hkdf::Hkdf;
use rusqlite::{Connection, OpenFlags};
use sha2::Sha256;
use std::path::Path;
use std::time::Duration;
use zeroize::Zeroizing;

pub(crate) const KEY_MAGIC: &[u8; 8] = b"MRMSKEY1";
// Intentionally neither Debug nor Clone nor serializable.
pub(crate) struct StorageKeys(pub(crate) Zeroizing<[u8; 32]>);
#[derive(Clone, Copy)]
pub(crate) enum Context {
    State,
    Spool(JobId, Stream),
}
#[derive(Clone, Copy)]
pub(crate) enum Journal {
    Wal,
    Delete,
}

pub(crate) fn db_error(error: rusqlite::Error) -> EngineError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            EngineError::Busy
        }
        _ => EngineError::StorageFailure,
    }
}

impl StorageKeys {
    fn raw_key(&self, context: Context) -> Result<Zeroizing<Vec<u8>>> {
        let mut info = b"marmish-storage-key-v1\0".to_vec();
        match context {
            Context::State => info.extend_from_slice(b"state\0"),
            Context::Spool(job, stream) => {
                info.extend_from_slice(b"spool\0");
                info.extend_from_slice(&job.0);
                info.push(match stream {
                    Stream::Stdout => 0,
                    Stream::Stderr => 1,
                });
            }
        }
        let mut derived = Zeroizing::new([0u8; 32]);
        Hkdf::<Sha256>::new(Some(b"marmish-storage-hkdf-v1"), self.0.as_ref())
            .expand(&info, derived.as_mut())
            .map_err(|_| EngineError::StorageFailure)?;
        let mut raw = Zeroizing::new(Vec::with_capacity(67));
        raw.extend_from_slice(b"x'");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in derived.iter() {
            raw.push(HEX[(byte >> 4) as usize]);
            raw.push(HEX[(byte & 15) as usize]);
        }
        raw.push(b'\'');
        Ok(raw)
    }
}

pub(crate) fn open(
    path: &Path,
    keys: &StorageKeys,
    context: Context,
    journal: Journal,
) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let conn = Connection::open_with_flags(path, flags).map_err(db_error)?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(db_error)?;
    let version: String = conn
        .query_row("PRAGMA cipher_version", [], |row| row.get(0))
        .map_err(db_error)?;
    if version.trim().is_empty() {
        return Err(EngineError::StorageFailure);
    }
    conn.execute_batch("PRAGMA cipher_compatibility=4; PRAGMA cipher_memory_security=ON; PRAGMA cipher_page_size=4096;").map_err(db_error)?;
    let raw = keys.raw_key(context)?;
    // The key is passed as data, never SQL text. Connection ownership excludes concurrent use.
    let result =
        unsafe { rusqlite::ffi::sqlite3_key(conn.handle(), raw.as_ptr().cast(), raw.len() as i32) };
    if result != rusqlite::ffi::SQLITE_OK {
        return Err(EngineError::StorageFailure);
    }
    let _: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |row| row.get(0))
        .map_err(db_error)?;
    let page_size: String = conn
        .query_row("PRAGMA cipher_page_size", [], |row| row.get(0))
        .map_err(db_error)?;
    if page_size != "4096" {
        return Err(EngineError::StorageFailure);
    }
    conn.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA synchronous=FULL;").map_err(db_error)?;
    let desired = match journal {
        Journal::Wal => "WAL",
        Journal::Delete => "DELETE",
    };
    let actual: String = conn
        .query_row(
            match journal {
                Journal::Wal => "PRAGMA journal_mode=WAL",
                Journal::Delete => "PRAGMA journal_mode=DELETE",
            },
            [],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    if !actual.eq_ignore_ascii_case(desired) {
        return Err(EngineError::StorageFailure);
    }
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::StateRoot;
    use std::os::unix::{fs::PermissionsExt, process::CommandExt};

    fn root() -> (tempfile::TempDir, StateRoot, StorageKeys) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let root = StateRoot::open(dir.path()).unwrap();
        let guard = root.lock_frontend().unwrap();
        let keys = root.initialize_storage(&guard).unwrap();
        (dir, root, keys)
    }
    #[test]
    fn actual_cipher_roundtrip_requires_key_and_context() {
        let (_dir, root, keys) = root();
        let path = root.prepare_db(true).unwrap();
        let conn = open(&path, &keys, Context::State, Journal::Wal).unwrap();
        let version: String = conn
            .query_row("PRAGMA cipher_version", [], |row| row.get(0))
            .unwrap();
        println!("Compiled SQLCipher: {version}");
        assert!(version.starts_with("4.17."));
        conn.execute_batch("CREATE TABLE probe(value BLOB); INSERT INTO probe VALUES(X'6100ff');")
            .unwrap();
        drop(conn);
        let recovered = root.load_storage_keys().unwrap();
        let conn = open(&path, &recovered, Context::State, Journal::Wal).unwrap();
        let data: Vec<u8> = conn
            .query_row("SELECT value FROM probe", [], |row| row.get(0))
            .unwrap();
        assert_eq!(data, b"a\0\xff");
        drop(conn);
        let plain = Connection::open(&path).unwrap();
        assert!(plain
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| row
                .get::<_, i64>(0))
            .is_err());
        drop(plain);
        let wrong = StorageKeys(Zeroizing::new([42; 32]));
        assert!(open(&path, &wrong, Context::State, Journal::Wal).is_err());
        assert!(open(
            &path,
            &keys,
            Context::Spool(JobId([1; 16]), Stream::Stdout),
            Journal::Wal
        )
        .is_err());
        let state = keys.raw_key(Context::State).unwrap();
        let stdout = keys
            .raw_key(Context::Spool(JobId([1; 16]), Stream::Stdout))
            .unwrap();
        let stderr = keys
            .raw_key(Context::Spool(JobId([1; 16]), Stream::Stderr))
            .unwrap();
        let other = keys
            .raw_key(Context::Spool(JobId([2; 16]), Stream::Stdout))
            .unwrap();
        assert!(*state != *stdout && *stdout != *stderr && *stdout != *other);
    }
    #[test]
    fn memory_lock_helper() {
        let Some(path) = std::env::var_os("MARMISH_CIPHER_MEMLOCK_ROOT") else {
            return;
        };
        let root = StateRoot::open_existing(Path::new(&path)).unwrap();
        let keys = root.load_storage_keys().unwrap();
        let conn = open(
            &root.prepare_db(false).unwrap(),
            &keys,
            Context::State,
            Journal::Wal,
        )
        .unwrap();
        let value: i64 = conn
            .query_row("SELECT n FROM probe", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, 7);
        conn.execute("INSERT INTO probe VALUES(8)", []).unwrap();
    }
    #[test]
    fn encryption_survives_unavailable_memory_lock() {
        let (_dir, root, keys) = root();
        let path = root.prepare_db(true).unwrap();
        let conn = open(&path, &keys, Context::State, Journal::Wal).unwrap();
        conn.execute_batch("CREATE TABLE probe(n); INSERT INTO probe VALUES(7);")
            .unwrap();
        drop(conn);
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "cipher::tests::memory_lock_helper",
                "--nocapture",
            ])
            .env("MARMISH_CIPHER_MEMLOCK_ROOT", root.path());
        unsafe {
            command.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::setrlimit(libc::RLIMIT_MEMLOCK, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        assert!(command.status().unwrap().success());
        let plain = Connection::open(&path).unwrap();
        assert!(plain
            .query_row("SELECT count(*) FROM sqlite_master", [], |row| row
                .get::<_, i64>(0))
            .is_err());
    }
}
