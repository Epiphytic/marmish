use super::*;
use cgka_traits::storage::MaintenanceStorage;
use hkdf::Hkdf;
use sha2::Sha256;
use std::{cell::Cell, path::PathBuf};
use storage_sqlite::{SqlCipherKey, SqliteAccountStorage};
use zeroize::Zeroizing;

#[test]
#[ignore = "uses fresh processes and actual encrypted account storage"]
fn peer_candidate_mdk_owner_qualification() {
    run_registration_cases(&[
        "mdk_explicit_close",
        "mdk_last_drop",
        "mdk_wrong_key",
        "mdk_before_open",
        "mdk_inside_raw_open",
        "mdk_during_progress",
        "mdk_before_close",
        "mdk_before_reopen",
        "mdk_live_root",
    ]);
}

pub(super) fn qualify(
    mode: &str,
    lease: &Arc<ProfileLease>,
    material: &IdentityMaterial,
    admission: &Admission,
    fatal: &Fatal,
) {
    let path = lease.mdk_database();
    let baseline = evidence(path);
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(b"marmish/v2/storage"), material.sqlcipher_root())
        .expand(b"mdk", key.as_mut())
        .unwrap();
    if mode == "wrong_key" {
        key[0] ^= 1;
    }

    if mode == "before_open" {
        displace_default();
        assert!(!admission.admit());
        assert!(fatal.is_tripped());
        assert_eq!(evidence(path), baseline);
        return;
    }

    if mode == "inside_raw_open" {
        crate::peer_quota_vfs::dispatch::account_open_probe::before_main_open(displace_default);
    }
    let observed = Cell::new(false);
    assert!(admission.admit());
    let result =
        SqliteAccountStorage::open_encrypted_observed(path, &SqlCipherKey::raw(*key), &|_| {
            if !observed.replace(true) && mode == "during_progress" {
                displace_default();
            }
        });
    assert!(observed.get());
    if mode == "wrong_key" {
        assert!(result.is_err());
        assert!(admission.admit());
        assert!(!fatal.is_tripped());
        assert_eq!(evidence(path), baseline);
        return;
    }

    let root = result.unwrap();
    let closer = root.closer();
    if mode == "before_close" {
        displace_default();
    }
    if matches!(mode, "inside_raw_open" | "during_progress" | "before_close") {
        assert!(!admission.admit());
        assert!(fatal.is_tripped());
        park(root, lease);
        assert!(!closer.is_released());
        return;
    }

    assert!(admission.admit());
    assert!(!fatal.is_tripped());
    assert!(evidence(path).database_fds > 0);
    if mode == "last_drop" {
        drop(root);
    } else {
        let survivor = root.clone();
        root.close().unwrap();
        assert!(closer.is_released());
        assert!(survivor.is_closed());
        assert!(survivor.key_package_lifecycle().is_err());
        let closed = evidence(path);
        assert_eq!(closed.database_fds, 0);
        drop(survivor);
        drop(root);
        assert_eq!(evidence(path), closed);
    }
    assert!(closer.is_released());
    let closed = evidence(path);
    assert_eq!(closed.database_fds, 0);
    assert!(closed.wal.is_none() && closed.shm.is_none());

    if mode == "before_reopen" {
        displace_default();
        assert!(!admission.admit());
        assert!(fatal.is_tripped());
        assert_eq!(evidence(path), closed);
        return;
    }
    assert!(admission.admit());
    let storage = SqliteAccountStorage::open_encrypted(path, &SqlCipherKey::raw(*key)).unwrap();
    assert!(admission.admit());
    assert!(storage.key_package_lifecycle().unwrap().is_some());

    if mode == "live_root" {
        let closer = storage.closer();
        let clone = storage.clone();
        displace_default();
        assert!(!admission.admit());
        assert!(fatal.is_tripped());
        let before = evidence(path);
        closer.begin_close();
        assert!(clone.key_package_lifecycle().is_err());
        drop(clone);
        assert_eq!(evidence(path), before);
        park(storage, lease);
        assert!(!closer.is_released());
    } else {
        assert!(admission.admit());
        storage.close().unwrap();
        assert_eq!(evidence(path).database_fds, 0);
    }
}

fn park(root: SqliteAccountStorage, lease: &Arc<ProfileLease>) {
    let path = lease.mdk_database();
    let before = evidence(path);
    assert!(before.database_fds > 0);
    root.closer().begin_close();
    assert!(root.key_package_lifecycle().is_err());
    std::mem::forget((root, Arc::clone(lease)));
    assert_eq!(evidence(path), before);
}

#[derive(Debug, Eq, PartialEq)]
struct Evidence {
    database_fds: usize,
    main: [u8; 32],
    wal: Option<[u8; 32]>,
    shm: Option<[u8; 32]>,
}

fn evidence(path: &Path) -> Evidence {
    use sha2::Digest as _;
    let sidecar = |suffix: &str| {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match fs::read(PathBuf::from(name)) {
            Ok(bytes) => Some(Sha256::digest(bytes).into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("could not inspect account sidecar: {error}"),
        }
    };
    let database_fds = fs::read_dir("/proc/self/fd")
        .unwrap()
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| fs::read_link(entry.path()).ok())
        .filter(|target| target == path)
        .count();
    Evidence {
        database_fds,
        main: Sha256::digest(fs::read(path).unwrap()).into(),
        wal: sidecar("-wal"),
        shm: sidecar("-shm"),
    }
}
