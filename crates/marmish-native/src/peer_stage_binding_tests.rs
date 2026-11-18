use crate::identity::BackendChoice;
use cgka_traits::app_components::NostrRoutingV1;
use nostr::{event::Event, types::Timestamp};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tempfile::TempDir;

use super::store::consent_checkpoint::wire::{Codec, MAX_OUTER_BYTES};
use super::*;

const ORIGIN: &str = "wss://stage-fixture.example/";
const FAULT: Error = Error::Clock;

pub(in super::super) struct Fixture {
    _private: Option<TempDir>,
    pub(in super::super) lease: ProfileLease,
    pub(in super::super) identity: IdentityMaterial,
    pub(in super::super) server: Keys,
    pub(in super::super) wall: Arc<AtomicU64>,
    pub(in super::super) now: u64,
}

impl Fixture {
    pub(in super::super) fn new() -> Self {
        let private = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let path = private.path().to_owned();
        Self::at(&path, Some(private), true)
    }

    pub(in super::super) fn at(path: &Path, private: Option<TempDir>, initialize: bool) -> Self {
        let mut lease = ProfileLease::acquire(path, "client").unwrap();
        let identity = lease
            .prepare(
                BackendChoice::File,
                Path::new(""),
                std::time::Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        if initialize {
            InvitationLane::initialize(&lease, &identity).unwrap();
        }
        let now = Timestamp::now().as_secs();
        Self {
            _private: private,
            lease,
            identity,
            server: Keys::generate(),
            wall: Arc::new(AtomicU64::new(now)),
            now,
        }
    }

    pub(in super::super) fn stage(&self) -> PathBuf {
        self.lease.root().join("peer/provisional-unknown.db")
    }

    pub(in super::super) fn start(&self, policy: Policy) -> InvitationState<'_> {
        self.open(policy, Checkpoint::attach_for_provisional_proof)
            .unwrap()
    }

    pub(in super::super) fn open(
        &self,
        policy: Policy,
        checkpoint: impl FnOnce(
            CursorStore,
            Keys,
            u32,
            &[&str],
            Policy,
        ) -> consent_checkpoint::Result<Checkpoint>,
    ) -> Result<InvitationState<'_>> {
        let (state, checked) = InvitationState::open(
            &self.lease,
            &self.identity,
            policy,
            &Route::configured(ORIGIN).unwrap(),
            Wall::Fixture(self.wall.clone()),
            checkpoint,
        )?;
        checked.unchanged()?;
        Ok(state)
    }

    pub(in super::super) fn admit(
        &self,
        state: &mut InvitationState<'_>,
        server: &Keys,
        created: u64,
    ) -> Selector {
        let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
        let routing = NostrRoutingV1::new([77; 32], vec![ORIGIN.to_owned()]).unwrap();
        let prepared = codec
            .prepare_offer(
                server,
                self.identity.public_key(),
                [91; 16],
                &routing,
                created,
            )
            .unwrap();
        let event: Event = serde_json::from_slice(prepared.frame()).unwrap();
        let incoming = RawRecord {
            id: event.id.to_bytes(),
            stamp: event.created_at.as_secs(),
            bytes: prepared.frame(),
        };
        assert!(matches!(
            state
                .checkpoint
                .admit(&state.source, incoming, self.now)
                .unwrap()
                .status,
            Status::Accepted(_)
        ));
        Selector::from(&prepared.flight)
    }

    pub(in super::super) fn reserve(
        &self,
        state: &mut InvitationState<'_>,
    ) -> (Selector, Reservation) {
        let selector = self.admit(state, &self.server, self.now);
        let reservation = state
            .checkpoint
            .reserve_provisioning(&state.source, selector, self.now)
            .unwrap();
        (selector, reservation)
    }
}

fn cut_at(at: Cut) -> impl FnMut(Cut) -> Result<()> {
    move |cut| if at == cut { Err(FAULT) } else { Ok(()) }
}

fn checkpoint_state(
    state: &mut InvitationState<'_>,
    reservation: &Reservation,
    wall: u64,
) -> State {
    state
        .checkpoint
        .bind_stage(&state.source, reservation, wall)
        .unwrap()
        .state()
}

