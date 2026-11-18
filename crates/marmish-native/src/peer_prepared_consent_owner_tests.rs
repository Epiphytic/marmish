use super::super::provisional::tests::Fixture;
use super::*;

const ORIGIN: &str = "wss://stage-fixture.example/";

fn reopen<'a>(
    fixture: &'a Fixture,
    role: Role,
    policy: Policy,
) -> Result<PreparedConsentOwner<'a>> {
    PreparedConsentOwner::open(
        &fixture.lease,
        &fixture.identity,
        role,
        policy,
        &Route::configured(ORIGIN).unwrap(),
        Wall::Fixture(fixture.wall.clone()),
    )
}

#[tokio::test]
async fn prepared_consent_mdks_open_preserves_exact_known_good_copy() {
    use consent_checkpoint::provisioning::stage_journal::State;
    let fixture = Fixture::new();
    let mut stage = fixture.start(Policy::default());
    let (selector, reservation) = fixture.reserve(&mut stage);
    stage
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    drop(stage);
    let mut owner = reopen(&fixture, Role::Both, Policy::default()).unwrap();
    let sample = owner.sample().unwrap();
    let reservation = owner
        .state
        .checkpoint
        .recover_provisioning(&owner.state.source, selector, sample.as_secs())
        .unwrap();
    let binding = reservation.stage_binding();
    let State::Prepared(witness, artifact) = owner
        .state
        .checkpoint
        .bind_stage(&owner.state.source, &reservation, sample.as_secs())
        .unwrap()
        .state()
    else {
        panic!("expected Prepared")
    };
    let before = fs::read(fixture.stage()).unwrap();
    let (artifact_read, _) = owner
        .snapshot
        .read(&owner.state, binding, witness, sample, |_| Ok(()))
        .unwrap();
    assert_eq!(artifact_read.id, artifact.id);
    assert_eq!(fs::read(fixture.stage()).unwrap(), before);
    assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());

    let sidecar = fixture.stage().with_extension("db-wal");
    fs::write(&sidecar, b"dirty sidecar").unwrap();
    assert!(
        owner
            .snapshot
            .read(&owner.state, binding, witness, sample, |_| Ok(()))
            .is_err()
    );
    fs::remove_file(sidecar).unwrap();
    assert_eq!(fs::read(fixture.stage()).unwrap(), before);
    assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());

    let mut damaged = before.clone();
    damaged[50] ^= 0x20;
    fs::write(fixture.stage(), &damaged).unwrap();
    assert!(
        owner
            .snapshot
            .read(&owner.state, binding, witness, sample, |_| Ok(()))
            .is_err()
    );
    assert_eq!(fs::read(fixture.stage()).unwrap(), damaged);
    assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());
    fs::write(fixture.stage(), &before).unwrap();

    let copy_sidecar = owner.snapshot.0.path().join("image.db-shm");
    let failed = owner
        .snapshot
        .read(&owner.state, binding, witness, sample, |_| {
            fs::write(&copy_sidecar, b"dirty fixture copy sidecar").unwrap();
            Ok(())
        });
    assert_eq!(failed.err(), Some(Error::StageIdentityMismatch));
    assert!(!owner.snapshot.0.path().join("image.db").exists());
    fs::remove_file(copy_sidecar).unwrap();
    assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());
    assert_eq!(fs::read(fixture.stage()).unwrap(), before);

    assert_wrong_key_and_migration_refused(&fixture, &owner, binding, witness, sample);
    assert_eq!(fs::read(fixture.stage()).unwrap(), before);

    let failed = owner
        .snapshot
        .read(&owner.state, binding, witness, sample, |copy| {
            fs::write(copy, b"changed").unwrap();
            Ok(())
        });
    assert_eq!(failed.err(), Some(Error::StageIdentityMismatch));
    assert_eq!(fs::read(fixture.stage()).unwrap(), before);
    assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());

    let failed = owner
        .snapshot
        .read(&owner.state, binding, witness, sample, |copy| {
            fs::remove_file(copy).unwrap();
            fs::create_dir(copy).unwrap();
            Ok(())
        });
    assert_eq!(failed.err(), Some(Error::Path));
    assert_eq!(
        owner
            .snapshot
            .read(&owner.state, binding, witness, sample, |_| Ok(()))
            .err(),
        Some(Error::Path)
    );
    assert_eq!(fs::read(fixture.stage()).unwrap(), before);
}

