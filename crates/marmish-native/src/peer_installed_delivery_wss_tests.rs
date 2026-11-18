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
    mdk_adapter::{
        peer_staged_proof_tests::peer_enrollment_welcome, terminal_relay_fixture::RecipientFault,
    },
    peer_control_version::ControlVersion,
    peer_quota_vfs::dispatch::{ProfileSlots, mount_for_proof, resolve_once},
    peer_relay_trust::{ReceiverRouteAdmission, qualified_recipient_fixture},
};

use super::super::tests::{time_limit, wall};
use super::*;
use consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::Observation;
use consent_checkpoint::{
    Status,
    wire::{Codec, MAX_OUTER_BYTES},
};
use service::{ConnectionAction, Refusal, Retirement, Service, Work};
use socket::owner::recipient::PublicationOutcome;
use store::TrustedOutcome;

const CHILD_FLAG: &str = "MARMISH_INSTALLED_DELIVERY_AFTER_RETRY";

#[test]
#[ignore = "private TLS with native KeyPackage and an actual 60-second Consent retry"]
fn peer_installed_delivery_recovers_after_lost_ack() {
    run_child("normal");
}

#[test]
#[ignore = "private TLS with an actual 60-second Consent retry and a read-only wake transaction"]
fn peer_installed_delivery_recovers_when_wake_cannot_write() {
    run_child("wake-refuses");
}

#[test]
#[ignore = "private TLS with a real deferred Delivery and historical native KeyPackage evidence"]
fn peer_installed_delivery_retires_saved_at_expiry() {
    run_child("saved-expires");
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Case {
    Normal,
    WakeRefuses,
    SavedExpires,
}

fn run_child(case: &str) {
    let name = concat!(
        "peer_invitation::prepared_consent::installed_consent::delivery::tests::",
        "peer_installed_delivery_child"
    );
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", name, "--nocapture"])
        .env(CHILD_FLAG, case)
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
    let case = match std::env::var(CHILD_FLAG).as_deref() {
        Ok("normal") => Case::Normal,
        Ok("wake-refuses") => Case::WakeRefuses,
        Ok("saved-expires") => Case::SavedExpires,
        _ => return,
    };
    mount_for_proof().unwrap();
    std::thread::Builder::new()
        .name("installed-delivery".into())
        .stack_size(32 << 20)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(Box::pin(run(case)));
        })
        .unwrap()
        .join()
        .unwrap();
}

