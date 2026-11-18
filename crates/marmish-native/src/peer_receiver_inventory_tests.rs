use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, symlink};
use std::path::Path;

use cgka_traits::app_components::NostrRoutingV1;
use nostr::event::Event;

use super::super::provisional::tests::Fixture;
use super::super::store::{
    RawRecord, TrustedOutcome,
    consent_checkpoint::{provisioning::stage_journal::FileWitness, wire},
};
use super::*;

const RELAY: &str = "wss://receiver-inventory.example/";

fn open(fixture: &Fixture) -> RecoveredOwner<'_> {
    RecoveredOwner::open(&fixture.lease, &fixture.identity, RELAY).unwrap()
}

fn write_extent(path: &Path, length: u64) -> FileWitness {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.set_len(length).unwrap();
    let stat = file.metadata().unwrap();
    FileWitness {
        device: stat.dev().to_be_bytes(),
        inode: stat.ino().to_be_bytes(),
    }
}

#[test]
fn peer_receiver_inventory_charges_unwitnessed_and_interrupted_files_across_reopen() {
    let f = Fixture::new();
    let mut owner = open(&f);
    let fresh = owner.recover(f.now).unwrap();
    assert_eq!(fresh.classes, [Class::Fenced, Class::Vacant]);
    assert_eq!(fresh.charged, RESERVED_BYTES);

    write_extent(&f.stage(), 0);
    assert_eq!(owner.recover(f.now).unwrap().classes, [Class::Fenced; 2]);
    drop(owner);

    let hot = f.stage().with_extension("db-journal");
    write_extent(&hot, 4096);
    let mut owner = open(&f);
    let ambiguous = owner.recover(f.now).unwrap();
    assert_eq!(ambiguous.classes, [Class::Fenced; 2]);
    assert_eq!(ambiguous.charged, RESERVED_BYTES + 4096);
    assert!(hot.exists());
    fs::remove_file(&hot).unwrap();
    fs::remove_file(f.stage()).unwrap();

    let snapshot = f.stage().with_file_name("receiver-join-snapshot.db");
    write_extent(&snapshot, 4096);
    let interrupted = owner.recover(f.now).unwrap();
    assert_eq!(interrupted.classes, [Class::Fenced; 2]);
    assert_eq!(interrupted.extents, [0, 0, 4096]);
    assert!(snapshot.exists());
}

#[test]
fn peer_receiver_inventory_recovers_only_exact_witness_and_retains_expired_slot() {
    let f = Fixture::new();
    let (mut owner, binding, witness) = start_original(&f, 4096);
    let journal = f.stage().with_extension("db-journal");
    let journal_file = write_extent(&journal, 8192);
    let inventory = owner.recover(f.now).unwrap();
    assert_eq!(
        inventory.classes[1],
        Class::JournalCandidate {
            binding,
            main: FileExtent {
                identity: (
                    u64::from_be_bytes(witness.device),
                    u64::from_be_bytes(witness.inode),
                ),
                logical: 4096,
                charged: 4096,
            },
            journal: FileExtent {
                identity: (
                    u64::from_be_bytes(journal_file.device),
                    u64::from_be_bytes(journal_file.inode),
                ),
                logical: 8192,
                charged: 8192,
            },
        },
    );
    assert_eq!(inventory.charged, RESERVED_BYTES + 12_288);
    assert!(inventory.clean_extent(binding).is_err());
    assert!(owner.recover(f.now - 1).is_err());

    let stale_source = owner.source;
    drop(owner.checkpoint);
    let mut owner = open(&f);
    assert!(
        owner
            .checkpoint
            .receiver_charges(&stale_source, f.now)
            .is_err()
    );
    assert_eq!(owner.recover(f.now + 86_401).unwrap(), inventory);
    let known = f.stage().with_file_name("provisional-known.db");
    write_extent(&known, TOTAL_ADMISSION - inventory.charged + 1);
    let over_budget = owner.recover(f.now).unwrap();
    assert_eq!(over_budget.classes, [Class::Fenced; 2]);
    assert_eq!(over_budget.charged, TOTAL_ADMISSION + 1);
    assert!(journal.exists());
    fs::remove_file(known).unwrap();
    assert_eq!(owner.recover(f.now).unwrap(), inventory);
    fs::remove_file(journal).unwrap();
    assert_clean_charge(&f, &mut owner, binding);

    let staged = OpenOptions::new().read(true).open(f.stage()).unwrap();
    fs::remove_file(f.stage()).unwrap();
    assert_eq!(owner.recover(f.now).unwrap().classes[1], Class::Fenced);
    write_extent(&f.stage(), 4096);
    assert_eq!(owner.recover(f.now).unwrap().classes[1], Class::Fenced);
    drop(staged);
}

