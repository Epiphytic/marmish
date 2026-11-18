use std::{
    fs::{self, OpenOptions},
    os::unix::fs::OpenOptionsExt,
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

use super::super::super::store::{TrustedOutcome, consent_checkpoint::wire};
use super::*;

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
        &["signing-loss", "unsigned"],
        &["issue", "copy-crash", "fenced"],
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
    let deadline = Instant::now() + Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("package subprocess timed out: {mode}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    if mode == "copy-crash" && output.status.code() == Some(78) {
        return;
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

    let mut owner = RecoveredOwner::open_registered(&lease, &identity, &mut registration).unwrap();
    let quota = owner.package_quota.unwrap();
    let reservation = owner
        .checkpoint
        .reserve_provisioning(&owner.source, selector, now)
        .unwrap();
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

    match mode.as_str() {
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
        "signing-loss" => {
            let turn = window();
            let started = owner
                .start_package(quota, &reservation, &turn)
                .unwrap()
                .unwrap();
            turn.stop_after_signing();
            assert!(
                owner
                    .issue_package(quota, &reservation, started, &turn)
                    .is_err()
            );
            assert_eq!(turn.signs(), 1);
            assert!(turn.sample().is_err());
        }
        "issue" => {
            let started = owner
                .start_package(quota, &reservation, &window())
                .unwrap()
                .unwrap();
            let point = Instant::now();
            let turn = window();
            let artifact = owner
                .issue_package(quota, &reservation, started, &turn)
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
            assert_eq!(read.artifact.id.as_slice(), id);
            assert_eq!(read.stage.binding(), reservation.stage_binding());
            assert_eq!(read.package.event.id, read.artifact.id);
            assert_eq!(
                read.artifact,
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
                State::Prepared(read.file, read.artifact)
            );
            let incorrect = Artifact {
                digest: [0; 32],
                ..read.artifact
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
        "fenced" | "empty" | "unsigned" => {
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
            if mode == "unsigned" {
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
                assert!(
                    !inspection
                        .provisional_is_signed(Wall::System.sample().unwrap())
                        .unwrap()
                );
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