async fn run(case: Case) {
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
    let mut service = Service::new(std::time::Instant::now());
    for expected in [1, 2] {
        assert_eq!(
            service.poll_connect(std::time::Instant::now()),
            ConnectionAction::Connect
        );
        relay.recipient_fault(RecipientFault::WrongLiveEose);
        let failure = match PeerRecipientPort::open(admission(), time_limit()).await {
            Ok(_) => panic!("malformed live EOSE granted a recipient port"),
            Err(failure) => failure,
        };
        assert_eq!(failure, Failure::Grammar);
        let now = std::time::Instant::now();
        service.refuse(now, Refusal::from(Error::Socket(failure)));
        let eligible = now + Duration::from_secs(expected);
        assert_eq!(
            service.poll_connect(std::time::Instant::now()),
            ConnectionAction::WaitUntil(eligible)
        );
        tokio::time::sleep_until(eligible.into()).await;
    }
    assert_eq!(
        service.poll_connect(std::time::Instant::now()),
        ConnectionAction::Connect
    );
    let mut recipient = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    assert!(service.authenticated(std::time::Instant::now()));
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
    let mut consent = codec
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
    let expires = if case == Case::SavedExpires {
        wall() + 7 + 300
    } else {
        consent.flight().expires_at - 1
    };
    let delivery = codec
        .prepare_delivery(&founder, &consent, &welcome, wall(), expires)
        .unwrap();
    assert!(
        sender
            .publish(&delivery, delivery.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    if case == Case::SavedExpires {
        let mut new_port = PeerRecipientPort::open(admission(), time_limit())
            .await
            .unwrap();
        let outer: nostr::event::Event = serde_json::from_slice(delivery.frame()).unwrap();
        let stamp = outer.created_at.as_secs();
        let page = new_port.page([stamp, stamp], time_limit()).await.unwrap();
        let id = delivery.flight.flight().carrier_id.to_bytes();
        let index = page
            .records()
            .iter()
            .position(|record| record.id == id)
            .unwrap();
        let disposition = fixture
            .delivery_input(&mut new_port, Input::History(&page, index))
            .unwrap();
        assert_eq!(disposition.status, Status::Deferred);
        assert_saved_expiry(&mut fixture, &mut new_port, expires - 300).await;
        return;
    }
    let wake_refuses = case == Case::WakeRefuses;
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
    service.disconnected(std::time::Instant::now());
    assert_eq!(
        service.poll_connect(std::time::Instant::now()),
        ConnectionAction::Connect
    );

    let mut recipient = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    assert!(service.authenticated(std::time::Instant::now()));
    let cursor = fixture
        .scan_delivery(&mut recipient, ScanStart::Recovery)
        .unwrap()
        .unwrap();
    let delivery_event = delivery.flight.flight().carrier_id;
    let delivery_id = delivery_event.to_bytes();
    let before = delivery_state(&fixture);
    for (fault, failure, retirement, cooldown) in [
        (
            RecipientFault::OversizedHistory,
            Failure::Size,
            Retirement::Size,
            4,
        ),
        (
            RecipientFault::ConflictingHistory(delivery_event),
            Failure::ConflictingHistory,
            Retirement::PageContract,
            8,
        ),
    ] {
        relay.recipient_fault(fault);
        let outer: nostr::event::Event = serde_json::from_slice(delivery.frame()).unwrap();
        let event_stamp = outer.created_at.as_secs();
        let rejected = recipient
            .page([event_stamp, event_stamp], time_limit())
            .await;
        assert_eq!(rejected.err(), Some(failure));
        assert!(matches!(
            recipient.wait_live(time_limit()).await,
            Err(Failure::Retired)
        ));
        assert_eq!(delivery_state(&fixture), before);
        let reason = Refusal::from(Error::Socket(failure));
        assert_eq!(reason, Refusal::RetireRelay(retirement));
        let now = std::time::Instant::now();
        service.refuse(now, reason);
        let eligible = now + Duration::from_secs(cooldown);
        assert_eq!(
            service.poll_connect(now),
            ConnectionAction::WaitUntil(eligible)
        );
        tokio::time::sleep_until(eligible.into()).await;
        assert_eq!(
            service.poll_connect(std::time::Instant::now()),
            ConnectionAction::Connect
        );
        recipient = PeerRecipientPort::open(admission(), time_limit())
            .await
            .unwrap();
        assert!(service.authenticated(std::time::Instant::now()));
    }
    let page = recipient
        .page([cursor.lower, cursor.until], time_limit())
        .await
        .unwrap();
    assert_eq!(page.records().len(), 4);
    let observed_delivery = page
        .records()
        .iter()
        .find(|arrival| arrival.id == delivery_id)
        .expect("published Delivery present in checked history")
        .body
        .get()
        .as_bytes()
        .to_vec();

    let delivery_index = page
        .records()
        .iter()
        .position(|record| record.id == delivery_id)
        .unwrap();
    let before = delivery_state(&fixture);
    let inspected = observe_delivery(
        &mut fixture,
        &mut recipient,
        Input::History(&page, delivery_index),
    );
    let (stage, checked_wall) = inspected.native_coordinates();
    let native = fixture.attest(stage, checked_wall).unwrap();
    recipient.stop(time_limit()).await.unwrap();
    service.disconnected(std::time::Instant::now());
    assert_eq!(
        service.poll_connect(std::time::Instant::now()),
        ConnectionAction::Connect
    );
    let mut recipient = PeerRecipientPort::open(admission(), time_limit())
        .await
        .unwrap();
    assert!(service.authenticated(std::time::Instant::now()));
    let page = recipient
        .page([cursor.lower, cursor.until], time_limit())
        .await
        .unwrap();
    let after_reconnect = page
        .records()
        .iter()
        .position(|record| record.id == delivery_id)
        .unwrap();
    let state = &mut fixture.owner.state;
    let refused = recipient.use_page_carrier(&page, after_reconnect, |carrier| {
        state
            .checkpoint
            .finish_installed_delivery(&state.source, carrier, *inspected, &native)
    });
    assert!(matches!(
        refused,
        Ok(Err(consent_checkpoint::Error::Source))
    ));
    assert_eq!(delivery_state(&fixture), before);

    let started = std::time::Instant::now();
    let (statuses, next) = {
        let mut batch = service.batch().unwrap();
        assert_eq!(batch.choose(true, false, false), Work::History);
        let result = fixture.delivery_page(&mut recipient, &cursor, &page);
        batch.history_result(&cursor, &result);
        result.unwrap()
    };
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

    relay.recipient_fault(RecipientFault::ReformattedHistory(delivery_event));
    let reformatted = recipient
        .page([cursor.lower, cursor.until], time_limit())
        .await
        .unwrap();
    let reformatted_index = reformatted
        .records()
        .iter()
        .position(|record| record.id == delivery_id)
        .unwrap();
    assert!(
        reformatted.records()[reformatted_index]
            .body
            .get()
            .as_bytes()
            != observed_delivery
    );
    let before = delivery_state(&fixture);
    let result = fixture.delivery_input(
        &mut recipient,
        Input::History(&reformatted, reformatted_index),
    );
    assert_eq!(
        result.err().map(Refusal::from),
        Some(Refusal::RetireRelay(Retirement::LexicalConflict))
    );
    assert_eq!(delivery_state(&fixture), before);

    // A second carrier can match the same signed Consent and package. It
    // cannot replace the exact Delivery that already owns the retry slot.
    let competing = codec
        .prepare_delivery(
            &founder,
            &consent,
            &welcome,
            wall(),
            consent.flight().expires_at - 1,
        )
        .unwrap();
    let competing_id = competing.flight.flight().carrier_id.to_bytes();
    assert_ne!(competing_id, delivery_id);
    assert!(
        sender
            .publish(&competing, competing.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let deadline = time_limit();
    let competing_live = loop {
        let record = recipient.wait_live(deadline).await.unwrap().unwrap();
        if record.arrival().id == competing_id {
            break record;
        }
    };
    assert_eq!(
        fixture
            .delivery_input(&mut recipient, Input::Live(&competing_live))
            .unwrap()
            .status,
        Status::Invalid
    );

    consent.fixture_flip_package_reference();
    let wrong_package = codec
        .prepare_delivery(
            &founder,
            &consent,
            &welcome,
            wall(),
            consent.flight().expires_at - 1,
        )
        .unwrap();
    consent.fixture_flip_package_reference();
    let wrong_id = wrong_package.flight.flight().carrier_id.to_bytes();
    assert!(
        sender
            .publish(&wrong_package, wrong_package.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let deadline = time_limit();
    let wrong_live = loop {
        let record = recipient.wait_live(deadline).await.unwrap().unwrap();
        if record.arrival().id == wrong_id {
            break record;
        }
    };
    let before = delivery_state(&fixture);
    let reads = fixture.package_reads.get();
    let result = fixture.delivery_input(&mut recipient, Input::Live(&wrong_live));
    assert_eq!(result.unwrap().status, Status::Invalid);
    assert_eq!(fixture.package_reads.get(), reads + 1);
    let after = delivery_state(&fixture);
    assert_eq!(before[..2], after[..2]);
    assert_eq!(before[4..], after[4..]);

    let expires = wall() + 7;
    let expiring = codec
        .prepare_delivery(&founder, &consent, &welcome, wall(), expires + 300)
        .unwrap();
    assert!(
        sender
            .publish(&expiring, expiring.flight.flight(), time_limit())
            .await
            .unwrap()
    );
    let expiring_id = expiring.flight.flight().carrier_id.to_bytes();
    let deadline = time_limit();
    let expiring_live = loop {
        let record = recipient.wait_live(deadline).await.unwrap().unwrap();
        if record.arrival().id == expiring_id {
            break record;
        }
    };
    assert_observed_expiry(&mut fixture, &mut recipient, &expiring_live, expires).await;

    // The first retry is due one second after deferral. Confirm the
    // stored work is actually eligible before asserting no native read.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let state = &mut fixture.owner.state;
    assert!(
        recipient
            .use_current_recipient(|account, origin, wall| {
                state
                    .checkpoint
                    .due_installed_delivery(&state.source, account, origin, wall)
            })
            .unwrap()
            .unwrap()
            .is_some()
    );
    let reads = fixture.package_reads.get();
    assert_eq!(fixture.retry_delivery(&mut recipient).unwrap(), None);
    assert_eq!(fixture.package_reads.get(), reads);

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
    let stamp = competing_live.arrival().stamp;
    let page = recipient.page([stamp, stamp], time_limit()).await.unwrap();
    let index = page
        .records()
        .iter()
        .position(|record| record.id == competing_id)
        .unwrap();
    let competing_inspection =
        observe_delivery(&mut fixture, &mut recipient, Input::History(&page, index));
    let (stage, sample) = competing_inspection.native_coordinates();
    let package = fixture.attest(stage, sample).unwrap();
    if wake_refuses {
        fixture.cut = Some(Cut::ReadOnlyDeliveryWake);
    }
    assert_eq!(
        recipient
            .publish_prepared_consent(&mut fixture, selector, time_limit())
            .await,
        PublicationOutcome::Settled
    );
    if wake_refuses {
        assert!(fixture.wake_failure.is_some());
        assert!(
            fixture
                .queued(selector, wall())
                .unwrap()
                .unwrap()
                .publication_state()
                .2
                .is_some(),
            "the failed wake must preserve the settled Consent"
        );
    } else {
        assert!(fixture.wake_failure.is_none());
    }
    let before = delivery_state(&fixture);
    let state = &mut fixture.owner.state;
    let competes = recipient
        .use_page_carrier(&page, index, |carrier| {
            state.checkpoint.finish_installed_delivery(
                &state.source,
                carrier,
                *competing_inspection,
                &package,
            )
        })
        .unwrap()
        .unwrap();
    assert_eq!(competes.status, Status::Invalid);
    let after = delivery_state(&fixture);
    assert_eq!(before[..2], after[..2]);
    assert_eq!(before[4..], after[4..]);
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
    fixture
        .delivery_page(&mut recipient, &cursor, &page)
        .unwrap();
    // The relay sorts obfuscated outer timestamps. The first bounded
    // recovery page need not include this exact Delivery.
    let outer: nostr::event::Event = serde_json::from_slice(delivery.frame()).unwrap();
    let held_stamp = outer.created_at.as_secs();
    let page = recipient
        .page([held_stamp, held_stamp], time_limit())
        .await
        .unwrap();
    let index = page
        .records()
        .iter()
        .position(|record| record.id == delivery_id)
        .unwrap();
    let reads = fixture.package_reads.get();
    assert_eq!(
        fixture
            .delivery_input(&mut recipient, Input::History(&page, index))
            .unwrap()
            .status,
        Status::Accepted(TrustedOutcome::AlreadyApplied),
    );
    assert_eq!(fixture.package_reads.get(), reads);
    let page = recipient.page([stamp, stamp], time_limit()).await.unwrap();
    let index = page
        .records()
        .iter()
        .position(|r| r.id == competing_id)
        .unwrap();
    let reads = fixture.package_reads.get();
    assert_eq!(
        fixture
            .delivery_input(&mut recipient, Input::History(&page, index))
            .unwrap()
            .status,
        Status::Invalid,
    );
    assert_eq!(fixture.package_reads.get(), reads);
    eprintln!(
        "installed_delivery wake_refuses={wake_refuses} mixed_ms={page_millis} retry_cold_ms={retry_cold_ms} ack_to_held_ms={held_ms} held_cold_ms={held_cold_ms} delivery_bytes={}",
        observed_delivery.len()
    );
}

fn delivery_state(fixture: &Fixture<'_>) -> [i64; 6] {
    let state = &fixture.owner.state;
    state
        .checkpoint
        .fixture_delivery_state(&state.source)
        .unwrap()
}

fn observe_delivery(
    fixture: &mut Fixture<'_>,
    recipient: &mut PeerRecipientPort,
    input: Input<'_>,
) -> Box<consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::Prepared> {
    let state = &mut fixture.owner.state;
    let observed = input
        .observe(recipient, |carrier| {
            state
                .checkpoint
                .observe_installed_delivery(&state.source, carrier)
        })
        .unwrap()
        .unwrap();
    let Observation::Inspect(inspected) = observed else {
        panic!("Delivery should still require inspected KeyPackage evidence");
    };
    inspected
}

async fn assert_observed_expiry(
    fixture: &mut Fixture<'_>,
    recipient: &mut PeerRecipientPort,
    live: &LiveRecord,
    expires: u64,
) {
    let before = delivery_state(fixture);
    let prepared = observe_delivery(fixture, recipient, Input::Live(live));
    let (stage, checked_wall) = prepared.native_coordinates();
    let native = fixture.attest(stage, checked_wall).unwrap();
    while wall() < expires {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    let state = &mut fixture.owner.state;
    let refused = Input::Live(live)
        .observe(recipient, |carrier| {
            state
                .checkpoint
                .finish_installed_delivery(&state.source, carrier, *prepared, &native)
        })
        .unwrap();
    assert_eq!(refused.err(), Some(consent_checkpoint::Error::Expired));
    assert_eq!(delivery_state(fixture), before);
    let reads = fixture.package_reads.get();
    assert_eq!(
        fixture
            .delivery_input(recipient, Input::Live(live))
            .unwrap()
            .status,
        Status::Invalid,
    );
    assert_eq!(fixture.package_reads.get(), reads);
    let after = delivery_state(fixture);
    assert_eq!(before[..2], after[..2]);
    assert_eq!(before[4..], after[4..]);
}

async fn assert_saved_expiry(
    fixture: &mut Fixture<'_>,
    recipient: &mut PeerRecipientPort,
    expires: u64,
) {
    while wall() < expires {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    // No negative fault touches the signed carrier, revision, or Consent.
    let replace_expiry = |fixture: &Fixture<'_>, value| {
        fixture
            .owner
            .state
            .checkpoint
            .fixture_statement(&format!("UPDATE peer_cursor_deferred SET expires={value}"));
    };
    let reads = fixture.package_reads.get();
    let before = delivery_state(fixture);
    assert_eq!(before[4..], [1, 0]);

    replace_expiry(fixture, expires + 1);
    assert_eq!(
        fixture.retry_delivery(recipient).err(),
        Some(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::Classification
        ))),
    );
    assert_eq!(fixture.package_reads.get(), reads);
    let after = delivery_state(fixture);
    assert_eq!(before[..2], after[..2]);
    assert_eq!(before[4..], after[4..]);
    assert_eq!(after[2], after[3]);

    replace_expiry(fixture, expires - 1);
    assert_eq!(
        fixture.retry_delivery(recipient).err(),
        Some(Error::Checkpoint(consent_checkpoint::Error::Cursor(
            store::Error::Classification
        ))),
    );
    assert_eq!(fixture.package_reads.get(), reads + 1);
    let after = delivery_state(fixture);
    assert_eq!(before[..2], after[..2]);
    assert_eq!(before[4..], after[4..]);
    assert_eq!(after[2], after[3]);

    replace_expiry(fixture, expires);
    let started = std::time::Instant::now();
    assert_eq!(fixture.retry_delivery(recipient).unwrap(), None);
    assert_eq!(fixture.package_reads.get(), reads + 2);
    let after = delivery_state(fixture);
    assert_eq!(before[..2], after[..2]);
    assert_eq!(after[4..], [0, 0]);
    assert_eq!(after[2], after[3]);
    assert!(after[2] >= i64::try_from(expires).unwrap());
    eprintln!(
        "installed_delivery saved_expiry_retirement_ms={}",
        started.elapsed().as_millis()
    );
}
