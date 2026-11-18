use super::super::provisional::{self, tests::Fixture};
use super::*;

use cgka_traits::app_components::NostrRoutingV1;
use nostr::event::Event;
use std::{
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    path::Path,
    sync::atomic::Ordering,
};
use store::TrustedOutcome;
use store::consent_checkpoint::{
    provisioning::Reservation,
    wire::{Codec, MAX_OUTER_BYTES, Offer, Prepared},
};

const ORIGIN: &str = "wss://stage-fixture.example/";
const SECOND: &str = "wss://stage-fixture-two.example/";
const STOP: Error = Error::Clock;
const DAY: u64 = 86_400;

fn reopen(f: &Fixture, policy: Policy) -> InvitationState<'_> {
    InvitationState::open_retirement(
        &f.lease,
        &f.identity,
        policy,
        &[Route::configured(ORIGIN).unwrap()],
        Wall::Fixture(f.wall.clone()),
    )
    .unwrap()
}

fn local(state: &InvitationState<'_>) -> Local {
    // SAFETY: reads the current test process's effective UID.
    state
        .checkpoint
        .authorize_local(unsafe { libc::geteuid() })
        .unwrap()
}

fn target(state: &InvitationState<'_>, key: Selector) -> (Local, RetirementTarget) {
    let local = local(state);
    let target = state.checkpoint.retirement_target(&local, key).unwrap();
    (local, target)
}

fn initialize_bound(f: &Fixture, policy: Policy) -> (Selector, Reservation) {
    let mut state = f.start(policy);
    let (selector, reservation) = f.reserve(&mut state);
    state
        .checkpoint
        .bind_stage(&state.source, &reservation, f.now)
        .unwrap();
    (selector, reservation)
}

fn placeholder(path: &Path) {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap();
}

fn cut(expected: Cut) -> impl FnMut(Cut) -> Result<()> {
    move |current| {
        if current == expected {
            Err(STOP)
        } else {
            Ok(())
        }
    }
}

fn offer(f: &Fixture, server: &Keys, origin: &str, id: u8, created: u64) -> Prepared<Offer> {
    let routing = NostrRoutingV1::new([7; 32], vec![origin.to_owned()]).unwrap();
    Codec::new(MAX_OUTER_BYTES)
        .unwrap()
        .prepare_offer(server, f.identity.public_key(), [id; 16], &routing, created)
        .unwrap()
}

fn admit(
    state: &mut InvitationState<'_>,
    origin: &str,
    frame: &Prepared<Offer>,
    wall: u64,
) -> Status {
    let source = state.checkpoint.bind_source(origin).unwrap();
    let event: Event = serde_json::from_slice(frame.frame()).unwrap();
    state
        .checkpoint
        .admit(
            &source,
            RawRecord {
                id: event.id.to_bytes(),
                stamp: event.created_at.as_secs(),
                bytes: frame.frame(),
            },
            wall,
        )
        .unwrap()
        .status
}

#[test]
fn peer_retirement_recovers_each_local_cut_with_exact_release() {
    for boundary in [Cut::Intent, Cut::Unlinked, Cut::Synced, Cut::FilesRemoved] {
        let f = Fixture::new();
        let (key, _) = initialize_bound(&f, Policy::default());
        placeholder(&f.stage());
        let mut state = reopen(&f, Policy::default());
        let (local, retire) = target(&state, key);
        assert_eq!(
            state.decline_stage(&local, &retire, cut(boundary)),
            Err(STOP)
        );
        assert_eq!(f.stage().exists(), boundary == Cut::Intent);
        drop(state);

        let mut state = reopen(
            &f,
            Policy {
                mode: consent_checkpoint::Mode::Off,
                ..Policy::default()
            },
        );
        let (local, retire) = target(&state, key);
        assert_eq!(
            state.decline_stage(&local, &retire, |_| Ok(())),
            Ok(Outcome::Released)
        );
        assert!(!f.stage().exists());
        assert_eq!(
            state.decline_stage(&local, &retire, |_| panic!()),
            Ok(Outcome::AlreadyReleased)
        );
        drop(state);

        let mut state = reopen(&f, Policy::default());
        let local = self::local(&state);
        assert!(
            state
                .checkpoint
                .released_target(&local, key, true, 1)
                .is_err()
        );
        assert!(
            state
                .checkpoint
                .released_target(&local, key, false, 2)
                .is_err()
        );
        let exact = state
            .checkpoint
            .released_target(&local, key, false, 1)
            .unwrap();
        assert_eq!(
            state.decline_stage(&local, &exact, |_| panic!()),
            Ok(Outcome::AlreadyReleased)
        );
        assert_eq!(
            state
                .checkpoint
                .examine_retirement(&local, &exact, Action::Decline, f.now + DAY)
                .err(),
            Some(consent_checkpoint::Error::Missing),
        );
    }
}