fn start_original(f: &Fixture, length: u64) -> (RecoveredOwner<'_>, Binding, FileWitness) {
    let routing = NostrRoutingV1::new([3; 32], vec![RELAY.to_owned()]).unwrap();
    let prepared = wire::Codec::for_version(
        wire::MAX_OUTER_BYTES,
        crate::peer_control_version::ControlVersion::V2,
    )
    .unwrap()
    .prepare_offer(&f.server, f.identity.public_key(), [1; 16], &routing, f.now)
    .unwrap();
    let event: Event = serde_json::from_slice(prepared.frame()).unwrap();
    let record = RawRecord {
        id: event.id.to_bytes(),
        stamp: event.created_at.as_secs(),
        bytes: prepared.frame(),
    };
    let key = Selector::from(&prepared.flight);
    let paths = Paths::new(&f.lease, &f.identity).unwrap();
    let conn =
        HardenedPeerConnection::open_installed(paths.cursor().unwrap().path(), *paths.cursor_key())
            .unwrap();
    let uid = unsafe { libc::geteuid() };
    let mut predecessor = Checkpoint::attach_installed_v2(
        conn,
        (*f.identity.signer()).clone(),
        uid,
        RELAY,
        Policy::default(),
    )
    .unwrap();
    let source = predecessor.bind_source(RELAY).unwrap();
    assert_eq!(
        predecessor.admit(&source, record, f.now).unwrap().status,
        Status::Accepted(TrustedOutcome::Applied),
    );
    drop(predecessor);

    let mut owner = open(f);
    let reservation = owner
        .checkpoint
        .reserve_provisioning(&owner.source, key, f.now)
        .unwrap();
    let binding = reservation.stage_binding();
    assert_eq!(
        owner.recover(f.now).unwrap().classes[1],
        Class::Create(binding)
    );

    let stage = owner
        .checkpoint
        .bind_stage(&owner.source, &reservation, f.now)
        .unwrap();
    assert_eq!(
        owner.recover(f.now).unwrap().classes[1],
        Class::Create(binding)
    );
    let witness = write_extent(&f.stage(), length);
    assert_eq!(owner.recover(f.now).unwrap().classes[1], Class::Fenced);
    owner
        .checkpoint
        .start_stage(&owner.source, &stage, witness, f.now)
        .unwrap();
    (owner, binding, witness)
}

fn assert_clean_charge(f: &Fixture, owner: &mut RecoveredOwner<'_>, binding: Binding) {
    let clean = owner.recover(f.now).unwrap().clean_extent(binding).unwrap();
    assert_eq!(clean.main.logical, 4096);
    assert_eq!(clean.charged_other, RESERVED_BYTES + 1);

    let mut stage = OpenOptions::new().write(true).open(f.stage()).unwrap();
    stage.write_all(&[1]).unwrap();
    stage.set_len(1).unwrap();
    stage.sync_all().unwrap();
    let physical = (stage.metadata().unwrap().blocks() * 512).max(1);
    drop(stage);
    let known = f.stage().with_file_name("provisional-known.db");
    write_extent(&known, 8192);
    let clean = owner.recover(f.now).unwrap().clean_extent(binding).unwrap();
    assert_eq!(clean.main.logical, 1);
    assert_eq!(clean.main.charged, physical);
    assert_eq!(clean.charged_other, RESERVED_BYTES + physical + 8192);
    fs::remove_file(known).unwrap();
}