#[test]
fn peer_stage_production_attach_refuses_test_variant() {
    let fixture = Fixture::new();
    assert!(fixture.open(Policy::default(), Checkpoint::attach).is_ok());
    assert!(
        fixture
            .open(Policy::default(), Checkpoint::attach_for_provisional_proof)
            .is_ok()
    );
    assert!(
        fixture
            .open(Policy::default(), Checkpoint::attach_for_provisional_proof)
            .is_ok()
    );
    let old_or_malformed =
        Error::Checkpoint(consent_checkpoint::Error::Cursor(store::Error::Binding));
    assert_eq!(
        fixture.open(Policy::default(), Checkpoint::attach).err(),
        Some(old_or_malformed)
    );
    fixture
        .start(Policy::default())
        .checkpoint
        .fixture_statement("DROP TRIGGER peer_stage_update_guard;");
    assert_eq!(
        fixture
            .open(Policy::default(), Checkpoint::attach_for_provisional_proof)
            .err(),
        Some(old_or_malformed)
    );
}

#[tokio::test]
async fn peer_stage_crash_before_native_fences_ambiguous_generation() {
    let fixture = Fixture::new();
    let mut state = fixture.start(Policy::default());
    let (selector, reservation) = fixture.reserve(&mut state);
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::PriorToBind))
            .await,
        Err(FAULT)
    );
    assert!(!fixture.stage().exists());
    assert!(
        !state
            .checkpoint
            .has_stage(&state.source, &reservation, fixture.now)
            .unwrap()
    );
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::Allocated))
            .await,
        Err(FAULT)
    );
    assert_eq!(fs::metadata(fixture.stage()).unwrap().len(), 0);
    assert_eq!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::Bound
    );
    let sidecar = fixture.stage().with_extension("db-journal");
    symlink(fixture.stage(), &sidecar).unwrap();
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::StageIdentityMismatch)
    );
    assert!(sidecar.is_symlink());
    fs::remove_file(&sidecar).unwrap();
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::NativeStarted))
            .await,
        Err(FAULT)
    );
    assert!(matches!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::NativeStarted(_)
    ));
    drop(state);
    let mut state = fixture.start(Policy::default());
    let reservation = state
        .checkpoint
        .recover_provisioning(&state.source, selector, fixture.now)
        .unwrap();
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::StageIdentityMismatch)
    );
    assert_eq!(fs::metadata(fixture.stage()).unwrap().len(), 0);
    fs::remove_file(fixture.stage()).unwrap();
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::StageIdentityMismatch)
    );
    assert!(!fixture.stage().exists());
}

#[tokio::test]
async fn peer_stage_native_artifact_recovered_exactly() {
    let fixture = Fixture::new();
    let mut state = fixture.start(Policy::default());
    let (selector, reservation) = fixture.reserve(&mut state);
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::NativePrepared))
            .await,
        Err(FAULT)
    );
    assert!(matches!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::NativeStarted(_)
    ));
    let native = fs::read(fixture.stage()).unwrap();
    drop(state);
    let mut state = fixture.start(Policy::default());
    let reservation = state
        .checkpoint
        .recover_provisioning(&state.source, selector, fixture.now)
        .unwrap();
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::Prepared))
            .await,
        Err(FAULT)
    );
    let completed = checkpoint_state(&mut state, &reservation, fixture.now);
    assert!(matches!(completed, State::Prepared(..)));
    assert_eq!(native, fs::read(fixture.stage()).unwrap());
    state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(native, fs::read(fixture.stage()).unwrap());
    assert_eq!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        completed
    );

    let old = fixture.stage().with_extension("held");
    fs::rename(fixture.stage(), &old).unwrap();
    fs::copy(&old, fixture.stage()).unwrap();
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::StageIdentityMismatch)
    );
    assert_eq!(native, fs::read(&old).unwrap());
    fs::remove_file(fixture.stage()).unwrap();
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::StageIdentityMismatch),
    );
    assert!(!fixture.stage().exists());
    fs::rename(&old, fixture.stage()).unwrap();
    fs::write(fixture.stage(), b"fixture-replaced stage bytes").unwrap();
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::Native(StageError::Storage)),
    );
    assert_eq!(
        fs::read(fixture.stage()).unwrap(),
        b"fixture-replaced stage bytes"
    );
    assert_eq!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        completed
    );
}

