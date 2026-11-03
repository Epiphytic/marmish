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
    marmish_engine::initialize_storage().map_err(|_| Error::Storage("sqlcipher_init"))?;
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
/// Build-only preservation check. Caller retains the profile lease and excludes
/// live owners/workers; an external startup is reaped before reacquisition.
#[cfg(any(test, feature = "installed-checks"))]
pub(crate) struct WalSnapshot {
    path: std::path::PathBuf,
    parts: Vec<Option<(u64, [u8; 32])>>,
    entries: std::collections::BTreeSet<std::ffi::OsString>,
}
#[cfg(any(test, feature = "installed-checks"))]
impl WalSnapshot {
    fn part(path: &Path) -> Result<Option<(u64, [u8; 32])>> {
        use sha2::Digest;
        use std::io::Read;
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(_) => {}
        }
        let f = crate::identity::open_private(path, false)?;
        let n = f.metadata()?.len();
        if n > 64 * 1024 * 1024 {
            return Err(Error::Invalid("lifecycle_inventory"));
        }
        let mut hash = Sha256::new();
        let mut block = [0u8; 8192];
        let mut reader = f.take(n + 1);
        let mut consumed = 0u64;
        loop {
            let n = reader.read(&mut block)?;
            if n == 0 {
                break;
            }
            consumed += n as u64;
            hash.update(&block[..n]);
        }
        if consumed != n {
            return Err(Error::Invalid("lifecycle_inventory"));
        }
        Ok(Some((n, hash.finalize().into())))
    }
    fn names(path: &Path) -> Result<std::collections::BTreeSet<std::ffi::OsString>> {
        let parent = path.parent().ok_or(Error::Invalid("lifecycle_inventory"))?;
        crate::identity::check_dir(parent)?;
        fs::read_dir(parent)?.map(|e| Ok(e?.file_name())).collect()
    }
    pub(crate) fn capture(path: &Path) -> Result<Self> {
        let mut parts = Vec::new();
        for suffix in ["", "-wal", "-shm"] {
            let mut p = path.as_os_str().to_os_string();
            p.push(suffix);
            parts.push(Self::part(Path::new(&p))?);
        }
        if parts[0].is_none() {
            return Err(Error::Invalid("lifecycle_inventory"));
        }
        Ok(Self {
            path: path.into(),
            parts,
            entries: Self::names(path)?,
        })
    }
    fn verify_parts(&self) -> Result<[bool; 2]> {
        let mut created = [false; 2];
        for (i, suffix) in ["", "-wal", "-shm"].into_iter().enumerate() {
            let mut p = self.path.as_os_str().to_os_string();
            p.push(suffix);
            let actual = Self::part(Path::new(&p))?;
            if actual == self.parts[i] {
                continue;
            }
            if i == 0
                || self.parts[i].is_some()
                || !matches!(actual,Some((n,_)) if n==if i==1 {0} else {32768})
            {
                return Err(Error::Invalid("lifecycle_inventory"));
            }
            created[i - 1] = true;
        }
        Ok(created)
    }
    pub(crate) fn verify_all(snapshots: &[Self]) -> Result<Vec<[bool; 2]>> {
        let mut parents = std::collections::BTreeMap::new();
        let mut results = Vec::new();
        for snapshot in snapshots {
            let created = snapshot.verify_parts()?;
            let parent = snapshot
                .path
                .parent()
                .ok_or(Error::Invalid("lifecycle_inventory"))?
                .to_path_buf();
            let allowed = parents
                .entry(parent)
                .or_insert_with(|| snapshot.entries.clone());
            for (index, exists) in created.into_iter().enumerate() {
                if exists {
                    let mut name = snapshot.path.file_name().unwrap().to_os_string();
                    name.push(if index == 0 { "-wal" } else { "-shm" });
                    allowed.insert(name);
                }
            }
            results.push(created);
        }
        for (parent, expected) in parents {
            if Self::names(&parent.join("unused"))? != expected {
                return Err(Error::Invalid("lifecycle_inventory"));
            }
        }
        Ok(results)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Real owner creation enables SQLCipher WAL in both upstream MDK and the
    /// native journal. Check the databases independently after every owner has
    /// closed; the older future-format fixture deliberately uses rollback mode.
    #[tokio::test]
    async fn real_closed_owner_wal_preflight_preserves_main_and_sidecar_inventory() {
        use crate::identity::BackendChoice;
        use crate::transport::{ArtiDirs, RelayConnector, RelayPolicy, RouteMode, TransportLimits};
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        fn inventory(path: &Path) -> Vec<Option<Vec<u8>>> {
            ["", "-wal", "-shm"]
                .into_iter()
                .map(|suffix| {
                    let mut name = path.as_os_str().to_os_string();
                    name.push(suffix);
                    match fs::symlink_metadata(&name) {
                        Ok(meta) => {
                            assert!(meta.is_file() && !meta.file_type().is_symlink());
                            Some(fs::read(name).unwrap())
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                        Err(_) => panic!("private inventory failed"),
                    }
                })
                .collect()
        }

        let parent = fs::canonicalize(std::env::var_os("HOME").unwrap()).unwrap();
        let root = tempfile::Builder::new()
            .prefix("marmish-real-wal-preflight-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(parent)
            .unwrap();
        let mut profile = ProfileLease::acquire(root.path(), "wal-proof").unwrap();
        let material = profile
            .prepare(
                BackendChoice::File,
                &std::env::current_exe().unwrap(),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let connector = RelayConnector::new(
            RelayPolicy::parse(
                &["wss://127.0.0.1:9/".into()],
                RouteMode::Off,
                TransportLimits::default(),
            )
            .unwrap(),
            ArtiDirs {
                state: profile.arti_state().into(),
                cache: profile.arti_cache().into(),
            },
        )
        .unwrap();
        let mut owner =
            crate::mdk_adapter::MdkOwner::open(&profile, &material, connector, vec![]).unwrap();
        let hk = Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), material.sqlcipher_root());
        for (path, info) in [
            (profile.mdk_database(), b"mdk".as_slice()),
            (profile.journal_database(), b"journal".as_slice()),
        ] {
            let mut key = Zeroizing::new([0; 32]);
            hk.expand(info, key.as_mut()).unwrap();
            let conn = open(path, &key).unwrap().unwrap();
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
            drop(conn);
        }
        let engine_material = crate::command_identity::derive(
            profile.profile_id(),
            material.public_key().to_bytes(),
            material.sqlcipher_root(),
        )
        .unwrap();
        let worker = root.path().join("worker");
        fs::copy(std::env::current_exe().unwrap(), &worker).unwrap();
        fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
        let engine = marmish_engine::Engine::open_with_master(
            marmish_engine::EnginePolicy {
                state_root: profile.root().join("commands"),
                worker_executable: worker,
                server: marmish_engine::ServerId(material.public_key().to_bytes()),
                allowed_principals: vec![marmish_engine::PrincipalId([9; 32])],
                allow_shell_command: false,
                command_policy: None,
                cwd: Some(root.path().into()),
                limits: Default::default(),
            },
            engine_material.binding,
            marmish_engine::ProvidedMaster::new(*engine_material.master),
        )
        .unwrap();
        drop(engine);
        owner
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await
            .unwrap();
        drop(owner);
        // Snapshot after every owner and mode probe closes. Main databases and
        // existing sidecars must stay exact; only absent empty WAL/SHM auxiliaries
        // may be created under the approved preservation predicate.
        let mdk_before = inventory(profile.mdk_database());
        let journal_before = inventory(profile.journal_database());
        let snapshots = [
            profile.mdk_database().to_path_buf(),
            profile.journal_database().to_path_buf(),
            profile.root().join("commands/engine.sqlite3"),
        ]
        .iter()
        .map(|p| WalSnapshot::capture(p).unwrap())
        .collect::<Vec<_>>();
        check(&profile, &material).unwrap();
        let auxiliary = WalSnapshot::verify_all(&snapshots).unwrap();
        assert_eq!(auxiliary.len(), 3);
        let mdk_after = inventory(profile.mdk_database());
        let journal_after = inventory(profile.journal_database());
        for (label, before, after) in [
            ("mdk", &mdk_before, &mdk_after),
            ("journal", &journal_before, &journal_after),
        ] {
            for (index, (old, new)) in before.iter().zip(after).enumerate() {
                eprintln!(
                    "wal_inventory database={label} component={index} before_bytes={:?} after_bytes={:?} unchanged={}",
                    old.as_ref().map(Vec::len),
                    new.as_ref().map(Vec::len),
                    old == new
                );
            }
        }
        profile.cleanup_runtime().unwrap();
        assert_eq!(mdk_before[0], mdk_after[0]);
        assert_eq!(journal_before[0], journal_after[0]);

        // Real read-only SQLite transactions keep genuine WAL sidecars present.
        // Unlike the absent-auxiliary case above, every preexisting sidecar byte
        // must now survive another preflight unchanged, including engine state.
        let mut state_key = Zeroizing::new([0; 32]);
        Hkdf::<Sha256>::new(
            Some(b"marmish-storage-hkdf-v1"),
            engine_material.master.as_ref(),
        )
        .expand(b"marmish-storage-key-v1\0state\0", state_key.as_mut())
        .unwrap();
        let mut readers = Vec::new();
        for (path, info) in [
            (
                profile.mdk_database().to_path_buf(),
                Some(b"mdk".as_slice()),
            ),
            (
                profile.journal_database().to_path_buf(),
                Some(b"journal".as_slice()),
            ),
            (profile.root().join("commands/engine.sqlite3"), None),
        ] {
            let mut key = Zeroizing::new([0; 32]);
            if let Some(info) = info {
                hk.expand(info, key.as_mut()).unwrap();
            } else {
                key.copy_from_slice(state_key.as_ref());
            }
            let conn = open(&path, &key).unwrap().unwrap();
            conn.execute_batch("BEGIN; SELECT count(*) FROM sqlite_master;")
                .unwrap();
            let mode: String = conn
                .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                .unwrap();
            assert_eq!(mode, "wal");
            let before = inventory(&path);
            assert!(before[1].is_some() && before[2].is_some());
            readers.push((conn, path, before));
        }
        let snapshots = readers
            .iter()
            .map(|(_, path, _)| WalSnapshot::capture(path).unwrap())
            .collect::<Vec<_>>();
        check(&profile, &material).unwrap();
        assert_eq!(
            WalSnapshot::verify_all(&snapshots).unwrap(),
            vec![[false, false]; 3]
        );
        for (conn, path, before) in readers {
            assert_eq!(inventory(&path), before, "preexisting WAL reader inventory");
            conn.execute_batch("ROLLBACK").unwrap();
        }
    }
    #[test]
    fn wal_inventory_allows_only_new_private_empty_auxiliaries() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let write = |name: &str, bytes: &[u8]| {
            let p = dir.path().join(name);
            fs::write(&p, bytes).unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
            p
        };
        let a = write("a", b"encrypted-main-a");
        let b = write("b", b"encrypted-main-b");
        let snapshots = vec![
            WalSnapshot::capture(&a).unwrap(),
            WalSnapshot::capture(&b).unwrap(),
        ];
        let wal = write("a-wal", b"");
        let shm = write("b-shm", &vec![0; 32768]);
        assert_eq!(
            WalSnapshot::verify_all(&snapshots).unwrap(),
            vec![[true, false], [false, true]]
        );
        fs::write(&wal, b"x").unwrap();
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
        fs::write(&wal, b"").unwrap();
        fs::set_permissions(&shm, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
        fs::set_permissions(&shm, fs::Permissions::from_mode(0o600)).unwrap();
        let extra = write("a-journal", b"");
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
        fs::remove_file(extra).unwrap();
        fs::remove_file(&wal).unwrap();
        std::os::unix::fs::symlink(&a, &wal).unwrap();
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
        fs::remove_file(&wal).unwrap();
        fs::hard_link(&a, &wal).unwrap();
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
        fs::remove_file(&wal).unwrap();
        fs::write(&a, b"changed").unwrap();
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
    }
    #[test]
    fn preexisting_sidecar_mutation_is_never_auxiliary_creation() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let main = dir.path().join("db");
        let wal = dir.path().join("db-wal");
        for p in [&main, &wal] {
            fs::write(p, b"original").unwrap();
            fs::set_permissions(p, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let snapshots = vec![WalSnapshot::capture(&main).unwrap()];
        fs::write(&wal, b"").unwrap();
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
        fs::remove_file(&wal).unwrap();
        assert!(WalSnapshot::verify_all(&snapshots).is_err());
    }
    #[test]
    fn version_mismatch_is_distinct_from_corruption_and_readonly() {
        marmish_engine::initialize_storage().unwrap();
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
        marmish_engine::initialize_storage().unwrap();
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