#[tokio::test]
async fn peer_retirement_retires_native_started_and_prepared_file_only() {
    for prepared in [false, true] {
        let f = Fixture::new();
        let mut v1 = f.start(Policy::default());
        let (key, reservation) = f.reserve(&mut v1);
        let result = v1
            .prepare_provisional(&reservation, |cut| {
                if !prepared && cut == provisional::Cut::NativeStarted {
                    Err(STOP)
                } else {
                    Ok(())
                }
            })
            .await;
        assert_eq!(result, if prepared { Ok(()) } else { Err(STOP) });
        let length = fs::metadata(f.stage()).unwrap().len();
        assert_eq!(length > 0, prepared);
        drop(v1);

        let mut state = reopen(&f, Policy::default());
        let (local, retire) = target(&state, key);
        assert_eq!(
            state.decline_stage(&local, &retire, |_| Ok(())),
            Ok(Outcome::Released)
        );
        assert!(!f.stage().exists());
    }
}

#[test]
fn peer_retirement_blocks_ambiguous_path_before_any_unlink() {
    for path_change in ["symlink", "hardlink", "wrong_mode", "journal", "wal", "shm"] {
        let f = Fixture::new();
        let (key, _) = initialize_bound(&f, Policy::default());
        let stage = f.stage();
        let outside = f.lease.root().join("other");
        placeholder(&outside);
        match path_change {
            "symlink" => symlink(&outside, &stage).unwrap(),
            "hardlink" => fs::hard_link(&outside, &stage).unwrap(),
            "wrong_mode" => {
                placeholder(&stage);
                fs::set_permissions(&stage, fs::Permissions::from_mode(0o640)).unwrap();
            }
            suffix => {
                placeholder(&stage);
                placeholder(&stage.with_extension(format!("db-{suffix}")));
            }
        }
        let mut state = reopen(&f, Policy::default());
        let existing = state
            .checkpoint
            .recover_provisioning(&state.source, key, f.now)
            .unwrap();
        let (local, retire) = target(&state, key);
        assert_eq!(
            state.decline_stage(&local, &retire, |_| Ok(())),
            Ok(Outcome::Blocked)
        );
        assert!(stage.exists() && outside.exists());
        assert_eq!(
            state
                .checkpoint
                .bind_stage(&state.source, &existing, f.now)
                .err(),
            Some(consent_checkpoint::Error::Retiring)
        );
        assert_eq!(
            state.decline_stage(&local, &retire, |_| panic!()),
            Ok(Outcome::Blocked)
        );
    }
}

#[test]
fn peer_retirement_retries_an_unavailable_cursor_path_before_intent() {
    let f = Fixture::new();
    let (key, _) = initialize_bound(&f, Policy::default());
    let stage = f.stage();
    placeholder(&stage);
    let mut state = reopen(&f, Policy::default());
    let (local, retire) = target(&state, key);
    let cursor = stage.parent().unwrap().join("cursor.db");
    let saved = f.lease.root().join("saved-cursor.db");
    fs::rename(&cursor, &saved).unwrap();
    let result = state.decline_stage(&local, &retire, |_| panic!("committed an intent"));
    fs::rename(&saved, &cursor).unwrap();
    assert_eq!(result, Err(Error::Path));
    assert!(stage.exists());
    let reservation = state
        .checkpoint
        .recover_provisioning(&state.source, key, f.now)
        .unwrap();
    assert!(
        state
            .checkpoint
            .bind_stage(&state.source, &reservation, f.now)
            .is_ok()
    );
    assert_eq!(
        state.decline_stage(&local, &retire, |_| Ok(())),
        Ok(Outcome::Released)
    );
}