#[test]
fn peer_receiver_inventory_refuses_aliases_unknown_artifacts_and_bad_cursor() {
    let f = Fixture::new();
    let mut owner = open(&f);
    let cursor = f.stage().with_file_name("cursor.db");
    let known = f.stage().with_file_name("provisional-known.db");
    fs::hard_link(&cursor, &known).unwrap();
    assert!(owner.recover(f.now).is_err());
    fs::remove_file(&known).unwrap();
    symlink(&cursor, &known).unwrap();
    assert!(owner.recover(f.now).is_err());
    fs::remove_file(&known).unwrap();
    let unknown = f.stage().with_file_name("unaccounted.db");
    write_extent(&unknown, 4096);
    assert!(owner.recover(f.now).is_err());
    fs::remove_file(unknown).unwrap();

    write_extent(&known, TOTAL_ADMISSION - RESERVED_BYTES);
    let full = owner.recover(f.now).unwrap();
    assert_eq!(full.classes, [Class::Fenced; 2]);
    assert_eq!(full.charged, TOTAL_ADMISSION);
    assert!(known.exists());

    let cursor_journal = cursor.with_extension("db-journal");
    write_extent(&cursor_journal, 0);
    assert!(owner.recover(f.now).is_err());
    drop(owner);
    assert!(RecoveredOwner::open(&f.lease, &f.identity, RELAY).is_err());
    assert!(cursor_journal.exists());
}

#[test]
fn managed_rollback_rechecks_measured_extents_inodes_and_sidecars() {
    let fixture = Fixture::new();
    let (mut owner, binding, _) = start_original(&fixture, 4096);
    let path = fixture.stage();
    let journal_path = path.with_extension("db-journal");
    write_extent(&journal_path, 8192);
    let original = owner.recover(fixture.now).unwrap();
    let Class::JournalCandidate { main, journal, .. } = original.classes[1] else {
        panic!("fixture journal missing");
    };
    let evidence = RollbackFiles::open(&owner.paths, main, journal).unwrap();
    assert!(evidence.read_journal_bounds().is_some());

    for extent in [&path, &journal_path] {
        let file = OpenOptions::new().write(true).open(extent).unwrap();
        let previous = file.metadata().unwrap().len();
        file.set_len(previous + 1).unwrap();
        assert!(evidence.read_journal_bounds().is_none());
        assert!(RollbackFiles::open(&owner.paths, main, journal).is_err());
        file.set_len(previous).unwrap();
    }
    for extension in ["db-wal", "db-shm"] {
        let sidecar = path.with_extension(extension);
        write_extent(&sidecar, 0);
        assert!(evidence.read_journal_bounds().is_none());
        assert_eq!(
            owner.recover(fixture.now).unwrap().classes[1],
            Class::Fenced
        );
        fs::remove_file(sidecar).unwrap();
    }

    let retired = OpenOptions::new().read(true).open(&path).unwrap();
    fs::remove_file(&path).unwrap();
    let replaced = write_extent(&path, main.logical);
    assert_ne!(
        replaced.inode,
        retired.metadata().unwrap().ino().to_be_bytes()
    );
    assert!(evidence.read_journal_bounds().is_none());
    assert_eq!(
        owner.recover(fixture.now).unwrap().classes[1],
        Class::Fenced
    );
    assert!(
        owner
            .recover(fixture.now)
            .unwrap()
            .clean_extent(binding)
            .is_err()
    );
}

const RECOVERY_CHILD: &str = "peer_invitation::receiver_inventory::tests::managed_recovery_child";
const RECOVERY_MODE: &str = "MARMISH_TEST_MANAGED_RECOVERY";
const RECOVERY_ROOT: &str = "MARMISH_TEST_MANAGED_RECOVERY_ROOT";
const RECOVERY_GUARDS: [&str; 7] = [
    "stale-generation",
    "wrong-access",
    "journal-write",
    "journal-truncate",
    "main-cap",
    "combined-cap",
    "main-readonly",
];