fn assert_wrong_key_and_migration_refused(
    fixture: &Fixture,
    owner: &PreparedConsentOwner<'_>,
    binding: Binding,
    witness: FileWitness,
    sample: Duration,
) {
    use storage_sqlite::{SqlCipherHardening, open_hardened_sqlcipher};

    let image = SnapshotArea::new().unwrap();
    let path = image.0.path().join("seed.db");
    let stage_path = fixture.stage();
    let original = fs::read(&stage_path).unwrap();
    let key = *owner.state.paths.provisional_key(binding);
    let mut wrong = key;
    wrong[0] ^= 1;
    for seed_key in [wrong, key] {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        let seed = rusqlite::Connection::open(&path).unwrap();
        open_hardened_sqlcipher(
            &seed,
            &SqlCipherKey::raw(seed_key),
            SqlCipherHardening::live_cache(),
        )
        .unwrap();
        seed.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA user_version=7")
            .unwrap();
        drop(seed);
        let initial = fs::read(&path).unwrap();
        fs::write(&stage_path, &initial).unwrap();
        assert_eq!(
            owner
                .snapshot
                .read(&owner.state, binding, witness, sample, |_| Ok(()))
                .err(),
            Some(Error::Native(StageError::Storage)),
        );
        assert_eq!(fs::read(&stage_path).unwrap(), initial);
        assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());
        if seed_key == key {
            // Control: native first-open really migrates this same encrypted seed.
            let native = OpenStage::open_provisional(
                &path,
                &SqlCipherKey::raw(key),
                owner.state.signer.clone(),
                &owner.state.origin_url,
                owner.state.paths.provisional_label(binding),
                true,
            )
            .unwrap();
            drop(native);
            assert_ne!(fs::read(&path).unwrap(), initial);
        }
        fs::remove_file(&path).unwrap();
    }
    fs::write(stage_path, original).unwrap();
}

fn reserve_near_expiry(
    fixture: &Fixture,
    owner: &mut PreparedConsentOwner<'_>,
) -> (Selector, Reservation) {
    let selector = fixture.admit(&mut owner.state, &fixture.server, fixture.now - 86_390);
    let reservation = owner
        .state
        .checkpoint
        .reserve_provisioning(&owner.state.source, selector, fixture.now)
        .unwrap();
    (selector, reservation)
}