#[test]
fn peer_retirement_preserves_replaced_dir_and_changed_stage_without_release() {
    for changed in [
        "directory",
        "replaced_leaf",
        "leaf_appeared",
        "reappeared_after_sync",
    ] {
        let f = Fixture::new();
        let (key, _) = initialize_bound(&f, Policy::default());
        let stage = f.stage();
        if changed != "leaf_appeared" {
            placeholder(&stage);
        }
        let mut state = reopen(&f, Policy::default());
        let (local, retire) = target(&state, key);
        let boundary = if changed == "reappeared_after_sync" {
            Cut::FilesRemoved
        } else {
            Cut::Intent
        };
        assert_eq!(
            state.decline_stage(&local, &retire, cut(boundary)),
            Err(STOP)
        );
        match changed {
            "directory" => {
                let parent = stage.parent().unwrap();
                let cursor = parent.join("cursor.db");
                let saved = f.lease.root().join("old-peer");
                fs::remove_file(&stage).unwrap();
                fs::rename(parent, &saved).unwrap();
                fs::create_dir(parent).unwrap();
                fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
                fs::rename(saved.join("cursor.db"), cursor).unwrap();
            }
            "replaced_leaf" => {
                // Rename rather than unlink so the first inode cannot be reused.
                fs::rename(&stage, f.lease.root().join("old-leaf")).unwrap();
                placeholder(&stage);
            }
            _ => placeholder(&stage),
        }
        assert_eq!(
            state.decline_stage(&local, &retire, |_| panic!()),
            Err(Error::StageIdentityMismatch)
        );
        assert_eq!(stage.exists(), changed != "directory");
        assert!(state.checkpoint.retirement_target(&local, key).is_ok());
    }
}

