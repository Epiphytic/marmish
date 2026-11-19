use super::super::provisional::tests::Fixture;
use super::*;
use crate::mdk_adapter::peer_staged_proof_tests::peer_enrollment_welcome;
use cgka_traits::app_components::NostrRoutingV1;
use consent_checkpoint::wire::{Codec, MAX_OUTER_BYTES};
use nostr::event::{Event, EventBuilder, FinalizeEvent, Kind, Tag};
use sha2::Digest as _;

const ORIGIN: &str = "wss://stage-fixture.example/";

#[tokio::test]
#[ignore = "opt-in isolated public native join and guarded SQLCipher readback"]
async fn exact_joined_snapshot_proof_requires_closed_image_and_receipt() {
    tokio::time::timeout(Duration::from_secs(120), qualify_joined_snapshot(false))
        .await
        .expect("bounded native joined-image proof");
}

#[cfg(all(
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
#[tokio::test]
#[ignore = "opt-in offline near-cap SQLCipher growth with real joined native stage"]
async fn exact_joined_snapshot_grows_to_near_cap() {
    tokio::time::timeout(Duration::from_secs(120), qualify_joined_snapshot(true))
        .await
        .expect("bounded native near-cap proof");
}

async fn qualify_joined_snapshot(grow: bool) {
    let fixture = Fixture::new();
    drop(fixture.start(Policy::default()));
    let mut owner = PreparedConsentOwner::open(
        &fixture.lease,
        &fixture.identity,
        Role::Both,
        Policy::default(),
        &Route::configured(ORIGIN).unwrap(),
        Wall::Fixture(fixture.wall.clone()),
    )
    .unwrap();
    let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
    let recipient = fixture.identity.signer();
    let routing = NostrRoutingV1::new([61; 32], vec![ORIGIN.into()]).unwrap();
    let offer = codec
        .prepare_offer(
            &fixture.server,
            recipient.public_key(),
            [63; 16],
            &routing,
            fixture.now,
        )
        .unwrap();
    let event: Event = serde_json::from_slice(offer.frame()).unwrap();
    let incoming = RawRecord {
        id: event.id.to_bytes(),
        stamp: event.created_at.as_secs(),
        bytes: offer.frame(),
    };
    let checkpoint = &mut owner.state.checkpoint;
    let source = &owner.state.source;
    assert!(matches!(
        checkpoint
            .admit(source, incoming, fixture.now)
            .unwrap()
            .status,
        Status::Accepted(_)
    ));
    let selector = Selector::from(&offer.flight);
    let reservation = checkpoint
        .reserve_provisioning(source, selector, fixture.now)
        .unwrap();
    owner
        .state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    owner
        .queue_prepared_consent(&reservation, |_| Ok(()))
        .unwrap();
    let queued = owner
        .state
        .checkpoint
        .recover_queued_consent(&owner.state.source, selector, fixture.now)
        .unwrap()
        .unwrap();
    let stage = queued.stage();
    let consent = codec
        .open_consent(&fixture.server, queued.frame(), &offer.flight, fixture.now)
        .unwrap();
    let sample = owner.sample().unwrap();
    let attest = owner.attest(stage, sample).unwrap();
    let package_digest: [u8; 32] = Sha256::digest(&attest.package.bytes).into();
    let founder_private = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let welcome = peer_enrollment_welcome(
        founder_private.path(),
        Arc::new(fixture.server.clone()),
        recipient.public_key(),
        consent.offer().routing(),
        consent.offer().room_id(),
        consent.key_package().clone(),
    )
    .await;
    let flight = codec
        .prepare_delivery(
            &fixture.server,
            &consent,
            &welcome,
            fixture.now,
            consent.flight().expires_at - 1,
        )
        .unwrap();
    let delivery = codec
        .open_delivery(&recipient, flight.frame(), &consent, fixture.now)
        .unwrap();
    let wrong_welcome = EventBuilder::new(Kind::GiftWrap, "other signed fixture Welcome")
        .tag(Tag::public_key(fixture.identity.public_key()))
        .finalize(&Keys::generate())
        .unwrap();
    let wrong_flight = codec
        .prepare_delivery(
            &fixture.server,
            &consent,
            &serde_json::to_vec(&wrong_welcome).unwrap(),
            fixture.now,
            consent.flight().expires_at - 1,
        )
        .unwrap();
    let wrong_delivery = codec
        .open_delivery(
            &fixture.identity.signer(),
            wrong_flight.frame(),
            &consent,
            fixture.now,
        )
        .unwrap();
    let alternate_package =
        create_alternate_package(&owner, stage.0, founder_private.path(), sample).await;
    let alternate_consent = codec
        .prepare_consent(
            &recipient,
            consent.offer(),
            &alternate_package.bytes,
            fixture.now,
            consent.flight().expires_at,
        )
        .unwrap();
    let alternate_flight = codec
        .prepare_delivery(
            &fixture.server,
            &alternate_consent.flight,
            &welcome,
            fixture.now,
            consent.flight().expires_at - 1,
        )
        .unwrap();
    let alternate_delivery = codec
        .open_delivery(
            &recipient,
            alternate_flight.frame(),
            &alternate_consent.flight,
            fixture.now,
        )
        .unwrap();
    let alternate_digest: [u8; 32] = Sha256::digest(&alternate_package.bytes).into();
    assert_eq!(delivery.binding().slot, alternate_delivery.binding().slot);
    assert_ne!(
        delivery.binding().reference,
        alternate_delivery.binding().reference
    );
    verify_native_stage(
        &owner,
        stage,
        &delivery,
        &wrong_delivery,
        (&alternate_delivery, alternate_digest),
        package_digest,
        sample,
    )
    .await;
    #[cfg(all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    if grow {
        let exact = owner
            .inspect_joined(stage, &delivery, package_digest)
            .unwrap();
        super::super::near_cap_fixture::verify_joined_fixture(&owner.state.paths, stage);
        let enlarged = owner
            .inspect_joined(stage, &delivery, package_digest)
            .unwrap();
        assert_eq!(enlarged.coordinates(), exact.coordinates());
        assert_eq!(enlarged.carrier(), exact.carrier());
        assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());
        drop(owner);
        super::super::near_cap_fixture::verify_bound_refusal(fixture.lease);
    }
    #[cfg(not(all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )))]
    assert!(!grow);
}