#[test]
#[ignore = "opt-in subprocesses using the bundled SQLCipher rollback pager"]
fn managed_recovery_hot_journal_and_stock_wal() {
    use std::os::unix::fs::{FileExt, PermissionsExt};

    #[derive(Clone, Copy)]
    enum Trial {
        Valid,
        InvalidHeader,
        InvalidFirstRecord,
        SuperJournal,
        OversizedJournal,
        WrongKey,
    }

    for case in [
        Trial::Valid,
        Trial::InvalidHeader,
        Trial::InvalidFirstRecord,
        Trial::SuperJournal,
        Trial::OversizedJournal,
        Trial::WrongKey,
    ] {
        let root = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let root = root.path().canonicalize().unwrap();
        let fixture = Fixture::at(&root, None, true);
        let (owner, binding, _) = start_original(&fixture, 0);
        let key = if matches!(case, Trial::WrongKey) {
            SqlCipherKey::raw([42; 32])
        } else {
            SqlCipherKey::raw(*owner.paths.provisional_key(binding))
        };
        let stage = fixture.stage();
        let db = fixture_keyed(&stage, &key);
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
             CREATE TABLE fixture(value TEXT NOT NULL);
             INSERT INTO fixture VALUES('committed');",
        )
        .unwrap();
        db.close().unwrap();
        drop(owner);
        drop(fixture);
        let baseline = fs::read(&stage).unwrap();
        let crash = if matches!(case, Trial::WrongKey) {
            "crash-wrong-key"
        } else {
            "crash"
        };
        run_recovery_child(&root, crash, 80);
        let interrupted = fs::read(&stage).unwrap();
        assert!(interrupted != baseline);
        let journal = stage.with_extension("db-journal");
        if matches!(case, Trial::SuperJournal | Trial::OversizedJournal) {
            let file = OpenOptions::new().write(true).open(&journal).unwrap();
            if matches!(case, Trial::SuperJournal) {
                let mut tail = [0; 16];
                tail[..4].copy_from_slice(&4_u32.to_be_bytes());
                tail[8..].copy_from_slice(&[0xd9, 0xd5, 0x05, 0xf9, 0x20, 0xa1, 0x63, 0xd7]);
                file.write_all_at(&tail, file.metadata().unwrap().len() - 16)
                    .unwrap();
            } else {
                file.set_len(TOTAL_ADMISSION - RESERVED_BYTES).unwrap();
            }
            file.sync_all().unwrap();
        }
        if matches!(case, Trial::InvalidHeader | Trial::InvalidFirstRecord) {
            let journal = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&journal)
                .unwrap();
            let mut sector = [0; 4];
            journal.read_exact_at(&mut sector, 20).unwrap();
            let offset = if matches!(case, Trial::InvalidFirstRecord) {
                // First record checksum follows its page number and cipher page.
                u64::from(u32::from_be_bytes(sector)) + 4 + 4096
            } else {
                0
            };
            let mut byte = [0];
            journal.read_exact_at(&mut byte, offset).unwrap();
            journal.write_all_at(&[byte[0] ^ 1], offset).unwrap();
            journal.sync_all().unwrap();
        }
        if matches!(case, Trial::WrongKey) {
            run_recovery_child(&root, "wrong-key", 0);
        } else if matches!(
            case,
            Trial::InvalidHeader
                | Trial::InvalidFirstRecord
                | Trial::SuperJournal
                | Trial::OversizedJournal
        ) {
            let mode = if matches!(case, Trial::OversizedJournal) {
                "oversized-journal"
            } else {
                "fence"
            };
            run_recovery_child(&root, mode, 0);
            assert!(journal.is_file());
            assert!(fs::read(&stage).unwrap() == interrupted);
        } else {
            let journal_before = fs::read(&journal).unwrap();
            for mode in RECOVERY_GUARDS {
                run_recovery_child(&root, mode, 0);
            }
            assert!(fs::read(&stage).unwrap() == interrupted);
            assert!(fs::read(&journal).unwrap() == journal_before);
            run_recovery_child(&root, "reinterrupt", 80);
            assert!(fs::read(&journal).unwrap() == journal_before);
            run_recovery_child(&root, "recover", 0);
            assert!(!journal.exists());
            let db = fixture_keyed(&stage, &key);
            let value: String = db
                .query_row("SELECT value FROM fixture", [], |row| row.get(0))
                .unwrap();
            assert_eq!(value, "committed");
            db.close().unwrap();
        }
    }
}

fn fixture_keyed(path: &Path, key: &SqlCipherKey) -> rusqlite::Connection {
    use rusqlite::{Connection, OpenFlags};
    use storage_sqlite::{SqlCipherHardening, open_hardened_sqlcipher};
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .unwrap();
    open_hardened_sqlcipher(&db, key, SqlCipherHardening::live_cache()).unwrap();
    db
}