#[test]
fn peer_retirement_decline_cooldown_across_origins_and_classes() {
    let f = Fixture::new();
    let routes = [
        Route::configured(ORIGIN).unwrap(),
        Route::configured(SECOND).unwrap(),
    ];
    let paths = Paths::new(&f.lease, &f.identity).unwrap();
    let checked = paths.cursor().unwrap();
    let connection = HardenedPeerConnection::open(checked.path(), *paths.cursor_key()).unwrap();
    let cursor = CursorStore::attach(
        connection,
        f.identity.public_key().to_bytes(),
        &[routes[0].id, routes[1].id],
    )
    .unwrap();
    let reserved = [f.server.public_key()].into_iter().collect();
    let policy = Policy {
        reserved,
        ..Policy::default()
    };
    // SAFETY: reads the current process's effective UID for fixture setup.
    let uid = unsafe { libc::geteuid() };
    let checkpoint = Checkpoint::attach_for_provisional_proof(
        cursor,
        (*f.identity.signer()).clone(),
        uid,
        &[ORIGIN, SECOND],
        Policy::default(),
    )
    .unwrap();
    drop(checkpoint);
    let mut state = InvitationState::open_retirement(
        &f.lease,
        &f.identity,
        policy,
        &routes,
        Wall::Fixture(f.wall.clone()),
    )
    .unwrap();
    let frame = offer(&f, &f.server, ORIGIN, 1, f.now);
    let selector = Selector::from(&frame.flight);
    assert_eq!(
        admit(&mut state, ORIGIN, &frame, f.now),
        Status::Accepted(TrustedOutcome::Applied)
    );
    let reservation = state
        .checkpoint
        .reserve_provisioning(&state.source, selector, f.now)
        .unwrap();
    assert!(reservation.stage_binding().coordinates().0);
    state
        .checkpoint
        .bind_stage(&state.source, &reservation, f.now)
        .unwrap();
    let (local, target) = target(&state, selector);
    assert_eq!(
        state.decline_stage(&local, &target, |_| Ok(())),
        Ok(Outcome::Released)
    );

    let fresh = offer(&f, &f.server, SECOND, 2, f.now);
    for (origin, offer) in [(ORIGIN, &frame), (SECOND, &fresh)] {
        assert_eq!(
            admit(&mut state, origin, offer, f.now),
            Status::Accepted(TrustedOutcome::SkippedCooldown)
        );
    }
    let stranger = Keys::generate();
    let frame = offer(&f, &stranger, SECOND, 3, f.now);
    let unknown = Selector::from(&frame.flight);
    assert_eq!(
        admit(&mut state, SECOND, &frame, f.now),
        Status::Accepted(TrustedOutcome::Applied)
    );
    let source = state.checkpoint.bind_source(SECOND).unwrap();
    assert!(
        !state
            .checkpoint
            .reserve_provisioning(&source, unknown, f.now)
            .unwrap()
            .stage_binding()
            .coordinates()
            .0
    );
    let fresh = offer(&f, &f.server, SECOND, 4, f.now + DAY);
    assert_eq!(
        admit(&mut state, SECOND, &fresh, f.now + DAY),
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert_eq!(
        state
            .checkpoint
            .reserve_provisioning(&source, Selector::from(&fresh.flight), f.now + DAY)
            .unwrap()
            .stage_binding()
            .coordinates()
            .1,
        2
    );
}

#[test]
fn peer_retirement_owned_clock_expiry_keeps_signed_replay_and_rate_charge() {
    let f = Fixture::new();
    let mut state = f.start(Policy::default());
    let expiring = offer(&f, &f.server, ORIGIN, 1, f.now - DAY + 1);
    let key = Selector::from(&expiring.flight);
    assert_eq!(
        admit(&mut state, ORIGIN, &expiring, f.now),
        Status::Accepted(TrustedOutcome::Applied)
    );
    let reservation = state
        .checkpoint
        .reserve_provisioning(&state.source, key, f.now)
        .unwrap();
    state
        .checkpoint
        .bind_stage(&state.source, &reservation, f.now)
        .unwrap();
    drop(state);
    let mut state = reopen(&f, Policy::default());
    let (local, retire) = target(&state, key);
    assert_eq!(
        state.expire_stage(&retire, |_| panic!()),
        Err(Error::Checkpoint(consent_checkpoint::Error::NotExpired))
    );
    f.wall.store(f.now + 1, Ordering::Relaxed);
    assert_eq!(state.expire_stage(&retire, cut(Cut::Intent)), Err(STOP));
    assert_eq!(
        state.decline_stage(&local, &retire, |_| Ok(())),
        Ok(Outcome::Released)
    );
    let db = state.checkpoint.retirement_database_for_fixture();
    let replay_deadline = db
        .query_row(
            "SELECT replay_until FROM peer_consent_fence WHERE revision=?1",
            [key.revision],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    assert_eq!(
        u64::try_from(replay_deadline).unwrap(),
        expiring.flight.flight().replay_safe_until
    );
    let replacement = offer(&f, &f.server, ORIGIN, 2, f.now + 1);
    let new_key = Selector::from(&replacement.flight);
    assert_eq!(
        admit(&mut state, ORIGIN, &replacement, f.now + 1),
        Status::Accepted(TrustedOutcome::Applied)
    );
    assert_eq!(
        state
            .checkpoint
            .reserve_provisioning(&state.source, new_key, f.now + 599)
            .err(),
        Some(consent_checkpoint::Error::Cursor(store::Error::Backlog))
    );
    let next = state
        .checkpoint
        .reserve_provisioning(&state.source, new_key, f.now + 600)
        .unwrap();
    assert_eq!(next.stage_binding().coordinates().1, 2);
}

#[test]
fn peer_retirement_mount_refuses_blank_changed_and_older_constructors() {
    let f = Fixture::new();
    let mount = || {
        InvitationState::open_retirement(
            &f.lease,
            &f.identity,
            Policy::default(),
            &[Route::configured(ORIGIN).unwrap()],
            Wall::Fixture(f.wall.clone()),
        )
    };
    assert_eq!(
        mount().err(),
        Some(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::Binding
        )))
    );
    let state = f.start(Policy::default());
    state
        .checkpoint
        .fixture_statement("CREATE TABLE unrecognized (i INTEGER)");
    drop(state);
    assert!(mount().is_err());
    let state = f.start(Policy::default());
    state
        .checkpoint
        .fixture_statement("DROP TABLE unrecognized; DROP TABLE peer_consent_fence;");
    drop(state);
    assert!(mount().is_err());
    let f = Fixture::new();
    drop(f.start(Policy::default()));
    drop(reopen(&f, Policy::default()));
    for open in [Checkpoint::attach, Checkpoint::attach_for_provisional_proof] {
        assert_eq!(
            f.open(Policy::default(), open).err(),
            Some(Error::Checkpoint(consent_checkpoint::Error::Cursor(
                store::Error::Binding
            )))
        );
    }
}