#[test]
fn peer_stage_exact_reference_graph_survives_sql_replace() {
    let fixture = Fixture::new();
    let other = Keys::generate();
    let mut state = fixture.start(Policy {
        reserved: [other.public_key()].into(),
        ..Policy::default()
    });
    let (selector, reservation) = fixture.reserve(&mut state);
    let alternative = fixture.admit(&mut state, &other, fixture.now);
    state
        .checkpoint
        .reserve_provisioning(&state.source, alternative, fixture.now)
        .unwrap();
    let local = state
        .checkpoint
        .authorize_local(unsafe { libc::geteuid() })
        .unwrap();
    let retire = state
        .checkpoint
        .retirement_target(&local, selector)
        .unwrap();
    assert_eq!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::Bound
    );
    assert_eq!(
        state
            .checkpoint
            .retire_provisioning(&local, retire, fixture.now),
        Err(consent_checkpoint::Error::Bound)
    );
    let conflict = Err(consent_checkpoint::Error::Cursor(store::Error::Conflict));
    assert_eq!(
        state.checkpoint.approve(&local, selector, fixture.now),
        conflict
    );
    assert_eq!(
        state.checkpoint.discard(&local, selector, fixture.now),
        conflict
    );

    state
        .checkpoint
        .fixture_statement("PRAGMA foreign_keys=OFF; PRAGMA recursive_triggers=OFF;");
    for (statement, reason) in [
        (
            "DELETE FROM peer_provisioning_slot WHERE class=0",
            "slot is bound",
        ),
        (
            "UPDATE peer_provisioning_slot SET generation=9 WHERE class=0",
            "slot is bound",
        ),
        (
            "INSERT OR REPLACE INTO peer_provisioning_slot(class) VALUES(0)",
            "slot is bound",
        ),
        (
            "UPDATE OR REPLACE peer_provisioning_slot SET class=0 WHERE class=1",
            "slot is bound",
        ),
        (
            "UPDATE OR REPLACE peer_provisioning_slot SET server=?1 WHERE class=1",
            "slot is bound",
        ),
        (
            "DELETE FROM peer_consent_invite WHERE server=?1",
            "invite is bound",
        ),
        (
            "INSERT OR REPLACE INTO peer_consent_invite SELECT * FROM peer_consent_invite WHERE server=?1",
            "invite is bound",
        ),
        (
            "UPDATE OR REPLACE peer_consent_invite SET server=?1 WHERE server<>?1",
            "invite is bound",
        ),
        ("DELETE FROM peer_provisional_stage_v1", "stage is bound"),
        (
            "INSERT OR REPLACE INTO peer_provisional_stage_v1 SELECT * FROM peer_provisional_stage_v1",
            "stage requires exact new claim",
        ),
        (
            "UPDATE peer_provisional_stage_v1 SET state=2",
            "stage state is monotonic",
        ),
    ] {
        let identity = selector.server.to_bytes();
        let argument = statement.contains("?1").then_some(&identity);
        let error = state
            .checkpoint
            .fixture_try_statement(statement, argument)
            .unwrap_err();
        assert!(
            matches!(
                error, rusqlite::Error::SqliteFailure(code, Some(message))
                    if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER && message == reason
            ),
            "{statement}"
        );
    }
    assert_eq!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::Bound
    );
    state.checkpoint.fixture_statement(
        "DELETE FROM peer_consent_invite
         WHERE server=(SELECT server FROM peer_provisioning_slot WHERE class=1);",
    );
    let error = state
        .checkpoint
        .fixture_try_statement(
            "INSERT INTO peer_provisional_stage_v1
            (class, generation, server, offer_id, revision, origin, state)
         SELECT class, generation, server, offer_id, revision, origin, 0
         FROM peer_provisioning_slot WHERE class=1",
            None,
        )
        .unwrap_err();
    assert!(matches!(
        error, rusqlite::Error::SqliteFailure(code, Some(message))
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER
                && message == "stage requires exact new claim"
    ));
}