async fn create_alternate_package(
    owner: &PreparedConsentOwner<'_>,
    binding: Binding,
    private: &Path,
    sample: Duration,
) -> NativePackage {
    let path = private.join("alternate-package.db");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    OpenStage::open_provisional(
        &path,
        &SqlCipherKey::raw(*owner.state.paths.provisional_key(binding)),
        owner.state.signer.clone(),
        ORIGIN,
        owner.state.paths.provisional_label(binding),
        true,
    )
    .unwrap()
    .generate_provisional(sample, false)
    .await
    .unwrap()
    .event
}

async fn verify_native_stage(
    owner: &PreparedConsentOwner<'_>,
    stage: (Binding, FileWitness),
    delivery: &Delivery,
    wrong_delivery: &Delivery,
    (alternate_delivery, alternate_digest): (&Delivery, [u8; 32]),
    package_digest: [u8; 32],
    sample: Duration,
) {
    let original = owner
        .state
        .paths
        .reopen_provisional(stage.0, stage.1)
        .unwrap();
    let prepared = original.digest().unwrap();
    assert!(matches!(
        owner.inspect_joined(stage, delivery, package_digest),
        Err(Error::Native(StageError::Unavailable))
    ));
    assert_eq!(original.digest().unwrap(), prepared);
    assert!(JoinWitness::new(delivery, "wss://wrong-fixture.example/", package_digest).is_err());
    assert!(matches!(
        owner
            .join_custody_stage(stage, delivery, [0; 32], sample)
            .await,
        Err(Error::Native(StageError::Invalid))
    ));
    assert_eq!(original.digest().unwrap(), prepared);
    assert!(matches!(
        owner
            .join_custody_stage(stage, alternate_delivery, alternate_digest, sample)
            .await,
        Err(Error::Native(StageError::Invalid))
    ));
    assert_eq!(original.digest().unwrap(), prepared);
    owner
        .join_custody_stage(stage, delivery, package_digest, sample)
        .await
        .unwrap();
    let joined_image = original.digest().unwrap();
    assert_ne!(joined_image, prepared);
    for replay in [delivery, wrong_delivery] {
        assert!(matches!(
            owner
                .join_custody_stage(stage, replay, package_digest, sample)
                .await,
            Err(Error::Native(StageError::Invalid))
        ));
        assert_eq!(original.digest().unwrap(), joined_image);
    }
    let joined = owner
        .inspect_joined(stage, delivery, package_digest)
        .unwrap();
    assert!(joined.stage() == stage);
    assert_eq!(joined.carrier(), delivery.flight().carrier_id.to_bytes());
    let (group, _, _, _) = joined.coordinates();
    assert_ne!(group, delivery.offer().room_id());
    let again = owner
        .inspect_joined(stage, delivery, package_digest)
        .unwrap();
    assert_eq!(again.coordinates(), joined.coordinates());
    assert_eq!(original.digest().unwrap(), joined_image);

    assert!(owner.inspect_joined(stage, delivery, [0; 32]).is_err());
    assert!(matches!(
        owner.inspect_joined(stage, wrong_delivery, package_digest),
        Err(Error::Native(StageError::Invalid))
    ));
    let sidecar = original.path().with_extension("db-journal");
    fs::write(&sidecar, b"must not be recovered by readback").unwrap();
    assert!(
        owner
            .inspect_joined(stage, delivery, package_digest)
            .is_err()
    );
    assert_eq!(
        fs::read(&sidecar).unwrap(),
        b"must not be recovered by readback"
    );
    fs::remove_file(sidecar).unwrap();
    assert_eq!(original.digest().unwrap(), joined_image);
    assert!(owner.snapshot.0.path().read_dir().unwrap().next().is_none());
}