#[test]
fn peer_retirement_guards_protect_sql_and_profile_capability() {
    let f = Fixture::new();
    let (key, _) = initialize_bound(&f, Policy::default());
    let mut state = reopen(&f, Policy::default());
    let (local, retire) = target(&state, key);
    // SAFETY: reads the current process's effective UID for wrong-UID comparison.
    assert!(
        state
            .checkpoint
            .authorize_local(unsafe { libc::geteuid() } ^ 1)
            .is_err()
    );
    let other = Fixture::new();
    drop(other.start(Policy::default()));
    let other = reopen(&other, Policy::default());
    assert_eq!(
        state.decline_stage(&self::local(&other), &retire, |_| panic!()),
        Err(Error::Checkpoint(consent_checkpoint::Error::LocalOwner))
    );
    assert_eq!(
        state.decline_stage(&local, &retire, cut(Cut::Intent)),
        Err(STOP)
    );
    state
        .checkpoint
        .fixture_statement("PRAGMA foreign_keys=OFF; PRAGMA recursive_triggers=OFF;");
    for sql in [
        "DELETE FROM peer_retirement_intent",
        "UPDATE OR REPLACE peer_retirement_intent SET started=started+1",
        "INSERT OR REPLACE INTO peer_retirement_intent SELECT * FROM peer_retirement_intent",
        "DELETE FROM peer_retirement_cooldown",
        "UPDATE OR REPLACE peer_retirement_cooldown SET until=until+1",
        "INSERT OR REPLACE INTO peer_retirement_cooldown SELECT * FROM peer_retirement_cooldown",
        "UPDATE peer_provisional_stage_v1 SET state=1, device=zeroblob(8), inode=zeroblob(8)",
        "DELETE FROM peer_retirement_epoch",
    ] {
        assert!(
            state.checkpoint.fixture_try_statement(sql, None).is_err(),
            "{sql}"
        );
    }
    state
        .checkpoint
        .fixture_statement("PRAGMA foreign_keys=ON;");
    assert_eq!(
        state.decline_stage(&local, &retire, |_| Ok(())),
        Ok(Outcome::Released)
    );
}

#[test]
#[ignore = "opt-in SQLCipher retirement and verified restart timing"]
fn peer_retirement_local_latency() {
    let (mut decline, mut restart) = (Vec::new(), Vec::new());
    for boundary in [None, Some(Cut::FilesRemoved)] {
        for _ in 0..20 {
            let f = Fixture::new();
            let (key, _) = initialize_bound(&f, Policy::default());
            placeholder(&f.stage());
            let mut state = reopen(&f, Policy::default());
            let (local, retire) = target(&state, key);
            if let Some(boundary) = boundary {
                assert_eq!(
                    state.decline_stage(&local, &retire, cut(boundary)),
                    Err(STOP)
                );
                drop(state);
                let start = std::time::Instant::now();
                let mut state = reopen(&f, Policy::default());
                let (local, retire) = target(&state, key);
                assert_eq!(
                    state.decline_stage(&local, &retire, |_| Ok(())),
                    Ok(Outcome::Released)
                );
                restart.push(start.elapsed().as_micros());
            } else {
                let start = std::time::Instant::now();
                assert_eq!(
                    state.decline_stage(&local, &retire, |_| Ok(())),
                    Ok(Outcome::Released)
                );
                decline.push(start.elapsed().as_micros());
            }
        }
    }
    for (phase, mut sample) in [("decline", decline), ("restart_release", restart)] {
        sample.sort_unstable();
        println!(
            "peer_retirement {phase} n={} p50_us={} p95_us={}",
            sample.len(),
            sample[9],
            sample[18]
        );
    }
}

