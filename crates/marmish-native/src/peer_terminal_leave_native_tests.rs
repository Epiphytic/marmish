//! Local opt-in proof using separate owners with no inherited native state.

use sha2::Digest as _;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64};

use crate::peer_invitation::prepared_consent::leave_owner::{Cut, Disposition, LeaveOwner};
use crate::peer_quota_vfs::dispatch::mount_for_proof;

use super::*;

const MODE: &str = "MARMISH_LEAVE_PROOF_MODE";
const ROOT: &str = "MARMISH_LEAVE_PROOF_ROOT";
const INTERRUPTED: i32 = 81;

#[test]
#[ignore = "opt-in bounded native Leave qualification in isolated processes"]
fn peer_terminal_leave_completes_in_one_managed_poll() {
    if let Some(mode) = std::env::var_os(MODE) {
        mount_for_proof().unwrap();
        let root = PathBuf::from(std::env::var_os(ROOT).unwrap());
        let mode = mode.into_string().unwrap();
        if mode == "setup" {
            std::thread::Builder::new()
                .stack_size(32 * 1024 * 1024)
                .spawn(move || {
                    tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap()
                        .block_on(setup(&root));
                })
                .unwrap()
                .join()
                .unwrap();
        } else {
            run(&root, &mode);
        }
        return;
    }

    let run = |root: &Path, mode: &str, expected| {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "peer_terminal_leave_completes_in_one_managed_poll",
                "--ignored",
                "--test-threads=1",
            ])
            .env(MODE, mode)
            .env(ROOT, root)
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert_eq!(status.code(), Some(expected), "isolated Leave mode {mode}");
                break;
            }
            if started.elapsed() >= DEADLINE {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("isolated Leave mode timed out: {mode}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let directory = || {
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap()
    };

    let full = directory();
    run(full.path(), "setup", 0);
    let initial = stage_image(full.path());
    run(full.path(), "foreign_generation", 0);
    run(full.path(), "before_start", 0);
    assert_eq!(initial, stage_image(full.path()));
    run(full.path(), "native_closed", INTERRUPTED);
    run(full.path(), "captured", INTERRUPTED);
    run(full.path(), "recheck", 0);
    let saved = fs::read(full.path().join("proof-id")).unwrap();
    run(full.path(), "recheck", 0);
    assert_eq!(saved, fs::read(full.path().join("proof-id")).unwrap());

    let armed = directory();
    run(armed.path(), "setup", 0);
    let original = stage_image(armed.path());
    run(armed.path(), "armed", INTERRUPTED);
    let stage = armed.path().join("client/peer/provisional-unknown.db");
    let preserved = armed.path().join("preserved-image");
    fs::rename(&stage, &preserved).unwrap();
    fixture_write(stage.clone(), &fs::read(&preserved).unwrap());
    run(armed.path(), "refuse", 0);
    fs::remove_file(&stage).unwrap();
    fs::rename(preserved, stage).unwrap();
    run(armed.path(), "indeterminate", 0);
    assert_eq!(original, stage_image(armed.path()));

    let stopped = directory();
    run(stopped.path(), "setup", 0);
    run(stopped.path(), "after_stop", 0);
    run(stopped.path(), "recheck", 0);

    let journal = directory();
    run(journal.path(), "setup", 0);
    run(journal.path(), "hot_write", 80);
    let journal_path = journal
        .path()
        .join("client/peer/provisional-unknown.db-journal");
    let hot = fs::read(&journal_path).expect("synced hot journal");
    assert!(!hot.is_empty());
    let original = stage_image(journal.path());
    run(journal.path(), "refuse", 0);
    assert_eq!(original, stage_image(journal.path()));
    assert!(
        hot == fs::read(&journal_path).unwrap(),
        "hot journal changed"
    );
}

fn stage_image(root: &Path) -> [u8; 32] {
    Sha256::digest(fs::read(root.join("client/peer/provisional-unknown.db")).unwrap()).into()
}