#[tokio::test]
#[ignore = "opt-in SQLCipher/MDK native preparation and verified restart timing"]
async fn peer_stage_local_latency() {
    let mut prepare = Vec::new();
    let mut restart = Vec::new();
    for _ in 0..20 {
        let fixture = Fixture::new();
        let mut state = fixture.start(Policy::default());
        let (selector, reservation) = fixture.reserve(&mut state);
        let start = std::time::Instant::now();
        state
            .prepare_provisional(&reservation, |_| Ok(()))
            .await
            .unwrap();
        prepare.push(start.elapsed().as_micros());
        drop(state);
        let start = std::time::Instant::now();
        let mut state = fixture.start(Policy::default());
        let reservation = state
            .checkpoint
            .recover_provisioning(&state.source, selector, fixture.now)
            .unwrap();
        state
            .prepare_provisional(&reservation, |_| Ok(()))
            .await
            .unwrap();
        restart.push(start.elapsed().as_micros());
    }
    for (operation, mut values) in [("prepare", prepare), ("restart", restart)] {
        values.sort_unstable();
        println!(
            "peer_native_stage {operation} n={} p50_us={} p95_us={}",
            values.len(),
            values[9],
            values[18]
        );
    }
}

#[tokio::test]
async fn peer_stage_unsigned_native_commit_finalizes_without_generation() {
    let fixture = Fixture::new();
    let mut state = fixture.start(Policy::default());
    let (selector, reservation) = fixture.reserve(&mut state);
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::NativeSigning))
            .await,
        Err(Error::Native(StageError::Invalid))
    );
    let State::NativeStarted(witness) = checkpoint_state(&mut state, &reservation, fixture.now)
    else {
        panic!("expected durable native ownership after the signer interruption");
    };
    let binding = reservation.stage_binding();
    let key = SqlCipherKey::raw(*state.paths.provisional_key(binding));
    let label = state.paths.provisional_label(binding);
    let file = state.paths.reopen_provisional(binding, witness).unwrap();
    let peer = OpenStage::open_provisional(
        file.path(),
        &key,
        state.signer.clone(),
        ORIGIN,
        label,
        false,
    )
    .unwrap();
    assert!(
        !peer
            .provisional_is_signed(Duration::from_secs(fixture.now))
            .unwrap()
    );
    assert_eq!(
        peer.read_provisional(Duration::from_secs(fixture.now))
            .err(),
        Some(StageError::Invalid)
    );
    drop(peer);
    drop(state);

    let mut state = fixture.start(Policy::default());
    let reservation = state
        .checkpoint
        .recover_provisioning(&state.source, selector, fixture.now)
        .unwrap();
    state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    let bytes = fs::read(fixture.stage()).unwrap();
    assert!(matches!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::Prepared(..)
    ));
    state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(bytes, fs::read(fixture.stage()).unwrap());
}

#[tokio::test]
async fn peer_stage_rechecks_offer_expiry_after_native_signing() {
    let fixture = Fixture::new();
    let mut state = fixture.start(Policy::default());
    let expires = fixture.now + 20;
    let selector = fixture.admit(&mut state, &fixture.server, expires - 86_400);
    let reservation = state
        .checkpoint
        .reserve_provisioning(&state.source, selector, fixture.now)
        .unwrap();
    assert_eq!(
        state
            .prepare_provisional(&reservation, |step| {
                if step == Cut::NativePrepared {
                    fixture.wall.store(expires, Ordering::Relaxed);
                }
                Ok(())
            })
            .await,
        Err(Error::Checkpoint(consent_checkpoint::Error::Expired))
    );
    let native = fs::read(fixture.stage()).unwrap();
    fixture.wall.store(fixture.now, Ordering::Relaxed);
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::ClockRegressed
        )))
    );
    assert_eq!(native, fs::read(fixture.stage()).unwrap());
}

