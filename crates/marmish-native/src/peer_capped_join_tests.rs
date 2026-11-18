use super::*;
use crate::peer_invitation::provisional::tests::Fixture;
use crate::peer_quota_vfs::dispatch::mount_for_proof;
use consent_checkpoint::wire::{Codec, MAX_OUTER_BYTES};
use std::{
    process::Command,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant as Stopwatch,
};

const MODE: &str = "MARMISH_CAPPED_WELCOME_PROOF_MODE";
const ORIGIN: &str = "wss://stage-fixture.example/";
const DEADLINE: Duration = Duration::from_secs(120);
const AGGREGATE_LIMIT: u64 = 128 * 1024 * 1024;

#[test]
#[ignore = "opt-in isolated real native Welcome, capped readback and refusal"]
fn peer_capped_welcome_native_proof() {
    if let Ok(mode) = std::env::var(MODE) {
        mount_for_proof().unwrap();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(qualify(&mode))
            })
            .unwrap()
            .join()
            .unwrap();
        return;
    }
    let test = format!("{}::peer_capped_welcome_native_proof", module_path!());
    let test = test.split_once("::").unwrap().1;
    for mode in ["join", "capacity", "mutation", "cancellation"] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                test,
                "--nocapture",
                "--test-threads=1",
            ])
            .env(MODE, mode)
            .spawn()
            .unwrap();
        let start = Stopwatch::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "capped Welcome mode {mode}: {status}");
                break;
            }
            if start.elapsed() >= DEADLINE {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("capped Welcome mode {mode} timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

async fn qualify(mode: &str) {
    let setup = Stopwatch::now();
    let fixture = Fixture::new();
    drop(fixture.start(Policy::default()));
    let route = Route::configured(ORIGIN).unwrap();
    drop(
        PreparedConsentOwner::open(
            &fixture.lease,
            &fixture.identity,
            Role::Both,
            Policy::default(),
            &route,
            Wall::Fixture(fixture.wall.clone()),
        )
        .unwrap(),
    );
    let mut owner = PreparedConsentOwner::open_custody(
        &fixture.lease,
        &fixture.identity,
        Role::Both,
        Policy::default(),
        &route,
        Wall::Fixture(fixture.wall.clone()),
    )
    .unwrap();
    let founder_dir = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let (fixture_stage, _founder) =
        postjoin_tests::reach_held(&fixture, &mut owner, founder_dir.path()).await;
    let held = fixture_stage.row;
    let delivery = fixture_stage.delivery;
    let (binding, witness) = held.stage();
    let original = owner
        .state
        .paths
        .reopen_provisional(binding, witness)
        .unwrap();
    let path = original.path();
    let known = path.with_file_name("provisional-known.db");
    let snapshot = path.with_file_name("terminal-leave-snapshot.db");
    let initial = original.digest().unwrap();
    let length = original.read_file().unwrap().metadata().unwrap().len();
    let setup_time = setup.elapsed();
    let original_path = path.to_owned();
    let adapter = CappedJoin::new(&fixture.lease, 0).unwrap();
    let ticks = AtomicU64::new(10);
    let stop = AtomicBool::new(false);
    let wall = Wall::Fixture(fixture.wall.clone());
    let fence = || {
        TurnFence::new(
            &wall,
            &stop,
            Some(&ticks),
            fixture.now,
            held.held_deadline(),
        )
        .unwrap()
    };

    let other = Codec::new(MAX_OUTER_BYTES)
        .unwrap()
        .prepare_delivery(
            &fixture.server,
            &fixture_stage.consent,
            &fixture_stage.welcome.payload,
            fixture.now,
            fixture.now + 360,
        )
        .unwrap();
    let other = Codec::new(MAX_OUTER_BYTES)
        .unwrap()
        .open_delivery(
            &fixture.identity.signer(),
            other.frame(),
            &fixture_stage.consent,
            fixture.now,
        )
        .unwrap();
    assert_ne!(other.flight().carrier_id, delivery.flight().carrier_id);
    assert_eq!(
        adapter.prepare(&owner.state, &held, &other, &fence()).err(),
        Some(Error::Binding)
    );
    assert_eq!(
        adapter
            .read_joined(&owner.state, &held, &delivery, &fence())
            .err(),
        Some(Error::Binding)
    );
    assert!(!snapshot.exists());
    if mode == "mutation" {
        let saved = path.with_file_name("fixture-saved-stage");
        fs::rename(path, &saved).unwrap();
        let replacement = create_private(path);
        replacement.set_len(length).unwrap();
        drop(replacement);
        assert_eq!(
            adapter
                .prepare(&owner.state, &held, &delivery, &fence())
                .err(),
            Some(Error::StageIdentityMismatch)
        );
        fs::remove_file(path).unwrap();
        fs::rename(saved, path).unwrap();
    }

    if mode == "join" {
        let occupied = create_private(&snapshot);
        occupied.set_len(8).unwrap();
        assert_eq!(
            adapter
                .prepare(&owner.state, &held, &delivery, &fence())
                .err(),
            Some(Error::Path)
        );
        assert_eq!(occupied.metadata().unwrap().len(), 8);
        drop(occupied);
        fs::remove_file(&snapshot).unwrap();
    }
    let budget = if mode == "capacity" {
        let budget = create_private(&known);
        budget.set_len(AGGREGATE_LIMIT - length * 2 + 1).unwrap();
        assert_eq!(
            adapter
                .prepare(&owner.state, &held, &delivery, &fence())
                .err(),
            Some(Error::Path)
        );
        assert!(!snapshot.exists());
        budget.set_len(AGGREGATE_LIMIT - length * 2).unwrap();
        Some(budget)
    } else {
        None
    };

    let prepare_start = Stopwatch::now();
    let prepared = adapter
        .prepare(&owner.state, &held, &delivery, &fence())
        .unwrap();
    let prepare_time = prepare_start.elapsed();
    assert_eq!(prepared.digest, initial);
    assert!(!snapshot.exists());
    assert_eq!(original.digest().unwrap(), initial);

    let start = owner
        .state
        .checkpoint
        .start_custody_join(&owner.state.source, &held, fence().wall().unwrap())
        .unwrap();
    if mode == "cancellation" {
        stop.store(true, Ordering::Release);
        assert_eq!(
            adapter
                .join(&owner.state, &held, &delivery, &fence(), prepared, start)
                .err(),
            Some(Error::Clock)
        );
        stop.store(false, Ordering::Release);
        let started = read_state(
            &mut owner,
            fixture_stage.selector,
            fixture_stage.generation,
            fixture.now,
        );
        assert_eq!(started.state(), CustodyState::JoinStarted);
        assert!(
            owner
                .state
                .checkpoint
                .start_custody_join(&owner.state.source, &held, fixture.now)
                .is_err()
        );
        assert!(
            adapter
                .read_joined(&owner.state, &started, &delivery, &fence())
                .is_err()
        );
        assert!(!snapshot.exists());
        assert_eq!(original.digest().unwrap(), initial);
        println!("capped Welcome mode=cancellation guarded=1");
        return;
    }
    if mode == "mutation" {
        let mut image = fs::read(&original_path).unwrap();
        let tail = image.last_mut().unwrap();
        *tail ^= 1;
        fs::write(&original_path, &image).unwrap();
        assert_eq!(
            adapter
                .join(&owner.state, &held, &delivery, &fence(), prepared, start)
                .err(),
            Some(Error::StageIdentityMismatch)
        );
        *image.last_mut().unwrap() ^= 1;
        fs::write(&original_path, &image).unwrap();
        assert!(
            owner
                .state
                .checkpoint
                .start_custody_join(&owner.state.source, &held, fixture.now)
                .is_err()
        );
        let started = read_state(
            &mut owner,
            fixture_stage.selector,
            fixture_stage.generation,
            fixture.now,
        );
        stop.store(true, Ordering::Release);
        assert_eq!(
            adapter
                .read_joined(&owner.state, &started, &delivery, &fence())
                .err(),
            Some(Error::Clock)
        );
        stop.store(false, Ordering::Release);
        fixture.wall.store(held.held_deadline(), Ordering::Release);
        assert_eq!(
            adapter
                .read_joined(&owner.state, &started, &delivery, &fence())
                .err(),
            Some(Error::Clock)
        );
        fixture.wall.store(fixture.now, Ordering::Release);
        assert!(
            adapter
                .read_joined(&owner.state, &started, &delivery, &fence())
                .is_err()
        );
        assert!(!snapshot.exists());
        assert_eq!(original.digest().unwrap(), initial);
        println!("capped Welcome mode=mutation guarded=1");
        return;
    }

    if let Some(budget) = &budget {
        budget.set_len(AGGREGATE_LIMIT - length).unwrap();
    }
    // A new bounded window after the first operation's five seconds elapsed.
    ticks.store(6_010, Ordering::Release);
    let join_start = Stopwatch::now();
    if let Some(budget) = budget {
        let (outcome, metrics) = adapter
            .join_once(&owner.state, &held, &delivery, &fence(), prepared, start)
            .unwrap();
        let join_time = join_start.elapsed();
        assert!(outcome.is_err());
        assert!(metrics.main_opens > 0);
        assert!(metrics.rejected.iter().sum::<u64>() > 0);
        assert!(metrics.peak <= length);
        assert!(!snapshot.exists());
        assert_eq!(original.digest().unwrap(), initial);
        drop(budget);
        fs::remove_file(known).unwrap();
        let started = read_state(
            &mut owner,
            fixture_stage.selector,
            fixture_stage.generation,
            fixture.now,
        );
        assert_eq!(started.state(), CustodyState::JoinStarted);
        assert!(
            adapter
                .read_joined(&owner.state, &started, &delivery, &fence())
                .is_err()
        );
        println!(
            "capped Welcome mode=capacity setup_ms={} prepare_ms={} join_ms={} managed_peak={} rejected_main={} rejected_journal={} main_opens={}",
            setup_time.as_millis(),
            prepare_time.as_millis(),
            join_time.as_millis(),
            metrics.peak,
            metrics.rejected[0],
            metrics.rejected[1],
            metrics.main_opens
        );
        return;
    }
    adapter
        .join(&owner.state, &held, &delivery, &fence(), prepared, start)
        .unwrap();
    let join_time = join_start.elapsed();
    assert!(
        owner
            .state
            .checkpoint
            .start_custody_join(&owner.state.source, &held, fixture.now)
            .is_err()
    );
    let started = read_state(
        &mut owner,
        fixture_stage.selector,
        fixture_stage.generation,
        fixture.now,
    );
    assert_eq!(started.state(), CustodyState::JoinStarted);
    let read_start = Stopwatch::now();
    let readback: CappedJoinedReadback = adapter
        .read_joined(&owner.state, &started, &delivery, &fence())
        .unwrap();
    let read_time = read_start.elapsed();
    assert_eq!(
        readback.proof().coordinates().0,
        fixture_stage.group.as_slice()
    );
    let native = readback.proof().authority.clone();
    let joined_image = original.digest().unwrap();
    assert_ne!(joined_image, initial);
    owner
        .state
        .checkpoint
        .commit_custody_pending(
            &owner.state.source,
            &started,
            readback.proof(),
            fence().wall().unwrap(),
        )
        .unwrap();
    let pending = read_state(
        &mut owner,
        fixture_stage.selector,
        fixture_stage.generation,
        fixture.now,
    );
    assert!(pending.matches_pending_readback(readback.proof()));
    let reread = adapter
        .read_joined(&owner.state, &pending, &delivery, &fence())
        .unwrap();
    assert_eq!(reread.proof().authority, native);
    assert_eq!(original.digest().unwrap(), joined_image);
    assert!(!snapshot.exists());
    println!(
        "capped Welcome mode=join setup_ms={} prepare_ms={} join_ms={} read_ms={}",
        setup_time.as_millis(),
        prepare_time.as_millis(),
        join_time.as_millis(),
        read_time.as_millis()
    );
}

fn read_state(
    owner: &mut PreparedConsentOwner<'_>,
    selector: Selector,
    generation: u64,
    wall: u64,
) -> HeldEvidence {
    owner
        .state
        .checkpoint
        .read_custody(&owner.state.source, selector, generation, wall)
        .unwrap()
        .unwrap()
}

fn create_private(path: &Path) -> fs::File {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap()
}