#[tokio::test]
async fn prepared_consent_queue_restarts_with_exact_transcript_after_offer_expiry() {
    use consent_checkpoint::Mode;
    use std::sync::atomic::Ordering;

    let fixture = Fixture::new();
    drop(fixture.start(Policy::default()));
    let mut owner = reopen(
        &fixture,
        Role::Client,
        Policy {
            mode: Mode::Automatic,
            ..Policy::default()
        },
    )
    .unwrap();
    let (selector, reservation) = reserve_near_expiry(&fixture, &mut owner);
    owner
        .state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    let sample = owner.sample().unwrap();
    let candidate = owner
        .state
        .checkpoint
        .inspect_prepared_consent(&owner.state.source, &reservation, sample.as_secs())
        .unwrap();
    let mut proof = owner.attest(candidate.stage(), sample).unwrap();
    owner.state.checkpoint.fixture_assert_queue_rollback(
        &owner.state.source,
        &reservation,
        &proof,
        sample.as_secs(),
    );
    assert!(owner.recover_queued_consent(selector).unwrap().is_none());
    for cut in [Cut::BeforeSign, Cut::AfterSign, Cut::BeforeQueueCommit] {
        assert!(
            owner
                .queue_prepared_consent(&reservation, |at| {
                    if at == cut { Err(Error::Clock) } else { Ok(()) }
                })
                .is_err()
        );
        assert!(owner.recover_queued_consent(selector).unwrap().is_none());
    }
    let original = owner
        .queue_prepared_consent(&reservation, |_| Ok(()))
        .unwrap();
    assert_eq!(
        owner
            .queue_prepared_consent(&reservation, |_| panic!("recovery must never sign"))
            .unwrap(),
        original,
    );
    owner
        .state
        .checkpoint
        .fixture_assert_mutated_transcript_rejected(
            &owner.state.source,
            selector,
            &proof,
            sample.as_secs(),
        );
    let queued = owner
        .state
        .checkpoint
        .recover_queued_consent(&owner.state.source, selector, sample.as_secs())
        .unwrap()
        .unwrap();
    proof.package.id[0] ^= 1;
    assert_eq!(
        owner
            .state
            .checkpoint
            .verify_queued_consent(&queued, &proof, sample.as_secs())
            .err(),
        Some(consent_checkpoint::Error::Package),
    );
    drop(owner);

    fixture.wall.store(fixture.now + 10, Ordering::Relaxed);
    let mut resumed = reopen(&fixture, Role::Both, Policy::default()).unwrap();
    assert!(
        resumed
            .state
            .checkpoint
            .recover_provisioning(&resumed.state.source, selector, fixture.now + 10,)
            .is_err()
    );
    assert_eq!(
        resumed.recover_queued_consent(selector).unwrap(),
        Some(original)
    );
    assert!(
        resumed
            .queue_prepared_consent(&reservation, |_| Ok(()))
            .is_err()
    );
    let stale = Selector {
        id: [73; 16],
        ..selector
    };
    assert_eq!(resumed.recover_queued_consent(stale).unwrap(), None);

    assert!(reopen(&fixture, Role::Server, Policy::default()).is_err());
    for policy in [
        Policy {
            mode: Mode::Off,
            ..Policy::default()
        },
        Policy {
            blocked: [fixture.server.public_key()].into(),
            ..Policy::default()
        },
    ] {
        assert!(
            reopen(&fixture, Role::Client, policy)
                .unwrap()
                .recover_queued_consent(selector)
                .is_err()
        );
    }
}

fn sink(origin: Origin) -> FixtureSink {
    FixtureSink {
        owner: Arc::new(()),
        origin,
        ready: true,
        response: SinkResponse::Positive,
        last: None,
    }
}

