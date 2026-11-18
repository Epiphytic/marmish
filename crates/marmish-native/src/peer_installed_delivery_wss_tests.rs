use std::{
    fs::Permissions,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::Arc,
};

use cgka_traits::app_components::NostrRoutingV1;
use nostr::event::EventId;
use sha2::Digest;

use crate::{
    mdk_adapter::peer_staged_proof_tests::peer_enrollment_welcome,
    peer_control_version::ControlVersion,
    peer_quota_vfs::dispatch::{ProfileSlots, mount_for_proof, resolve_once},
    peer_relay_trust::{ReceiverRouteAdmission, qualified_recipient_fixture},
};

use super::super::tests::{time_limit, wall};
use super::*;
use consent_checkpoint::{
    Status,
    wire::{Codec, MAX_OUTER_BYTES},
};
use socket::owner::recipient::PublicationOutcome;
use store::TrustedOutcome;

const CHILD_FLAG: &str = "MARMISH_INSTALLED_DELIVERY_AFTER_RETRY";

#[test]
#[ignore = "private TLS with native KeyPackage and an actual 60-second Consent retry"]
fn peer_installed_delivery_recovers_after_lost_ack() {
    let name = concat!(
        "peer_invitation::prepared_consent::installed_consent::delivery::tests::",
        "peer_installed_delivery_child"
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", name, "--nocapture"])
        .env(CHILD_FLAG, "1")
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let started = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if started.elapsed() >= Duration::from_secs(150) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("installed Delivery watchdog");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "child of the installed Delivery WSS proof"]
fn peer_installed_delivery_child() {
    if std::env::var_os(CHILD_FLAG).is_none() {
        return;
    }
    mount_for_proof().unwrap();
    std::thread::Builder::new()
        .name("installed-delivery".into())
        .stack_size(32 << 20)
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(Box::pin(run()));
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn run() {
    let founder = Arc::new(Keys::generate());
    let second = Keys::generate();
    let (relay, _test_home, lease, material) = qualified_recipient_fixture(
        founder.public_key(),
        second.public_key(),
        Duration::from_secs(180),
    )
    .await;
    let founder_dir = tempfile::Builder::new()
        .permissions(Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let account = material.public_key();
    let url = relay.url.as_str();
    let route = Route::configured(url).unwrap();
    let routing = NostrRoutingV1::new([3; 32], vec![url.to_owned()]).unwrap();
    Paths::new(&lease, &material).unwrap().initialize().unwrap();
    let admission = || ReceiverRouteAdmission::fixture(lease.clone(), &material).unwrap();
    let mut fixture = Fixture::open(&lease, &material, &route).unwrap();
    let mut recipient = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    let connector = relay.connector(lease.root(), true, None);
    let mut sender = Owner::open(
        &connector,
        url,
        founder.public_key(),
        &founder,
        time_limit(),
    )
    .await
    .unwrap();
    let codec = fixture.owner.state.checkpoint.custody_codec();
    let offer = codec
        .prepare_offer(&founder, account, [0x89; 16], &routing, wall())
        .unwrap();
    let selector = Selector::from(&offer.flight);
    assert!(
        sender
            .publish(&offer, offer.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let live = recipient.wait_live(time_limit()).await.unwrap().unwrap();
    let applied = fixture
        .delivery_input(&mut recipient, Input::Live(&live))
        .unwrap();
    assert_eq!(applied.status, Status::Accepted(TrustedOutcome::Applied));

    let state = &mut fixture.owner.state;
    let reservation = state
        .checkpoint
        .reserve_provisioning(&state.source, selector, wall())
        .unwrap();
    let (_, generation, ..) = reservation.stage_binding().coordinates();
    let quota = ProfileSlots::reserve(&lease, store::installed::RESERVED_BYTES).unwrap();
    resolve_once(state.prepare_provisional_with_quota(&reservation, |_| Ok(()), Some(&quota)))
        .unwrap()
        .unwrap();
    let transcript = fixture.queue(&reservation, wall()).unwrap();
    let consent_carrier = EventId::from_byte_array(transcript.carrier);
    relay.recipient_lose_ack(consent_carrier);
    assert_eq!(
        recipient
            .publish_prepared_consent(&mut fixture, selector, time_limit())
            .await,
        PublicationOutcome::PossiblyExposed(Error::Socket(Failure::Deadline))
    );
    assert!(matches!(
        recipient.wait_live(time_limit()).await,
        Err(Failure::Retired)
    ));
    let received = sender.wait_live(time_limit()).await.unwrap().unwrap();
    let consent = codec
        .open_consent(
            &founder,
            received.arrival().body.get().as_bytes(),
            &offer.flight,
            wall(),
        )
        .unwrap();
    assert_eq!(consent.flight().revision.to_bytes(), transcript.revision);
    let welcome = peer_enrollment_welcome(
        founder_dir.path(),
        founder.clone(),
        account,
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
            wall(),
            consent.flight().expires_at - 1,
        )
        .unwrap();
    assert!(
        sender
            .publish(&delivery, delivery.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let other_connector = relay.connector(lease.root(), true, None);
    let mut other = Owner::open(
        &other_connector,
        url,
        second.public_key(),
        &second,
        time_limit(),
    )
    .await
    .unwrap();

    let second_offer = codec
        .prepare_offer(&second, account, [0x90; 16], &routing, wall())
        .unwrap();
    assert!(
        other
            .publish(&second_offer, second_offer.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let v1 = Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V1).unwrap();
    let invalid = v1
        .prepare_offer(&second, account, [0x91; 16], &routing, wall())
        .unwrap();
    assert!(
        other
            .publish(&invalid, invalid.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    drop(recipient);

    let mut recipient = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    let cursor = fixture
        .scan_delivery(&mut recipient, ScanStart::Recovery)
        .unwrap()
        .unwrap();
    let page = recipient
        .page([cursor.lower, cursor.until], time_limit())
        .await
        .unwrap();
    assert_eq!(page.records().len(), 4);
    let delivery_id = delivery.flight.flight().carrier_id.to_bytes();
    let observed_delivery = page
        .records()
        .iter()
        .find(|arrival| arrival.id == delivery_id)
        .expect("published Delivery present in checked history")
        .body
        .get()
        .as_bytes()
        .to_vec();

    let started = std::time::Instant::now();
    let (statuses, next) = fixture
        .delivery_page(&mut recipient, &cursor, &page)
        .unwrap();
    let page_millis = started.elapsed().as_millis();
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == Status::Deferred)
            .count(),
        1
    );
    assert_eq!(
        statuses
            .iter()
            .filter(|status| **status == Status::Invalid)
            .count(),
        1
    );
    assert!(statuses.contains(&Status::Accepted(TrustedOutcome::Applied)));
    let next = next.expect("full mixed page");
    let page = recipient
        .page([next.lower, next.until], time_limit())
        .await
        .unwrap();
    assert!(
        fixture
            .delivery_page(&mut recipient, &next, &page)
            .unwrap()
            .1
            .is_none()
    );

    let state = &mut fixture.owner.state;
    assert!(
        state
            .checkpoint
            .read_custody(&state.source, selector, generation, wall())
            .unwrap()
            .is_none()
    );
    drop(fixture);
    drop(recipient);

    let started = std::time::Instant::now();
    let mut fixture = Fixture::open(&lease, &material, &route).unwrap();
    let queued = fixture.queued(selector, wall()).unwrap().unwrap();
    let (_, retry_at, ack) = queued.publication_state();
    let retry_at = retry_at.unwrap();
    assert!(ack.is_none());
    let retry_cold_ms = started.elapsed().as_millis();

    let mut recipient = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    while wall() < retry_at {
        tokio::time::sleep(Duration::from_millis(125)).await;
    }
    assert_eq!(
        recipient
            .publish_prepared_consent(&mut fixture, selector, time_limit())
            .await,
        PublicationOutcome::Settled
    );
    assert!(fixture.wake_failure.is_none());
    let started = std::time::Instant::now();
    assert_eq!(
        fixture.retry_delivery(&mut recipient).unwrap(),
        Some(Status::Accepted(TrustedOutcome::Held))
    );
    let held_ms = started.elapsed().as_millis();
    assert_eq!(fixture.retry_delivery(&mut recipient).unwrap(), None);

    let writes: Vec<_> = relay
        .peer_transcript()
        .into_iter()
        .filter(|write| write.event == consent_carrier)
        .map(|write| (write.bytes_sha256, write.bytes_len))
        .collect();
    assert_eq!(
        writes,
        vec![(Sha256::digest(queued.frame()).into(), queued.frame().len()); 2]
    );
    drop(fixture);
    let started = std::time::Instant::now();
    let mut fixture = Fixture::open(&lease, &material, &route).unwrap();
    let state = &mut fixture.owner.state;
    let held = state
        .checkpoint
        .read_custody(&state.source, selector, generation, wall())
        .unwrap()
        .unwrap();
    assert_eq!(held.delivery_carrier(), delivery_id);
    assert!(
        held.signed_delivery() == observed_delivery,
        "cold Held must preserve the exact recipient history bytes"
    );
    assert_eq!(
        held.state(),
        consent_checkpoint::provisioning::prepared_consent::custody::CustodyState::Held
    );
    assert!(!held.is_active());
    let stage = fixture.attest(held.stage(), wall()).unwrap();
    fixture.owner.verify_custody(&stage).unwrap();
    let held_cold_ms = started.elapsed().as_millis();

    let cursor = fixture
        .scan_delivery(&mut recipient, ScanStart::Recovery)
        .unwrap()
        .unwrap();
    let page = recipient
        .page([cursor.lower, cursor.until], time_limit())
        .await
        .unwrap();
    let statuses = fixture
        .delivery_page(&mut recipient, &cursor, &page)
        .unwrap()
        .0;
    assert!(statuses.contains(&Status::Accepted(TrustedOutcome::AlreadyApplied)));
    eprintln!(
        "installed_delivery mixed_ms={page_millis} retry_cold_ms={retry_cold_ms} ack_to_held_ms={held_ms} held_cold_ms={held_cold_ms} delivery_bytes={}",
        observed_delivery.len()
    );
}