#[tokio::test]
async fn peer_stage_ownership_and_policy_fail_before_storage() {
    let fixture = Fixture::new();
    let mut state = fixture.start(Policy::default());
    let (selector, reservation) = fixture.reserve(&mut state);
    let initial = fixture.stage();
    fs::write(&initial, b"private fixture marker").unwrap();
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::StageIdentityMismatch)
    );
    assert_eq!(fs::read(&initial).unwrap(), b"private fixture marker");
    assert!(
        !state
            .checkpoint
            .has_stage(&state.source, &reservation, fixture.now)
            .unwrap()
    );
    fs::remove_file(&initial).unwrap();
    drop(state);

    let mut state = fixture.start(Policy::default());
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::Checkpoint(consent_checkpoint::Error::LocalOwner))
    );
    let reservation = state
        .checkpoint
        .recover_provisioning(&state.source, selector, fixture.now)
        .unwrap();
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::Bound))
            .await,
        Err(FAULT)
    );
    drop(state);

    let mut state = fixture.start(Policy {
        blocked: [selector.server].into(),
        ..Policy::default()
    });
    assert_eq!(
        state
            .checkpoint
            .recover_provisioning(&state.source, selector, fixture.now)
            .err(),
        Some(consent_checkpoint::Error::Policy)
    );
    assert!(!initial.exists());
}

#[tokio::test]
async fn peer_stage_initial_and_confirmation_commits_recover_after_sql_failure() {
    let fixture = Fixture::new();
    let mut state = fixture.start(Policy::default());
    let (_, reservation) = fixture.reserve(&mut state);
    state.checkpoint.fixture_statement(
        "CREATE TEMP TRIGGER fail_insert_stage BEFORE INSERT ON peer_provisional_stage_v1
         BEGIN SELECT RAISE(ABORT, 'fixture'); END;",
    );
    assert!(
        state
            .prepare_provisional(&reservation, |_| Ok(()))
            .await
            .is_err()
    );
    assert!(
        !state
            .checkpoint
            .has_stage(&state.source, &reservation, fixture.now)
            .unwrap()
    );
    assert!(!fixture.stage().exists());
    state.checkpoint.fixture_statement(
        "DROP TRIGGER fail_insert_stage;
         CREATE TEMP TRIGGER fail_prepare_stage BEFORE UPDATE OF package_id
         ON peer_provisional_stage_v1 BEGIN SELECT RAISE(ABORT, 'fixture'); END;",
    );
    assert!(
        state
            .prepare_provisional(&reservation, |_| Ok(()))
            .await
            .is_err()
    );
    assert!(matches!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::NativeStarted(_)
    ));
    let original = fs::read(fixture.stage()).unwrap();
    state
        .checkpoint
        .fixture_statement("DROP TRIGGER fail_prepare_stage;");
    state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    assert_eq!(original, fs::read(fixture.stage()).unwrap());
}