#[tokio::test]
async fn prepared_consent_attempt_retries_exact_bytes_and_requires_current_positive_ack() {
    use consent_checkpoint::{
        Mode,
        wire::{Codec, MAX_OUTER_BYTES, Transcript},
    };
    use std::sync::atomic::Ordering;

    let fixture = Fixture::new();
    drop(fixture.start(Policy::default()));
    let mut owner = reopen(
        &fixture,
        Role::Both,
        Policy {
            mode: Mode::Automatic,
            ..Policy::default()
        },
    )
    .unwrap();
    let (selector, reservation) = reserve_near_expiry(&fixture, &mut owner);
    owner
        .state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    let transcript = owner
        .queue_prepared_consent(&reservation, |_| Ok(()))
        .unwrap();
    let mut destination = sink(owner.state.origin);
    destination.ready = false;
    assert!(
        owner
            .reserve_prepared_attempt(selector, &destination)
            .unwrap()
            .is_none()
    );
    let mut wrong_route = sink([182; 32]);
    assert!(
        owner
            .reserve_prepared_attempt(selector, &wrong_route)
            .unwrap()
            .is_none()
    );
    assert!(wrong_route.last.take().is_none());
    destination.ready = true;
    let reserved = owner
        .reserve_prepared_attempt(selector, &destination)
        .unwrap()
        .unwrap();
    assert_eq!(reserved.attempt.sequence(), 1);
    // Cut after durable reserve. No frame has reached any sink.
    drop(reserved);
    assert!(destination.last.is_none());
    drop(owner);

    fixture.wall.store(fixture.now + 59, Ordering::Relaxed);
    let mut owner = reopen(&fixture, Role::Both, Policy::default()).unwrap();
    assert!(
        owner
            .reserve_prepared_attempt(selector, &destination)
            .unwrap()
            .is_none()
    );
    fixture.wall.store(fixture.now + 60, Ordering::Relaxed);
    let second = owner
        .reserve_prepared_attempt(selector, &destination)
        .unwrap()
        .unwrap();
    assert_eq!(second.attempt.sequence(), 2);
    for response in [
        SinkResponse::Negative,
        SinkResponse::WrongOrigin,
        SinkResponse::WrongCarrier,
        SinkResponse::WrongSequence,
    ] {
        destination.response = response;
        let receipt = destination.publish(&second.attempt).unwrap();
        assert!(
            receipt
                .as_ref()
                .is_none_or(|receipt| !destination.owns(receipt, &second.attempt))
        );
    }
    destination.response = SinkResponse::Positive;
    let delayed_ack = owner
        .publish_prepared_attempt(second, &mut destination)
        .unwrap()
        .unwrap();
    // Validate recipient decryption and exact inner transcript and carrier.
    let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
    let bytes = destination.last.clone().unwrap();
    let authenticated = owner
        .state
        .checkpoint
        .fixture_read_offer(&owner.state.source, selector, fixture.now)
        .unwrap();
    let recipient = codec
        .open_consent(&fixture.server, &bytes, &authenticated, fixture.now + 60)
        .unwrap();
    assert_eq!(Transcript::from_consent(&recipient), transcript);
    assert_eq!(recipient.flight().author, fixture.identity.public_key());
    assert_eq!(recipient.flight().recipient, fixture.server.public_key());

    fixture.wall.store(fixture.now + 120, Ordering::Relaxed);
    let third = owner
        .reserve_prepared_attempt(selector, &destination)
        .unwrap()
        .unwrap();
    assert_eq!(third.attempt.sequence(), 3);
    assert!(
        owner
            .settle_prepared_ack(delayed_ack, &destination)
            .is_err()
    );
    let matched = owner
        .publish_prepared_attempt(third, &mut destination)
        .unwrap()
        .unwrap();
    assert_eq!(destination.last.as_deref(), Some(bytes.as_slice()));
    owner.settle_prepared_ack(matched, &destination).unwrap();
    drop(owner);

    let mut restarted = reopen(&fixture, Role::Both, Policy::default()).unwrap();
    assert_eq!(
        restarted.recover_queued_consent(selector).unwrap(),
        Some(transcript)
    );
    assert!(
        restarted
            .reserve_prepared_attempt(selector, &destination)
            .unwrap()
            .is_none()
    );
    let beyond = restarted.state.checkpoint.recover_queued_consent(
        &restarted.state.source,
        selector,
        transcript.expires,
    );
    assert_eq!(beyond.err(), Some(consent_checkpoint::Error::Expired));
}

#[tokio::test]
async fn prepared_consent_expired_without_queue_and_wrong_inode_fail_closed() {
    use std::sync::atomic::Ordering;

    let fixture = Fixture::new();
    drop(fixture.start(Policy::default()));
    let mut owner = reopen(&fixture, Role::Client, Policy::default()).unwrap();
    let (selector, reservation) = reserve_near_expiry(&fixture, &mut owner);
    owner
        .state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    let original_path = fixture.stage();
    let substituted = original_path.with_extension("original");
    fs::rename(&original_path, &substituted).unwrap();
    fs::copy(&substituted, &original_path).unwrap();
    assert!(
        owner
            .queue_prepared_consent(&reservation, |_| Ok(()))
            .is_err()
    );
    assert!(owner.recover_queued_consent(selector).unwrap().is_none());
    fs::rename(substituted, original_path).unwrap();
    fixture.wall.store(fixture.now + 10, Ordering::Relaxed);
    assert!(
        owner
            .queue_prepared_consent(&reservation, |_| Ok(()))
            .is_err()
    );
    assert!(owner.recover_queued_consent(selector).unwrap().is_none());
}