fn run_recovery_child(root: &Path, mode: &str, status: i32) {
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::Instant;
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", RECOVERY_CHILD])
        .env_clear()
        .env(RECOVERY_MODE, mode)
        .env(RECOVERY_ROOT, root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(result) = child.try_wait().unwrap() {
            assert_eq!(result.code(), Some(status), "managed recovery case {mode}");
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("managed recovery case {mode} timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "private child of managed recovery test"]
fn managed_recovery_child() {
    use std::sync::atomic::{AtomicBool, AtomicU64};

    let mode = std::env::var(RECOVERY_MODE).unwrap();
    let root = std::env::var_os(RECOVERY_ROOT).unwrap();
    let f = Fixture::at(Path::new(&root), None, false);
    let stage = f.stage();
    let lease = Arc::new(f.lease);
    let crash = matches!(mode.as_str(), "crash" | "crash-wrong-key");
    if !crash {
        ReceiverQuota::bind_receiver(&lease).unwrap();
    }
    let mut owner = RecoveredOwner::open(&lease, &f.identity, RELAY).unwrap();
    let inventory = owner.recover(f.now).unwrap();
    if crash {
        let Class::Identified { binding, .. } = inventory.classes[1] else {
            panic!("fixture stage missing");
        };
        let key = if mode == "crash-wrong-key" {
            SqlCipherKey::raw([42; 32])
        } else {
            SqlCipherKey::raw(*owner.paths.provisional_key(binding))
        };
        drop(owner);
        crate::peer_quota_vfs::fixture_recovery_crash(&stage, &key);
        return;
    }

    if mode == "oversized-journal" {
        assert_eq!(inventory.classes[1], Class::Fenced);
        assert!(inventory.charged >= TOTAL_ADMISSION);
        return;
    }
    let Class::JournalCandidate {
        binding,
        main,
        journal,
    } = inventory.classes[1]
    else {
        panic!("fixture journal missing");
    };
    if RECOVERY_GUARDS.contains(&mode.as_str()) {
        use crate::peer_quota_vfs::dispatch::{RecoveryRequest, recovery_guard_tests};

        let quota = ReceiverQuota::bind_receiver(&lease).unwrap();
        let generation = NonZeroU64::new(binding.coordinates().1).unwrap();
        // Hold the current family fixed at its measured combined limit to
        // exercise a rejected single-byte growth at aggregate capacity.
        let charged = if mode == "combined-cap" {
            TOTAL_ADMISSION
        } else {
            inventory.charged
        };
        let charged_other = charged - main.logical - journal.logical;
        let key = SqlCipherKey::raw(*owner.paths.provisional_key(binding));
        let files = || RollbackFiles::open(&owner.paths, main, journal).unwrap();
        recovery_guard_tests::probe(
            &quota,
            &mode,
            RecoveryRequest {
                generation,
                files: files(),
                charged_other,
                key: &key,
                interrupt_after_write: false,
            },
            files,
        );
        return;
    }
    let wall = Wall::Fixture(f.wall);
    let ticks = AtomicU64::new(0);
    let stopped = AtomicBool::new(false);
    let fence = TurnFence::new(&wall, &stopped, Some(&ticks), f.now, f.now + 3).unwrap();
    if mode == "reinterrupt" {
        owner
            .recover_journal(&lease, binding, &fence, true)
            .unwrap();
        panic!("recovery failed to observe a database write");
    }
    if matches!(mode.as_str(), "fence" | "wrong-key") {
        assert!(
            owner
                .recover_journal(&lease, binding, &fence, false)
                .is_err()
        );
        let after = owner.recover(f.now).unwrap();
        if mode == "fence" {
            assert_eq!(after, inventory);
        } else {
            // The pager may have replayed bytes before the key was rejected.
            assert!(after.charged >= RESERVED_BYTES + 4096);
        }
        return;
    }
    assert_eq!(mode, "recover");
    let stock = tempfile::tempdir().unwrap();
    let stock_path = stock.path().join("ordinary.db");
    let (ready, ready_rx) = std::sync::mpsc::sync_channel(0);
    let (complete, complete_rx) = std::sync::mpsc::sync_channel(0);
    let stock = std::thread::spawn(move || {
        let db = rusqlite::Connection::open(&stock_path).unwrap();
        db.execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE ordinary(value INTEGER);
             INSERT INTO ordinary VALUES (21);",
        )
        .unwrap();
        assert!(stock_path.with_extension("db-wal").is_file());
        assert!(stock_path.with_extension("db-shm").is_file());
        ready.send(()).unwrap();
        complete_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        db.execute("INSERT INTO ordinary VALUES (21)", []).unwrap();
        let value: i64 = db
            .query_row("SELECT sum(value) FROM ordinary", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, 42);
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let clean = owner
        .recover_journal(&lease, binding, &fence, false)
        .unwrap();
    assert!(inventory.charged >= clean.charged_other);
    complete.send(()).unwrap();
    stock.join().unwrap();
}