async fn setup(root: &Path) {
    let founder = root.join("founder");
    fs::create_dir(&founder).unwrap();
    fs::set_permissions(&founder, fs::Permissions::from_mode(0o700)).unwrap();
    let fixture = Fixture::at(root, None, true);
    drop(fixture.start(Policy::default()));
    let route = Route::configured(ORIGIN).unwrap();
    let open = |epoch| {
        PreparedConsentOwner::open_with_epoch(
            &fixture.lease,
            &fixture.identity,
            Role::Both,
            Policy::default(),
            &route,
            Wall::Fixture(fixture.wall.clone()),
            epoch,
        )
        .unwrap()
    };
    drop(open(Epoch::Prepared));
    let mut owner = open(Epoch::LeaveEffect);
    let (pending, _) = reach_pending(&fixture, &mut owner, &founder).await;
    let now = fixture.now;
    let local = owner
        .state
        .checkpoint
        // SAFETY: reads the current effective UID of this fixture process.
        .authorize_local(unsafe { libc::geteuid() })
        .unwrap();
    owner
        .state
        .checkpoint
        .hold_pending_decline(
            &owner.state.source,
            pending.selector,
            pending.generation,
            now,
            &local,
            || Ok(()),
        )
        .unwrap();
    // Only public selector and clock coordinates, kept in the private fixture.
    let locator = [
        now.to_be_bytes().as_slice(),
        pending.generation.to_be_bytes().as_slice(),
        pending.selector.server.as_bytes(),
        &pending.selector.id,
        &pending.selector.revision,
    ]
    .concat();
    fixture_write(root.join("locator"), &locator);
}

fn fixture_write(path: PathBuf, bytes: &[u8]) {
    use std::io::Write as _;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn run(root: &Path, mode: &str) {
    let locator: [u8; 96] = fs::read(root.join("locator")).unwrap().try_into().unwrap();
    let number = |bytes: &[u8]| u64::from_be_bytes(bytes.try_into().unwrap());
    let wall = Arc::new(AtomicU64::new(number(&locator[..8])));
    let generation = number(&locator[8..16]) + u64::from(mode == "foreign_generation");
    let selector = Selector {
        server: nostr::key::PublicKey::from_slice(&locator[16..48]).unwrap(),
        id: locator[48..64].try_into().unwrap(),
        revision: locator[64..].try_into().unwrap(),
    };
    let fixture = Fixture::at(root, None, false);
    let route = Route::configured(ORIGIN).unwrap();
    let mut owner = LeaveOwner::open(
        &fixture.lease,
        &fixture.identity,
        Policy::default(),
        &route,
        Wall::Fixture(wall),
    )
    .unwrap();
    if mode == "hot_write" {
        owner = owner.crash_inside_original();
    }
    let stop = AtomicBool::new(mode == "before_start");
    let ticks = AtomicU64::new(0);
    let result = owner.run(
        selector,
        generation,
        &stop,
        (mode != "native_closed").then_some(&ticks),
        |cut| {
            match (mode, cut) {
                ("armed", Cut::Armed) | ("native_closed", Cut::NativeClosed) => exit_cut(),
                ("captured", Cut::Captured) => exit_cut(),
                ("recheck", Cut::Captured)
                | ("native_closed" | "hot_write" | "after_stop", Cut::Armed) => (),
                ("after_stop", Cut::NativeClosed) => stop.store(true, Ordering::Release),
                _ => panic!("unexpected Leave boundary in mode {mode}"),
            }
            Ok(())
        },
    );
    match mode {
        "indeterminate" => assert_eq!(result, Ok(Disposition::Indeterminate)),
        "before_start" | "after_stop" => assert_eq!(result, Err(Error::Clock)),
        "foreign_generation" => assert_eq!(result, Err(Error::Binding)),
        "refuse" => assert_eq!(result, Err(Error::StageIdentityMismatch)),
        "recheck" => {
            let Disposition::Captured(id) = result.expect("cold capture must be recoverable")
            else {
                panic!("native proposal remained indeterminate");
            };
            let path = root.join("proof-id");
            if path.exists() {
                assert_eq!(id.as_slice(), fs::read(path).unwrap());
            } else {
                fixture_write(path, &id);
            }
        }
        _ => panic!("proof did not stop at its declared boundary: {mode}"),
    }
}

fn exit_cut() -> ! {
    // SAFETY: this is only the disposable child. Exit without native/SQL drops.
    unsafe { libc::_exit(INTERRUPTED) }
}