#[tokio::test]
async fn prepared_consent_both_classes_keep_exact_ownership_and_export_limits() {
    let fixture = Fixture::new();
    let policy = || Policy {
        reserved: [fixture.server.public_key()].into(),
        ..Policy::default()
    };
    drop(fixture.start(policy()));
    let mut owner = reopen(&fixture, Role::Both, policy()).unwrap();
    let unknown = Keys::generate();
    let mut queued_selectors = Vec::new();
    for server in [&fixture.server, &unknown] {
        let selector = fixture.admit(&mut owner.state, server, fixture.now);
        queued_selectors.push(selector);
        let reservation = owner
            .state
            .checkpoint
            .reserve_provisioning(&owner.state.source, selector, fixture.now)
            .unwrap();
        owner
            .state
            .prepare_provisional(&reservation, |_| Ok(()))
            .await
            .unwrap();
        owner
            .queue_prepared_consent(&reservation, |_| Ok(()))
            .unwrap();
    }
    let third = Keys::generate();
    let third_selector = fixture.admit(&mut owner.state, &third, fixture.now);
    assert!(
        owner
            .state
            .checkpoint
            .reserve_provisioning(&owner.state.source, third_selector, fixture.now,)
            .is_err()
    );
    let cursor = owner.state.paths.cursor().unwrap();
    let second =
        HardenedPeerConnection::open(cursor.path(), *owner.state.paths.cursor_key()).unwrap();
    owner.state.checkpoint.fixture_check_outbox_guards(second);

    // A changed origin capability cannot be used by a restarted owner.
    let mut restarted = reopen(&fixture, Role::Both, policy()).unwrap();
    let selector = queued_selectors[0];
    assert!(
        restarted
            .state
            .checkpoint
            .recover_queued_consent(&owner.state.source, selector, fixture.now,)
            .is_err()
    );

    restarted
        .state
        .checkpoint
        .fixture_statement("UPDATE peer_cursor_origin SET quarantine_until=wall+1000");
    assert!(matches!(
        restarted.recover_queued_consent(queued_selectors[0]),
        Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::RelayContract
        )))
    ));
}

#[tokio::test]
#[ignore = "opt-in bounded local SQLCipher and MDK timing"]
async fn prepared_consent_local_benchmark() {
    use std::time::Instant;
    const SAMPLES: usize = 20;

    let mut phases = [const { Vec::new() }; 5];
    let mut bytes = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let fixture = Fixture::new();
        drop(fixture.start(Policy::default()));
        let mut owner = reopen(&fixture, Role::Client, Policy::default()).unwrap();
        let (selector, reservation) = fixture.reserve(&mut owner.state);
        let start = Instant::now();
        owner
            .state
            .prepare_provisional(&reservation, |_| Ok(()))
            .await
            .unwrap();
        phases[0].push(start.elapsed().as_micros());
        let start = Instant::now();
        owner
            .queue_prepared_consent(&reservation, |_| Ok(()))
            .unwrap();
        phases[1].push(start.elapsed().as_micros());
        let mut destination = sink(owner.state.origin);
        let first = owner
            .reserve_prepared_attempt(selector, &destination)
            .unwrap()
            .unwrap();
        assert_eq!(first.attempt.sequence(), 1);
        let original = first.attempt.bytes().to_vec();
        drop(first);
        drop(owner);
        fixture
            .wall
            .store(fixture.now + 60, std::sync::atomic::Ordering::Relaxed);

        let start = Instant::now();
        let mut recovered = reopen(&fixture, Role::Client, Policy::default()).unwrap();
        let reserved = recovered
            .reserve_prepared_attempt(selector, &destination)
            .unwrap()
            .unwrap();
        phases[2].push(start.elapsed().as_micros());
        assert_eq!(reserved.attempt.sequence(), 2);
        assert_eq!(reserved.attempt.bytes(), original);
        let start = Instant::now();
        let sent = recovered
            .publish_prepared_attempt(reserved, &mut destination)
            .unwrap()
            .unwrap();
        phases[3].push(start.elapsed().as_micros());
        assert_eq!(destination.last.as_deref(), Some(original.as_slice()));
        bytes.push(original.len());
        let start = Instant::now();
        recovered.settle_prepared_ack(sent, &destination).unwrap();
        phases[4].push(start.elapsed().as_micros());
    }
    for (name, samples) in ["native", "queue", "reopen-retry", "local-send", "ack"]
        .into_iter()
        .zip(&mut phases)
    {
        samples.sort_unstable();
        println!(
            "prepared-consent {name} n={SAMPLES} min_us={} p50_us={} p95_us={} max_us={}",
            samples[0],
            samples[(SAMPLES - 1) / 2],
            samples[SAMPLES * 95 / 100 - 1],
            samples[SAMPLES - 1]
        );
    }
    println!(
        "prepared-consent frame bytes min={} max={}",
        bytes.iter().min().unwrap(),
        bytes.iter().max().unwrap()
    );
}