#[test]
fn peer_retirement_rolls_back_every_journal_change_and_is_exclusive() {
    use store::consent_checkpoint::provisioning::retirement::Fault;

    for failure in [Fault::Intent, Fault::FilesRemoved, Fault::Release] {
        let f = Fixture::new();
        let (key, _) = initialize_bound(&f, Policy::default());
        placeholder(&f.stage());
        let mut state = reopen(&f, Policy::default());
        let checked = state.paths.cursor().unwrap();
        let second =
            HardenedPeerConnection::open(checked.path(), *state.paths.cursor_key()).unwrap();
        state.checkpoint.probe_retirement_guard(second).unwrap();
        let (local, retire) = target(&state, key);
        state.checkpoint.inject_retirement_fault(failure);
        assert_eq!(
            state.decline_stage(&local, &retire, |_| Ok(())),
            Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
                store::Error::Storage
            )))
        );
        assert_eq!(f.stage().exists(), failure == Fault::Intent);
        let current = state
            .checkpoint
            .examine_retirement(&local, &retire, Action::Decline, f.now)
            .unwrap()
            .unwrap();
        match failure {
            Fault::Intent => assert!(current.step.is_none()),
            Fault::FilesRemoved => assert!(matches!(current.step, Some(Step::Retiring(_)))),
            Fault::Release => assert!(matches!(current.step, Some(Step::FilesRemoved(_)))),
        }
        drop(state);
        let mut state = reopen(&f, Policy::default());
        let (local, retire) = target(&state, key);
        assert_eq!(
            state.decline_stage(&local, &retire, |_| Ok(())),
            Ok(Outcome::Released)
        );
    }
}

fn replace_guard_for_fixture(db: &rusqlite::Connection, guard: &str, sql: &str) {
    let restored: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1",
            [guard],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch(&format!("DROP TRIGGER {guard}; {sql}; {restored};"))
        .unwrap();
}

#[test]
fn peer_retirement_rejects_mutated_signed_offer_under_exact_schema() {
    let f = Fixture::new();
    let (key, _) = initialize_bound(&f, Policy::default());
    placeholder(&f.stage());
    let mut state = reopen(&f, Policy::default());
    let (local, retire) = target(&state, key);
    replace_guard_for_fixture(
        state.checkpoint.retirement_database_for_fixture(),
        "peer_stage_invite_update_guard",
        "UPDATE peer_consent_invite SET frame=x'00'",
    );
    assert_eq!(
        state.decline_stage(&local, &retire, |_| panic!()),
        Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::Binding
        ))),
    );
    assert!(f.stage().exists());
}

#[test]
fn peer_retirement_capacity_and_sql_arithmetic_fail_closed() {
    use store::consent_checkpoint::provisioning::stage_journal::State;

    let f = Fixture::new();
    let (key, _) = initialize_bound(&f, Policy::default());
    placeholder(&f.stage());
    let mut state = reopen(&f, Policy::default());
    let (local, target) = target(&state, key);
    let max_wall = i64::MAX as u64;
    assert_eq!(
        state.checkpoint.begin_retirement(
            &local,
            &target,
            State::Bound,
            Action::Decline,
            None,
            max_wall
        ),
        Err(consent_checkpoint::Error::Cursor(store::Error::Range)),
    );
    replace_guard_for_fixture(
        state.checkpoint.retirement_database_for_fixture(),
        "peer_retirement_cooldown_INSERT",
        "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM ids WHERE n<512)
            INSERT INTO peer_retirement_cooldown SELECT CAST(printf('%032d',n) AS BLOB),9223372036854775807 FROM ids",
    );
    assert_eq!(
        state.decline_stage(&local, &target, |_| panic!()),
        Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::Backlog
        ))),
    );
    assert!(f.stage().exists());
    let examined = state
        .checkpoint
        .examine_retirement(&local, &target, Action::Decline, f.now)
        .unwrap()
        .unwrap();
    assert!(examined.step.is_none());
}
