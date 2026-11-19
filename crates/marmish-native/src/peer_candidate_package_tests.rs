use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use cgka_traits::app_components::NostrRoutingV1;
use nostr::{event::Event, key::PublicKey};
use tokio_util::sync::CancellationToken;

use crate::{
    peer_candidate_startup,
    peer_control_version::ControlVersion,
    peer_relay_trust::candidate_registration::{
        package_fixture_registration, package_fixture_root,
    },
};

use super::super::super::store::{
    Error as CursorError, TrustedOutcome,
    consent_checkpoint::{Error as CheckpointError, wire},
};
use super::*;

#[path = "peer_managed_consent_wss_tests.rs"]
mod checked_ack;

const RELAY: &str = "wss://private.example/";
const ROOT: &str = "MARMISH_PACKAGE_FIXTURE_ROOT";
const MODE: &str = "MARMISH_PACKAGE_FIXTURE_MODE";
const CHILD: &str = "peer_invitation::receiver_inventory::package::tests::package_child";

#[test]
fn candidate_package_lifetime_bound_uses_checked_arithmetic() {
    let latest = 5 + NATIVE_PACKAGE_LIFETIME - MIN_PACKAGE_MARGIN;
    assert!(lifetime_possible(latest - 1, 5));
    assert!(lifetime_possible(latest, 5));
    assert!(!lifetime_possible(latest + 1, 5));
    assert!(!lifetime_possible(u64::MAX, 5));
    assert!(!lifetime_possible(1, u64::MAX));
}

#[test]
fn candidate_package_window_revocation_is_sticky() {
    let (startup, listener, _worker) = peer_candidate_startup::pair();
    listener.registered();
    let stopped = Arc::new(AtomicBool::new(false));
    let cancel = CancellationToken::new();
    let sample = Wall::System.sample().unwrap();
    let end = sample.as_secs() + 10;
    assert!(
        NativeWindow::start(
            listener.readiness(),
            stopped.clone(),
            cancel.clone(),
            sample,
            end
        )
        .is_err()
    );
    startup.ordinary_ready();
    assert!(
        NativeWindow::start(
            listener.readiness(),
            stopped.clone(),
            cancel.clone(),
            sample,
            sample.as_secs(),
        )
        .is_err()
    );
    let window = NativeWindow::start(
        listener.readiness(),
        stopped.clone(),
        cancel.clone(),
        sample,
        end,
    )
    .unwrap();
    assert!(window.require_offer_limit(end - 1).is_err());
    assert!(window.sample().is_ok());
    stopped.store(true, Ordering::Release);
    assert!(window.sample().is_err());
    stopped.store(false, Ordering::Release);
    assert!(window.sample().is_err());
    let second = NativeWindow::start(
        listener.readiness(),
        stopped.clone(),
        cancel.clone(),
        sample,
        end,
    )
    .unwrap();
    cancel.cancel();
    assert!(second.sample().is_err());
    let third = NativeWindow::start(
        listener.readiness(),
        stopped,
        CancellationToken::new(),
        sample,
        end,
    )
    .unwrap();
    startup.stop();
    assert!(third.sample().is_err());
}

#[test]
#[ignore = "opt-in process-global checked VFS and real SQLCipher MDK"]
fn candidate_package_cold_attestation_under_registered_quota() {
    for cuts in [
        &["bound", "issue", "copy-cancel", "attest", "prepared"][..],
        &["bound-file", "fenced"],
        &["started-empty", "empty"],
        &["early-stop", "bound"],
        &["schema-stop", "empty"],
        &["issue-stop", "schema-only"],
        &["prestage-lost", "schema-only"],
        &["prestage-extent", "schema-only"],
        &["foreign-package-owner", "schema-only"],
        &["prestage-expired", "schema-only"],
        &["signing-loss", "unsigned"],
        &["generation-crash", "recover-unsigned"],
        &["issue", "copy-crash", "fenced"],
        &["issue", "replace"],
        &["issue", "cap"],
    ] {
        let root = package_fixture_root();
        for mode in cuts {
            run_child(root.path(), mode);
        }
    }
}

#[test]
#[ignore = "opt-in process-global checked VFS and real SQLCipher MDK"]
fn managed_queued_package_uses_unchanged_charged_image_across_restart() {
    let root = package_fixture_root();
    for mode in [
        "issue",
        "queue",
        "queued-attest",
        "queued-replaced",
        "queued-journal",
        "queued-copy-crash",
        "queued-fenced",
    ] {
        run_child(root.path(), mode);
    }
}

