use super::*;

use crate::command_config::Role;
use crate::mdk_adapter::{
    peer_staged_proof_tests::peer_enrollment_welcome, terminal_relay_fixture::Relay,
};
use cgka_traits::app_components::NostrRoutingV1;
use consent_checkpoint::wire::{Codec, Delivery, MAX_OUTER_BYTES};
use prepared_consent::{History, PreparedConsentOwner, Received, VerifiedDelivery};
use sha2::Digest;
use std::{fs::Permissions, os::unix::fs::PermissionsExt, sync::atomic::AtomicU64};

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).unwrap();
    bytes
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
#[ignore = "explicit bounded loopback TLS and SQLCipher qualification"]
async fn peer_private_wss_offer_consent_delivery() {
    tokio::time::timeout(Duration::from_secs(120), enroll())
        .await
        .expect("bounded native Peer exchange");
}

async fn enroll() {
    let (private, lease, material) = tests::client();
    let founder_private = tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let signer = material.signer();
    let founder = Arc::new(Keys::generate());
    let mut relay = Relay::start_peer([signer.public_key(), founder.public_key()]).await;
    let origin = relay.url.to_string();
    let recipient_origin =
        QualifiedOrigin::for_shared_peer_fixture(&relay, private.path()).unwrap();
    let founder_origin =
        QualifiedOrigin::for_shared_peer_fixture(&relay, founder_private.path()).unwrap();
    let route = &recipient_origin.route;
    let wall = Wall::System.sample().unwrap().as_secs();
    let fixed_wall = Arc::new(AtomicU64::new(wall));
    let codec = Codec::new(MAX_OUTER_BYTES).unwrap();
    InvitationLane::initialize(&lease, &material).unwrap();
    drop(
        InvitationState::open(
            &lease,
            &material,
            Policy::default(),
            route,
            Wall::Fixture(fixed_wall.clone()),
            Checkpoint::attach_for_provisional_proof,
        )
        .expect("initialize validated provisional checkpoint"),
    );
    let open_local = || {
        PreparedConsentOwner::open(
            &lease,
            &material,
            Role::Client,
            Policy::default(),
            route,
            Wall::Fixture(fixed_wall.clone()),
        )
        .unwrap()
    };
    let mut local = open_local();
    let open_client = || {
        Owner::open(
            &recipient_origin.connector,
            &origin,
            signer.public_key(),
            &signer,
            deadline(),
        )
    };
    let mut client = open_client().await.unwrap();
    let mut server = Owner::open(
        &founder_origin.connector,
        &origin,
        founder.public_key(),
        &founder,
        deadline(),
    )
    .await
    .unwrap();
    let routing = NostrRoutingV1::new(random_bytes(), vec![origin.clone()]).unwrap();
    let offer = codec
        .prepare_offer(
            &founder,
            signer.public_key(),
            random_bytes(),
            &routing,
            wall,
        )
        .unwrap();
    assert!(
        server
            .publish(&offer, offer.flight.flight(), deadline())
            .await
            .unwrap()
    );
    let receipt = client
        .wait_live(deadline())
        .await
        .unwrap()
        .expect("addressed carrier");
    let authenticated = codec
        .open_offer(
            &signer,
            receipt.arrival().body.get().as_bytes(),
            Some(founder.public_key()),
            std::slice::from_ref(&origin),
            wall,
        )
        .unwrap();
    let selector = Selector::from(&authenticated);
    assert!(selector == Selector::from(&offer.flight));
    let Received::Admitted(offer_disposition) = local.receive_wss(&client, &receipt).unwrap()
    else {
        panic!("expected Offer disposition")
    };
    assert!(matches!(offer_disposition.status, Status::Accepted(_)));
    let reservation = local
        .state
        .checkpoint
        .reserve_provisioning(&local.state.source, selector, wall)
        .unwrap();
    local
        .state
        .prepare_provisional(&reservation, |_| Ok(()))
        .await
        .unwrap();
    let transcript = local
        .queue_prepared_consent(&reservation, |_| Ok(()))
        .unwrap();
    drop(local);
    let mut local = open_local();
    let queued = local
        .state
        .checkpoint
        .recover_queued_consent(&local.state.source, selector, wall)
        .unwrap()
        .unwrap();
    let exact_consent: [u8; 32] = Sha256::digest(queued.frame()).into();
    let (binding, witness) = queued.stage();
    let stage_image = || {
        Paths::new(&lease, &material)
            .unwrap()
            .reopen_provisional(binding, witness)
            .unwrap()
            .digest()
            .unwrap()
    };
    let original_image = stage_image();
    assert!(
        local
            .publish_consent_wss(&mut client, selector, deadline())
            .await
            .unwrap()
    );
    assert!(
        !local
            .publish_consent_wss(&mut client, selector, deadline())
            .await
            .unwrap()
    );
    let receipt = server
        .wait_live(deadline())
        .await
        .unwrap()
        .expect("addressed carrier");
    let consent = codec
        .open_consent(
            &founder,
            receipt.arrival().body.get().as_bytes(),
            &offer.flight,
            wall,
        )
        .unwrap();
    assert!(consent.flight().revision.to_bytes() == transcript.revision);
    let welcome = peer_enrollment_welcome(
        founder_private.path(),
        founder.clone(),
        consent.flight().author,
        consent.offer().routing(),
        consent.offer().room_id(),
        consent.key_package().clone(),
    )
    .await;
    let delivery = codec
        .prepare_delivery(
            &founder,
            &consent,
            &welcome,
            wall,
            consent.flight().expires_at - 1,
        )
        .unwrap();
    assert!(
        server
            .publish(&delivery, delivery.flight.flight(), deadline())
            .await
            .unwrap()
    );
    let receipt = client
        .wait_live(deadline())
        .await
        .unwrap()
        .expect("addressed carrier");
    let Received::Delivery(verified) = local.receive_wss(&client, &receipt).unwrap() else {
        panic!("expected typed live Delivery")
    };
    assert_delivery(&verified, &delivery.flight);
    let exact_received = verified.frame().to_vec();
    client.stop(deadline()).await.unwrap();
    let mut client = open_client().await.unwrap();
    assert!(matches!(
        local.receive_wss(&client, &receipt),
        Err(Error::Binding)
    ));
    let current_cursor = |owner: &mut PreparedConsentOwner<'_>| {
        owner
            .state
            .checkpoint
            .cursor
            .scan(owner.state.origin, ScanStart::Recovery, wall)
            .unwrap()
            .unwrap()
    };
    let before = current_cursor(&mut local);
    for _ in 0..2 {
        let History::Held(verified) = local.recover_wss(&mut client, deadline()).await.unwrap()
        else {
            panic!("verified Delivery holds history")
        };
        assert_delivery(&verified, &delivery.flight);
        assert!(verified.frame() == exact_received);
        assert!(current_cursor(&mut local) == before);
    }
    assert!(
        stage_image() == original_image,
        "exact recipient stage image unchanged"
    );
    let writes = relay.peer_transcript();
    assert_eq!(writes.len(), 3);
    for (entry, flight) in writes.iter().zip([
        offer.flight.flight(),
        consent.flight(),
        delivery.flight.flight(),
    ]) {
        assert!(entry.sender == flight.author);
        assert!(entry.recipient == flight.recipient);
        assert!(entry.event == flight.carrier_id);
    }
    assert!(
        writes[1].bytes_sha256 == exact_consent,
        "exact durable Consent bytes reached relay"
    );
    let other_consent = codec
        .prepare_consent(
            &signer,
            &authenticated,
            consent.package_bytes(),
            wall,
            consent.flight().expires_at - 1,
        )
        .unwrap();
    assert!(other_consent.flight.flight().revision != consent.flight().revision);
    let mismatched = codec
        .prepare_delivery(
            &founder,
            &other_consent.flight,
            &welcome,
            wall,
            consent.flight().expires_at - 2,
        )
        .unwrap();
    assert!(
        server
            .publish(&mismatched, mismatched.flight.flight(), deadline())
            .await
            .unwrap()
    );
    let receipt = client
        .wait_live(deadline())
        .await
        .unwrap()
        .expect("signed mismatched Delivery");
    assert!(matches!(
        local.receive_wss(&client, &receipt),
        Err(Error::Checkpoint(consent_checkpoint::Error::Package))
    ));
    assert!(stage_image() == original_image && current_cursor(&mut local) == before);
    // Reproduce a relay page contract violation through the existing cursor
    // API. An already validated, still-current live arrival must now be fenced.
    let repeated = record(receipt.arrival());
    assert_eq!(
        local.state.checkpoint.cursor.commit_page(
            &before,
            &Page {
                origin: local.state.origin,
                since: before.lower,
                until: before.until,
                records: &[repeated, repeated],
            },
            &[],
            wall,
        ),
        Err(store::Error::RelayContract)
    );
    assert!(matches!(
        local.receive_wss(&client, &receipt),
        Err(Error::Cursor(store::Error::RelayContract))
    ));
    client.stop(deadline()).await.unwrap();
    server.stop(deadline()).await.unwrap();
    relay.stop().await;
}

fn assert_delivery(record: &VerifiedDelivery, founder: &Delivery) {
    assert!(!record.frame().is_empty());
    let verified = record.delivery();
    assert!(verified.flight().carrier_id == founder.flight().carrier_id);
    assert!(verified.consent_revision() == founder.consent_revision());
    assert!(verified.binding() == founder.binding());
    assert!(verified.welcome_bytes() == founder.welcome_bytes());
    assert!(verified.welcome_id() == founder.welcome_id());
    assert!(verified.welcome_digest() == founder.welcome_digest());
    assert!(verified.offer().routing() == founder.offer().routing());
}