#[tokio::test]
async fn peer_stage_recovery_fences_empty_lifecycle_and_orphan() {
    let slot_only = Fixture::new();
    let orphan = Fixture::new();
    for (fixture, fault) in [
        (&slot_only, Cut::NativeStarted),
        (&orphan, Cut::NativePrepared),
    ] {
        let mut state = fixture.start(Policy::default());
        let (selector, reservation) = fixture.reserve(&mut state);
        assert_eq!(
            state.prepare_provisional(&reservation, cut_at(fault)).await,
            Err(FAULT),
        );
        let State::NativeStarted(witness) = checkpoint_state(&mut state, &reservation, fixture.now)
        else {
            panic!("expected durable native ownership");
        };
        let binding = reservation.stage_binding();
        let mut wrong_device = witness;
        wrong_device.device[0] ^= u8::MAX;
        assert_eq!(
            state.paths.reopen_provisional(binding, wrong_device).err(),
            Some(Error::StageIdentityMismatch),
        );
        let file = state.paths.reopen_provisional(binding, witness).unwrap();
        let label = state.paths.provisional_label(binding);
        let key = SqlCipherKey::raw(*state.paths.provisional_key(binding));
        let native = OpenStage::open_provisional(
            file.path(),
            &key,
            state.signer.clone(),
            ORIGIN,
            label.clone(),
            fault == Cut::NativeStarted,
        )
        .unwrap();
        // Force a fixture-only impossible state: the MDK pending row and native
        // material are written atomically. Neither half is usable by itself.
        native.fixture_replace_with_slot_only().unwrap();
        drop(native);
        let wrong_key = SqlCipherKey::raw(*state.paths.cursor_key());
        assert_eq!(
            OpenStage::open_provisional(
                file.path(),
                &wrong_key,
                state.signer.clone(),
                ORIGIN,
                label,
                false,
            )
            .err(),
            Some(StageError::Storage),
        );
        let before = file.digest().unwrap();
        drop(state);
        let mut state = fixture.start(Policy::default());
        let reservation = state
            .checkpoint
            .recover_provisioning(&state.source, selector, fixture.now)
            .unwrap();
        assert_eq!(
            state.prepare_provisional(&reservation, |_| Ok(())).await,
            Err(Error::Native(StageError::Invalid)),
        );
        assert_eq!(file.digest().unwrap(), before);
        assert_eq!(
            checkpoint_state(&mut state, &reservation, fixture.now),
            State::NativeStarted(witness),
        );
    }
}

#[tokio::test]
async fn peer_stage_quarantine_stops_native_allocation() {
    let fixture = Fixture::new();
    let mut state = fixture.start(Policy::default());
    let (_, reservation) = fixture.reserve(&mut state);
    assert_eq!(
        state
            .prepare_provisional(&reservation, cut_at(Cut::Bound))
            .await,
        Err(FAULT),
    );
    state
        .checkpoint
        .fixture_statement("UPDATE peer_cursor_origin SET quarantine_until=1");
    assert_eq!(
        state.prepare_provisional(&reservation, |_| Ok(())).await,
        Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::RelayContract,
        ))),
    );
    assert!(!fixture.stage().exists());
    state
        .checkpoint
        .fixture_statement("UPDATE peer_cursor_origin SET quarantine_until=NULL");
    assert_eq!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        State::Bound,
    );
}

#[test]
#[ignore = "isolated default SQLCipher VFS subprocess"]
fn peer_quota_dispatch_native_child() {
    let Ok(limit) = std::env::var("MARMISH_DISPATCH_NATIVE_LIMIT") else {
        return;
    };
    crate::peer_quota_vfs::dispatch::mount_for_proof().expect("dispatch_mount");
    if matches!(limit.as_str(), "crash" | "restart") {
        native_dispatch_restart(&limit);
        return;
    }
    let limit = limit.parse::<u64>().expect("dispatch_limit");
    let fixture = Fixture::new();
    let quota = ProfileSlots::reserve(&fixture.lease, (128 * 1024 * 1024) - limit)
        .expect("dispatch_reserve");
    let mut state = fixture.start(Policy::default());
    let (_, reservation) = fixture.reserve(&mut state);
    let first = crate::peer_quota_vfs::dispatch::resolve_once(
        state.prepare_provisional_with_quota(&reservation, |_| Ok(()), Some(&quota)),
    )
    .unwrap();
    if limit < 64 * 1024 {
        assert!(matches!(first, Err(Error::Native(StageError::Storage))));
        assert!(matches!(
            checkpoint_state(&mut state, &reservation, fixture.now),
            State::NativeStarted(_)
        ));
    } else {
        first.expect("dispatch_native_prepare");
        let before = fs::read(fixture.stage()).expect("dispatch_read_stage");
        crate::peer_quota_vfs::dispatch::resolve_once(state.prepare_provisional_with_quota(
            &reservation,
            |_| Ok(()),
            Some(&quota),
        ))
        .unwrap()
        .expect("dispatch_native_reopen");
        assert_eq!(
            before,
            fs::read(fixture.stage()).expect("dispatch_read_stage")
        );
        assert!(before.len() as u64 <= limit);
        assert!(!fixture.stage().with_extension("db-journal").exists());
        let State::Prepared(witness, _) = checkpoint_state(&mut state, &reservation, fixture.now)
        else {
            panic!("dispatch_expected_prepared");
        };
        let binding = reservation.stage_binding();
        let file = state.paths.reopen_provisional(binding, witness).unwrap();
        let exact = scoped_binding(file.path(), binding.coordinates(), witness).unwrap();
        let wrong = SqlCipherKey::raw([0xc9; 32]);
        assert!(matches!(
            quota.native_turn(
                exact,
                NativeTurn {
                    key: &wrong,
                    signer: state.signer.clone(),
                    origin: &state.origin_url,
                    label: state.paths.provisional_label(binding),
                    sample: Duration::from_secs(fixture.now),
                    operation: NativeOperation::Reopen { prepared: true },
                }
            ),
            Ok(Err(StageError::Storage))
        ));
        assert_eq!(before, fs::read(fixture.stage()).unwrap());
    }
    println!("dispatch-proof native {}", u8::from(limit >= 64 * 1024));
}