#[test]
#[ignore = "opt-in registered SQLCipher and real staged package"]
fn managed_consent_queue_and_attempt_preserve_exact_authority() {
    for cuts in [
        ["issue", "queue-managed", "queued-managed-attempt"],
        ["issue", "queue-commit-cut", "queued-managed-attempt"],
    ] {
        let root = package_fixture_root();
        for mode in cuts {
            run_child(root.path(), mode);
        }
    }
}

fn run_child(root: &Path, mode: &str) {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", CHILD, "--nocapture"])
        .env_clear()
        .env(ROOT, root)
        .env(MODE, mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("package subprocess timed out: {mode}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    match (mode, output.status.code()) {
        ("copy-crash" | "queued-copy-crash", Some(78)) | ("generation-crash", Some(80)) => return,
        _ => {}
    }
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "package case failed: {mode}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.starts_with("package_fixture_timing ") {
            eprintln!("{line}");
        }
    }
}

#[test]
#[ignore = "child of candidate_package_cold_attestation_under_registered_quota"]
fn package_child() {
    let Some(root) = std::env::var_os(ROOT) else {
        return;
    };
    let root = Path::new(&root);
    let mode = std::env::var(MODE).unwrap();
    let (lease, identity, registration) = package_fixture_registration(root);
    let mut registration = registration.attach(&lease, &identity).unwrap();
    let paths = Paths::new(&lease, &identity).unwrap();
    let member = lease.root().join("peer");
    let stage = member.join("provisional-unknown.db");
    let snapshot = member.join("receiver-join-snapshot.db");
    let cursor_path = member.join("cursor.db");
    let now = Wall::System.sample().unwrap().as_secs();
    let selector_file = root.join("fixture-selector");
    let (selector, expires) = if !selector_file.exists() {
        assert!(!cursor_path.exists());
        // The directory was actually registered before the test creates the
        // existing installed admission cursor. No raw fixture quota is used.
        create_empty(&cursor_path);
        let founder = Keys::generate();
        let prepared = wire::Codec::for_version(wire::MAX_OUTER_BYTES, ControlVersion::V2)
            .unwrap()
            .prepare_offer(
                &founder,
                identity.public_key(),
                [1; 16],
                &NostrRoutingV1::new([3; 32], vec![RELAY.to_owned()]).unwrap(),
                now,
            )
            .unwrap();
        let outer: Event = serde_json::from_slice(prepared.frame()).unwrap();
        let mut predecessor = Checkpoint::attach_installed_v2(
            HardenedPeerConnection::open_installed(&cursor_path, *paths.cursor_key()).unwrap(),
            (*identity.signer()).clone(),
            // SAFETY: reads the effective UID of the current process.
            unsafe { libc::geteuid() },
            RELAY,
            Policy::default(),
        )
        .unwrap();
        let source = predecessor.bind_source(RELAY).unwrap();
        let status = predecessor
            .admit(
                &source,
                RawRecord {
                    id: outer.id.to_bytes(),
                    stamp: outer.created_at.as_secs(),
                    bytes: prepared.frame(),
                },
                now,
            )
            .unwrap()
            .status;
        assert_eq!(status, Status::Accepted(TrustedOutcome::Applied));
        let selector = Selector::from(&prepared.flight);
        let expires = prepared.flight.flight().expires_at;
        let data = serde_json::to_vec(&(
            selector.server.to_hex(),
            selector.id,
            selector.revision,
            expires,
        ))
        .unwrap();
        fs::write(&selector_file, data).unwrap();
        (selector, expires)
    } else {
        let (server, id, revision, expires): (String, _, _, u64) =
            serde_json::from_slice(&fs::read(&selector_file).unwrap()).unwrap();
        (
            Selector {
                server: PublicKey::parse(&server).unwrap(),
                id,
                revision,
            },
            expires,
        )
    };

    let (quota, mut route) = registration.actor_parts();
    let mut owner = RecoveredOwner::open_registered(&lease, &identity, quota, &mut route).unwrap();
    assert!(std::ptr::eq(owner.package_quota.unwrap(), quota));
    let (startup, listener, _worker) = peer_candidate_startup::pair();
    listener.registered();
    startup.ordinary_ready();
    let window = || {
        NativeWindow::start(
            listener.readiness(),
            Arc::new(AtomicBool::new(false)),
            CancellationToken::new(),
            Wall::System.sample().unwrap(),
            expires.min(now + 60),
        )
        .unwrap()
    };

    if mode.starts_with("queued-") {
        let turn = window();
        let cursor_before = fs::read(&cursor_path).unwrap();
        let original = fs::read(&stage).unwrap();
        let wall = turn.sample().unwrap().as_secs();
        if mode == "queued-managed-attempt" {
            let account = identity.public_key();
            let load = |owner: &mut RecoveredOwner<'_>| {
                owner
                    .checkpoint
                    .read_managed_queued(
                        &owner.source,
                        account,
                        RELAY,
                        selector,
                        turn.sample().unwrap().as_secs(),
                    )
                    .unwrap()
                    .unwrap()
            };
            let first = load(&mut owner);
            let stale = load(&mut owner);
            let proof = owner.attest_queued_package(quota, &first, &turn).unwrap();
            let before = owner.checkpoint.consent_fixture_clocks();
            let rejected = owner.checkpoint.reserve_managed_consent(
                &owner.source,
                first,
                &proof,
                wall.max(before.0).max(before.1) + 1,
                || Err(CheckpointError::Expired),
            );
            assert!(matches!(rejected, Err(CheckpointError::Expired)));
            assert_eq!(owner.checkpoint.consent_fixture_clocks(), before);
            assert_eq!(fs::read(&cursor_path).unwrap(), cursor_before);

            let first = load(&mut owner);
            let point = Instant::now();
            let reserved = owner
                .reserve_managed_consent(quota, first, &turn)
                .unwrap()
                .unwrap();
            println!(
                "package_fixture_timing managed_reserve_ms={:.3} image_bytes={}",
                point.elapsed().as_secs_f64() * 1000.0,
                original.len()
            );
            assert_eq!(reserved.attempt().sequence(), 1);
            owner.managed_before_flush(quota, &reserved).unwrap();
            let after = owner.checkpoint.consent_fixture_clocks();
            let cursor_after = fs::read(&cursor_path).unwrap();

            assert!(matches!(
                owner.reserve_managed_consent(quota, stale, &turn),
                Err(Error::Checkpoint(CheckpointError::Cursor(
                    CursorError::Stale
                )))
            ));
            let current = load(&mut owner);
            let deferred = owner
                .reserve_managed_consent(quota, current, &turn)
                .unwrap();
            assert!(deferred.is_none());
            assert_eq!(owner.checkpoint.consent_fixture_clocks(), after);
            assert_eq!(fs::read(&cursor_path).unwrap(), cursor_after);
            assert_eq!(fs::read(&stage).unwrap(), original);
            assert_eq!(turn.signs(), 0);
            assert!(!snapshot.exists());
            return;
        }
        let mut read = |url, signer, at| {
            owner
                .checkpoint
                .read_managed_queued(&owner.source, signer, url, selector, at)
        };
        let account = identity.public_key();
        if mode == "queued-attest" {
            assert!(matches!(
                read("wss://foreign.example/", account, wall),
                Err(CheckpointError::Source)
            ));
            assert!(matches!(
                read(RELAY, Keys::generate().public_key(), wall),
                Err(CheckpointError::Source)
            ));
            assert!(read(RELAY, account, 0).is_err());
            assert!(read(RELAY, account, expires).is_err());
            assert!(!snapshot.exists());
            assert_eq!(fs::read(&cursor_path).unwrap(), cursor_before);
            assert_eq!(fs::read(&stage).unwrap(), original);
            let queued = read(RELAY, account, wall).unwrap().unwrap();
            let mut foreign =
                RecoveredOwner::open_registered(&lease, &identity, quota, &mut route).unwrap();
            let foreign_turn = window();
            assert!(matches!(
                foreign.attest_queued_package(quota, &queued, &foreign_turn),
                Err(Error::Checkpoint(CheckpointError::LocalOwner))
            ));
            assert_eq!(foreign_turn.signs(), 0);
            assert!(!snapshot.exists());
            assert_eq!(fs::read(&cursor_path).unwrap(), cursor_before);
            assert_eq!(fs::read(&stage).unwrap(), original);
        }
        let queued = owner
            .checkpoint
            .read_managed_queued(&owner.source, account, RELAY, selector, wall)
            .unwrap()
            .unwrap();
        if matches!(mode.as_str(), "queued-replaced" | "queued-journal") {
            let observed = if mode == "queued-replaced" {
                let moved = root.join("fixture-replaced-original");
                fs::rename(&stage, &moved).unwrap();
                fs::copy(&moved, &stage).unwrap();
                (moved, original.as_slice())
            } else {
                let journal = stage.with_extension("db-journal");
                create_empty(&journal);
                (journal, &[][..])
            };
            assert!(matches!(
                owner.attest_queued_package(quota, &queued, &turn),
                Err(Error::StageIdentityMismatch)
            ));
            assert_eq!(turn.signs(), 0);
            assert!(!snapshot.exists());
            assert_eq!(fs::read(&cursor_path).unwrap(), cursor_before);
            assert_eq!(fs::read(&stage).unwrap(), original);
            assert_eq!(fs::read(&observed.0).unwrap(), observed.1);
            if mode == "queued-replaced" {
                fs::rename(observed.0, &stage).unwrap();
            } else {
                fs::remove_file(observed.0).unwrap();
            }
            return;
        }
        if mode == "queued-copy-crash" {
            turn.set_copy_cut(true);
        }
        let point = Instant::now();
        let proof = owner.attest_queued_package(quota, &queued, &turn);
        if mode == "queued-fenced" {
            assert!(proof.is_err());
            assert!(snapshot.exists());
        } else {
            let proof = proof.unwrap();
            assert_eq!(
                proof.target.artifact().id.as_slice(),
                fs::read(root.join("fixture-artifact-id")).unwrap()
            );
            println!(
                "package_fixture_timing {mode}_ms={:.3} image_bytes={}",
                point.elapsed().as_secs_f64() * 1000.0,
                original.len()
            );
            assert!(!snapshot.exists());
            let point = Instant::now();
            owner
                .recheck_managed_package(quota, &proof, &window())
                .unwrap();
            println!(
                "package_fixture_timing redigest_ms={:.3}",
                point.elapsed().as_secs_f64() * 1000.0
            );
            let mut foreign =
                RecoveredOwner::open_registered(&lease, &identity, quota, &mut route).unwrap();
            assert!(matches!(
                foreign.recheck_managed_package(quota, &proof, &window()),
                Err(Error::Checkpoint(CheckpointError::LocalOwner))
            ));
            let image = OpenOptions::new().write(true).open(&stage).unwrap();
            use std::os::unix::fs::FileExt as _;
            let offset = u64::try_from(original.len() - 1).unwrap();
            image
                .write_all_at(&[original[original.len() - 1] ^ 1], offset)
                .unwrap();
            assert!(matches!(
                owner.recheck_managed_package(quota, &proof, &window()),
                Err(Error::StageIdentityMismatch)
            ));
            image
                .write_all_at(&original[original.len() - 1..], offset)
                .unwrap();
            owner
                .recheck_managed_package(quota, &proof, &window())
                .unwrap();
            startup.stop();
            assert!(owner.recheck_managed_package(quota, &proof, &turn).is_err());
        }
        assert_eq!(turn.signs(), 0);
        assert_eq!(fs::read(&cursor_path).unwrap(), cursor_before);
        assert_eq!(fs::read(&stage).unwrap(), original);
        return;
    }
    let reservation = owner
        .checkpoint
        .reserve_provisioning(&owner.source, selector, now)
        .unwrap();

    match mode.as_str() {
        "queue-managed" | "queue-commit-cut" => {
            let turn = window();
            let proof = owner
                .attest_package(quota, &reservation, None, &turn)
                .unwrap();
            let original = fs::read(&stage).unwrap();
            let saved_cursor = fs::read(&cursor_path).unwrap();
            let floors = owner.checkpoint.consent_fixture_clocks();
            assert!(matches!(
                owner
                    .checkpoint
                    .sign_managed_consent(&owner.source, &reservation, &proof, expires,),
                Err(CheckpointError::Expired)
            ));
            assert_eq!(owner.checkpoint.consent_fixture_clocks(), floors);
            assert_eq!(fs::read(&cursor_path).unwrap(), saved_cursor);
            let point = Instant::now();
            if mode == "queue-commit-cut" {
                assert!(
                    owner
                        .queue_managed_consent_with(quota, &reservation, &proof, &turn, || {
                            startup.stop();
                        })
                        .is_err()
                );
                let wall = Wall::System.sample().unwrap().as_secs();
                assert!(
                    owner
                        .checkpoint
                        .read_managed_queued(
                            &owner.source,
                            identity.public_key(),
                            RELAY,
                            selector,
                            wall,
                        )
                        .unwrap()
                        .is_some()
                );
            } else {
                owner
                    .queue_managed_consent(quota, &reservation, &proof, &turn)
                    .unwrap();
                let saved = fs::read(&cursor_path).unwrap();
                assert!(
                    owner
                        .queue_managed_consent(quota, &reservation, &proof, &turn)
                        .is_err()
                );
                assert_eq!(fs::read(&cursor_path).unwrap(), saved);
            }
            println!(
                "package_fixture_timing {mode}_ms={:.3} image_bytes={}",
                point.elapsed().as_secs_f64() * 1000.0,
                original.len()
            );
            assert_eq!(turn.signs(), 1);
            assert_eq!(fs::read(&stage).unwrap(), original);
            assert!(!snapshot.exists());
        }
        "queue" => {
            let turn = window();
            let proof = owner
                .attest_package(quota, &reservation, None, &turn)
                .unwrap();
            let wall = turn.sample().unwrap().as_secs();
            let candidate = owner
                .checkpoint
                .inspect_prepared_consent(&owner.source, &reservation, wall)
                .unwrap();
            let unsaved = owner
                .checkpoint
                .sign_managed_prepared_fixture(
                    &candidate,
                    &proof.target,
                    &proof.package.event.bytes,
                    wall,
                )
                .unwrap();
            owner
                .checkpoint
                .queue_prepared_consent(&owner.source, &reservation, unsaved, wall, |_| Ok(()))
                .unwrap();
            assert!(!snapshot.exists());
        }
        "bound" | "bound-file" => {
            let bound = owner
                .checkpoint
                .bind_stage(&owner.source, &reservation, now)
                .unwrap();
            assert_eq!(bound.state(), State::Bound);
            if mode == "bound-file" {
                create_empty(&stage);
            }
        }
        "started-empty" => {
            let started = owner
                .start_package(quota, &reservation, &window())
                .unwrap()
                .unwrap();
            assert_eq!(fs::metadata(&stage).unwrap().len(), 0);
            drop(started);
        }
        "early-stop" => {
            let turn = window();
            startup.stop();
            assert!(owner.start_package(quota, &reservation, &turn).is_err());
            assert!(!stage.exists());
        }
        "schema-stop" => {
            let turn = window();
            let started = owner
                .start_package(quota, &reservation, &turn)
                .unwrap()
                .unwrap();
            startup.stop();
            assert!(
                owner
                    .prestage_package(quota, &reservation, started)
                    .is_err()
            );
            assert_eq!(fs::metadata(&stage).unwrap().len(), 0);
        }
        "issue-stop" | "prestage-lost" | "prestage-extent" => {
            let started = owner
                .start_package(quota, &reservation, &window())
                .unwrap()
                .unwrap();
            let staged = owner
                .prestage_package(quota, &reservation, started)
                .unwrap();
            assert!(fs::metadata(&stage).unwrap().len() > 0);
            if mode == "prestage-lost" {
                drop(staged);
            } else {
                let changed = if mode == "issue-stop" {
                    startup.stop();
                    None
                } else {
                    let image = OpenOptions::new().append(true).open(&stage).unwrap();
                    let length = staged.extent.logical;
                    image.set_len(length + 1).unwrap();
                    Some((image, length))
                };
                let observed = staged.window.clone();
                assert!(owner.issue_package(quota, &reservation, staged).is_err());
                assert_eq!(observed.signs(), 0);
                if let Some((image, length)) = changed {
                    // Restore the exact image for the following cold unsigned check.
                    image.set_len(length).unwrap();
                }
            }
        }
        "foreign-package-owner" => {
            let started = owner
                .start_package(quota, &reservation, &window())
                .unwrap()
                .unwrap();
            let staged = owner
                .prestage_package(quota, &reservation, started)
                .unwrap();
            let mut foreign = RecoveredOwner::open(&lease, &identity, RELAY).unwrap();
            let current_wall = Wall::System.sample().unwrap().as_secs();
            let unrelated = foreign
                .checkpoint
                .reserve_provisioning(&foreign.source, selector, current_wall)
                .unwrap();
            let turn = staged.window.clone();
            assert!(matches!(
                owner.issue_package(quota, &unrelated, staged),
                Err(Error::Checkpoint(CheckpointError::LocalOwner))
            ));
            assert_eq!(turn.signs(), 0);
        }
        "prestage-expired" => {
            let anchor = Wall::System.sample().unwrap();
            let turn = NativeWindow::start(
                listener.readiness(),
                Arc::new(AtomicBool::new(false)),
                CancellationToken::new(),
                anchor,
                expires.min(anchor.as_secs() + 8),
            )
            .unwrap();
            let started = owner
                .start_package(quota, &reservation, &turn)
                .unwrap()
                .unwrap();
            turn.expire_schema_after_open();
            assert!(matches!(
                owner.prestage_package(quota, &reservation, started),
                Err(Error::Clock)
            ));
            assert_eq!(turn.signs(), 0);
            assert!(fs::metadata(&stage).unwrap().len() > 0);
        }
        "generation-crash" => {
            let started = owner
                .start_package(quota, &reservation, &window())
                .unwrap()
                .unwrap();
            let staged = owner
                .prestage_package(quota, &reservation, started)
                .unwrap();
            staged.window.crash_generation();
            let result = owner.issue_package(quota, &reservation, staged);
            panic!(
                "native generation did not reach its interrupted write: {:?}",
                result.err()
            );
        }
        "recover-unsigned" => {
            let before = owner.recover(now).unwrap();
            assert!(matches!(before.classes[1], Class::JournalCandidate { .. }));
            let turn = window();
            assert!(
                owner
                    .attest_package(quota, &reservation, None, &turn)
                    .is_err()
            );
            assert!(turn.sample().is_ok());
            assert_eq!(turn.signs(), 0);
            assert!(!snapshot.exists());
            let after = owner.recover(now).unwrap();
            assert!(matches!(after.classes[1], Class::Identified { .. }));
            assert!(after.charged > RESERVED_BYTES);
            assert!(
                owner
                    .start_package(quota, &reservation, &turn)
                    .unwrap()
                    .is_none()
            );
        }
        "signing-loss" => {
            let started = owner
                .start_package(quota, &reservation, &window())
                .unwrap()
                .unwrap();
            let staged = owner
                .prestage_package(quota, &reservation, started)
                .unwrap();
            let turn = staged.window.clone();
            turn.stop_after_signing();
            assert!(owner.issue_package(quota, &reservation, staged).is_err());
            assert_eq!(turn.signs(), 1);
            assert!(turn.sample().is_err());
        }
        "issue" => {
            let turn = window();
            // SAFETY: the isolated child runs one fixture and restores its process umask
            // immediately after checking exclusive original allocation.
            let previous_umask = unsafe { libc::umask(0o200) };
            let started = owner.start_package(quota, &reservation, &turn);
            // SAFETY: this restores the exact prior mask in the same isolated child.
            unsafe { libc::umask(previous_umask) };
            let started = started.unwrap().unwrap();
            assert_eq!(
                fs::metadata(&stage).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let staged = owner
                .prestage_package(quota, &reservation, started)
                .unwrap();
            let point = Instant::now();
            let turn = staged.window.clone();
            turn.trace_timing();
            let artifact = owner
                .issue_package(quota, &reservation, staged)
                .unwrap_or_else(|error| {
                    panic!(
                        "package issue failed after {:.2}s with {} signs: {error:?}",
                        point.elapsed().as_secs_f64(),
                        turn.signs()
                    )
                });
            assert_eq!(turn.signs(), 1);
            println!(
                "package_fixture_timing issue_ms={:.3}",
                point.elapsed().as_secs_f64() * 1000.0
            );
            assert!(artifact.expires > expires + 600);
            assert!(
                owner
                    .start_package(quota, &reservation, &window())
                    .unwrap()
                    .is_none()
            );
            fs::write(
                root.join("fixture-original-ciphertext"),
                fs::read(&stage).unwrap(),
            )
            .unwrap();
            fs::write(root.join("fixture-artifact-id"), artifact.id).unwrap();
            assert!(!snapshot.exists());
        }
        "attest" | "prepared" => {
            let before = fs::read(root.join("fixture-original-ciphertext")).unwrap();
            let id = fs::read(root.join("fixture-artifact-id")).unwrap();
            let point = Instant::now();
            let turn = window();
            let read = owner
                .attest_package(quota, &reservation, None, &turn)
                .unwrap();
            assert_eq!(turn.signs(), 0);
            println!(
                "package_fixture_timing {mode}_ms={:.3}",
                point.elapsed().as_secs_f64() * 1000.0
            );
            assert_eq!(read.target.artifact().id.as_slice(), id);
            assert_eq!(read.target.stage().0, reservation.stage_binding());
            assert_eq!(read.package.event.id, read.target.artifact().id);
            assert_eq!(
                read.target.artifact(),
                exact_artifact(&read.package, &window()).unwrap()
            );
            assert_eq!(fs::read(&stage).unwrap(), before);
            assert!(!snapshot.exists());
            assert_eq!(
                owner
                    .checkpoint
                    .bind_stage(
                        &owner.source,
                        &reservation,
                        Wall::System.sample().unwrap().as_secs()
                    )
                    .unwrap()
                    .state(),
                State::Prepared(read.target.stage().1, read.target.artifact())
            );
            let incorrect = Artifact {
                digest: [0; 32],
                ..read.target.artifact()
            };
            assert!(
                owner
                    .attest_package(quota, &reservation, Some(incorrect), &window())
                    .is_err()
            );
            assert!(!snapshot.exists());
        }
        "copy-cancel" | "copy-crash" => {
            let before = fs::read(&stage).unwrap();
            let turn = window();
            turn.set_copy_cut(mode == "copy-crash");
            assert!(
                owner
                    .attest_package(quota, &reservation, None, &turn)
                    .is_err()
            );
            assert_eq!(mode, "copy-cancel", "actual bounded copy cut did not run");
            assert!(turn.sample().is_err());
            assert!(!snapshot.exists());
            assert_eq!(fs::read(&stage).unwrap(), before);
        }
        "replace" | "cap" => {
            if mode == "replace" {
                let old = root.join("fixture-replaced-original");
                fs::rename(&stage, &old).unwrap();
                fs::copy(old, &stage).unwrap();
            } else {
                OpenOptions::new()
                    .write(true)
                    .open(&stage)
                    .unwrap()
                    .set_len(crate::peer_quota_vfs::dispatch::CLOSED_STAGE_LIMIT + 1)
                    .unwrap();
            }
            let before = fs::metadata(&stage).unwrap().len();
            let turn = window();
            assert!(
                owner
                    .attest_package(quota, &reservation, None, &turn)
                    .is_err()
            );
            assert_eq!(turn.signs(), 0);
            assert!(!snapshot.exists());
            assert_eq!(fs::metadata(&stage).unwrap().len(), before);
        }
        "fenced" | "empty" | "unsigned" | "schema-only" => {
            let before = fs::read(&stage).unwrap();
            if mode == "fenced" && !snapshot.exists() {
                assert!(owner.start_package(quota, &reservation, &window()).is_err());
            } else {
                assert!(
                    owner
                        .start_package(quota, &reservation, &window())
                        .unwrap()
                        .is_none()
                );
            }
            assert!(
                owner
                    .attest_package(quota, &reservation, None, &window())
                    .is_err()
            );
            assert_eq!(fs::read(&stage).unwrap(), before);
            if matches!(mode.as_str(), "unsigned" | "schema-only") {
                let binding = reservation.stage_binding();
                let inspect = root.join("fixture-unsigned-copy.db");
                fs::copy(&stage, &inspect).unwrap();
                let inspection = OpenStage::open_provisional(
                    &inspect,
                    &SqlCipherKey::raw(*paths.provisional_key(binding)),
                    identity.signer().clone(),
                    RELAY,
                    paths.provisional_label(binding),
                    false,
                )
                .unwrap();
                if mode == "schema-only" {
                    inspection.fixture_require_empty_package().unwrap();
                } else {
                    assert!(
                        !inspection
                            .provisional_is_signed(Wall::System.sample().unwrap())
                            .unwrap()
                    );
                }
            }
            if snapshot.exists() {
                assert!(fs::metadata(&snapshot).unwrap().len() > 0);
            }
        }
        _ => panic!("unexpected private fixture case"),
    }
    for name in [stage, snapshot, cursor_path] {
        for suffix in ["db-journal", "db-wal", "db-shm"] {
            assert!(!name.with_extension(suffix).exists());
        }
    }
    drop(owner);
    lease.cleanup_runtime().unwrap();
}

fn create_empty(path: &Path) {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .sync_all()
        .unwrap();
}
