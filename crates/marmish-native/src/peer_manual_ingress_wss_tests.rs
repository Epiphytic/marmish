use crate::{
    mdk_adapter::terminal_relay_fixture::{RecipientFault, Relay},
    peer_invitation::{
        receiver_inventory::manual_ingress::{Ingress, require_fatal},
        socket::wire::CARRIER,
    },
    peer_relay_trust::candidate_registration::ForwardingVerifier,
};

use super::*;

pub(super) struct Fixture<'a, 'profile> {
    pub(super) case: &'a str,
    pub(super) relay: &'a Relay,
    pub(super) port: &'a mut PeerRecipientPort,
    pub(super) sender: &'a mut Owner,
    pub(super) founder: &'a Keys,
    pub(super) account: PublicKey,
    pub(super) owner: &'a mut RecoveredOwner<'profile>,
    pub(super) forwarding: ForwardingVerifier,
}

#[test]
#[ignore = "opt-in registered same-owner Manual ingress over genuine private WSS"]
fn manual_ingress_attributes_history_and_live_on_one_private_port() {
    run_cases(&[
        "ingress",
        "ingress-bad-history",
        "ingress-backlog",
        "ingress-delivery",
    ]);
}

pub(super) async fn exercise(fixture: Fixture<'_, '_>) {
    let Fixture {
        case,
        relay,
        port,
        sender,
        founder,
        account,
        owner,
        forwarding,
    } = fixture;
    let (startup, listener, worker) = peer_candidate_startup::pair();
    startup.ordinary_ready();
    listener.registered();
    let ready = worker.ready().await.unwrap();
    let readiness = ready.readiness();
    let stop = Arc::new(AtomicBool::new(false));
    let deadline = || tokio::time::Instant::now() + Duration::from_secs(15);
    let writes = || relay.publication_attempts(CARRIER);
    let before = writes();
    let codec = wire::Codec::for_version(wire::MAX_OUTER_BYTES, ControlVersion::V2).unwrap();
    let now = Wall::System.sample().unwrap().as_secs();
    let offer = |index: u8, stamp: u64| {
        codec
            .prepare_offer(
                founder,
                account,
                [index; 16],
                &NostrRoutingV1::new([index; 32], vec![relay.url.to_string()]).unwrap(),
                stamp,
            )
            .unwrap()
    };
    // Use four expired offers in the backfill window. The only allowed
    // founder may have only one live Offer, reserved for the live check.
    let past = if case == "ingress-backlog" {
        now
    } else {
        now - 86_405
    };
    let history = [
        offer(11, past),
        offer(12, past),
        offer(13, past),
        offer(14, past),
    ];
    let (_, alien) = crate::dispatcher_fatal::Trip::new();
    port.use_current_native(|route| {
        require_fatal(&route, &forwarding.fatal()).unwrap();
        assert!(matches!(
            require_fatal(&route, &alien),
            Err(receiver_inventory::Error::Binding)
        ));
    })
    .unwrap();
    for message in &history {
        assert!(
            sender
                .publish(message, message.flight.flight(), deadline())
                .await
                .unwrap()
        );
    }

    let mut ingress = Ingress::recover(
        owner,
        port,
        readiness.clone(),
        stop.clone(),
        forwarding.clone(),
    )
    .unwrap();
    assert!(Ingress::recover(owner, port, readiness, stop.clone(), forwarding).is_err());
    if case != "ingress" {
        let before_page = owner
            .checkpoint
            .fixture_delivery_state(&owner.source)
            .unwrap();
        if case == "ingress-bad-history" {
            // The oversized actual WSS response cannot establish checked EOSE.
            relay.recipient_fault(RecipientFault::OversizedHistory);
        }
        let outcome = ingress.history_turn(owner, port, Some(deadline())).await;
        if case == "ingress-backlog" {
            use crate::peer_invitation::{receiver_inventory::Error, store::Error as CursorError};
            assert!(matches!(
                outcome,
                Err(Error::Checkpoint(consent_checkpoint::Error::Cursor(
                    CursorError::Backlog
                )))
            ));
        } else {
            assert!(outcome.is_err());
        }
        let after_page = owner
            .checkpoint
            .fixture_delivery_state(&owner.source)
            .unwrap();
        // No page revision or retry or custody row was advanced on either error.
        assert_eq!(after_page[0], before_page[0]);
        assert_eq!(&after_page[4..], &before_page[4..]);
        assert!(
            ingress
                .live_turn(owner, port, deadline(), || {})
                .await
                .is_err()
        );
        assert_eq!(writes(), before + 4);
        return;
    }

    assert!(
        !ingress
            .history_turn(owner, port, Some(deadline()))
            .await
            .unwrap()
    );
    assert!(
        ingress
            .history_turn(owner, port, Some(deadline()))
            .await
            .unwrap()
    );
    assert_eq!(writes(), before + 4);

    let message = offer(15, now);
    assert!(
        sender
            .publish(&message, message.flight.flight(), deadline())
            .await
            .unwrap()
    );
    assert!(matches!(
        ingress
            .live_turn(owner, port, deadline(), || {})
            .await
            .unwrap(),
        Some(consent_checkpoint::Status::Accepted(_))
    ));
    assert_eq!(
        ingress
            .live_turn(owner, port, deadline(), || {})
            .await
            .unwrap(),
        None
    );
    assert_eq!(writes(), before + 5);

    stop.store(true, std::sync::atomic::Ordering::Release);
    assert!(
        ingress
            .live_turn(owner, port, deadline(), || {})
            .await
            .is_err()
    );
    assert_eq!(writes(), before + 5);
}

pub(super) async fn measured_delivery(
    ingress: &mut Ingress,
    fixture: &mut joined_port::Fixture<'_, '_>,
    owner: &mut RecoveredOwner<'_>,
    queued: &ManagedQueued,
) {
    let before = fixture.relay.publication_attempts(CARRIER);
    let carrier = fixture.send_delivery(queued).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if ingress
            .live_turn(owner, fixture.port, deadline, || {})
            .await
            .unwrap()
            == Some(consent_checkpoint::Status::Deferred)
        {
            break;
        }
        assert!(tokio::time::Instant::now() < deadline);
    }
    owner
        .checkpoint
        .fixture_retry_row(&owner.source, carrier)
        .unwrap();
    assert_eq!(fixture.relay.publication_attempts(CARRIER), before + 1);
}