fn native_dispatch_restart(mode: &str) {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let path =
        PathBuf::from(std::env::var_os("MARMISH_DISPATCH_RESTART_ROOT").expect("dispatch_root"));
    let crash = mode == "crash";
    let fixture = Fixture::at(&path, None, crash);
    let quota = ProfileSlots::reserve(&fixture.lease, 124 * 1024 * 1024).expect("dispatch_reserve");
    let mut state = fixture.start(Policy::default());
    let selector_file = path.join("fixture.selector");
    if crash {
        let quota = quota.fixture_stop_in_synced_transaction();
        let (selector, reservation) = fixture.reserve(&mut state);
        let mut output = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(selector_file)
            .expect("fixture_selector");
        output.write_all(&selector.server.to_bytes()).unwrap();
        output.write_all(&selector.id).unwrap();
        output.write_all(&selector.revision).unwrap();
        output.sync_all().unwrap();
        let _ = crate::peer_quota_vfs::dispatch::resolve_once(
            state.prepare_provisional_with_quota(&reservation, |_| Ok(()), Some(&quota)),
        );
        panic!("native_crash_cut_not_reached");
    }
    let raw = fs::read(selector_file).expect("fixture_selector");
    let selector = Selector {
        server: nostr::key::PublicKey::from_slice(&raw[..32]).unwrap(),
        id: raw[32..48].try_into().unwrap(),
        revision: raw[48..].try_into().unwrap(),
    };
    let reservation = state
        .checkpoint
        .recover_provisioning(&state.source, selector, fixture.now)
        .expect("dispatch_recover_reservation");
    let durable = checkpoint_state(&mut state, &reservation, fixture.now);
    assert!(matches!(durable, State::NativeStarted(_)));
    let main = fixture.stage();
    let journal = main.with_extension("db-journal");
    let before = [fs::read(&main).unwrap(), fs::read(&journal).unwrap()];
    assert!(before[1].len() > 512);
    assert!(before[0].len() + before[1].len() <= 4 * 1024 * 1024);
    assert!(rusqlite::Connection::open(&main).is_err());
    let result = crate::peer_quota_vfs::dispatch::resolve_once(
        state.prepare_provisional_with_quota(&reservation, |_| Ok(()), Some(&quota)),
    )
    .unwrap();
    assert_eq!(result, Err(Error::StageIdentityMismatch));
    assert_eq!(
        checkpoint_state(&mut state, &reservation, fixture.now),
        durable
    );
    assert_eq!(
        [fs::read(&main).unwrap(), fs::read(&journal).unwrap()],
        before
    );
    let other = rusqlite::Connection::open(path.join("ordinary.db")).expect("ordinary_after_crash");
    other
        .execute_batch("CREATE TABLE usable(value); INSERT INTO usable VALUES(1)")
        .unwrap();
    println!("dispatch-proof restart");
}
