//! External founder for the installed Manual recipient proof.

use std::{path::Path, sync::Arc};

use cgka_traits::app_components::NostrRoutingV1;
use nostr::{
    event::EventId,
    key::{Keys, PublicKey},
};
use tokio::time::{Instant, timeout_at};

use crate::{
    mdk_adapter::{
        peer_staged_proof_tests::peer_enrollment_welcome, terminal_relay_fixture::Relay,
    },
    peer_control_version::ControlVersion,
    peer_invitation::{
        Wall,
        socket::owner::Owner,
        store::consent_checkpoint::{
            Selector,
            wire::{Codec, Consent, MAX_OUTER_BYTES, Offer},
        },
    },
};

const DELIVERY_LIFETIME: u64 = 600;
const HELD_EXPIRY_MARGIN: u64 = 300;

pub(super) struct FounderFixture<'a> {
    socket: Owner,
    signer: Arc<Keys>,
    recipient: PublicKey,
    root: &'a Path,
    codec: Codec,
}

pub(super) struct Offered(Offer);

impl Offered {
    pub(super) fn selector(&self) -> Selector {
        Selector::from(&self.0)
    }

    pub(super) fn carrier(&self) -> EventId {
        self.0.flight().carrier_id
    }
}

pub(super) struct Consented(Consent);

impl Consented {
    pub(super) fn carrier(&self) -> EventId {
        self.0.flight().carrier_id
    }
}

pub(super) struct Delivered {
    pub(super) carrier: EventId,
    pub(super) pending_expires_at: u64,
}

impl<'a> FounderFixture<'a> {
    pub(super) async fn open(
        relay: &Relay,
        signer: Arc<Keys>,
        recipient: PublicKey,
        root: &'a Path,
        deadline: Instant,
    ) -> Self {
        assert_ne!(signer.public_key(), recipient);
        let connector = relay.connector(root, true, None);
        let socket = Owner::open(
            &connector,
            relay.url.as_str(),
            signer.public_key(),
            &signer,
            deadline,
        )
        .await
        .expect("authenticated founder socket");
        let codec = Codec::for_version(MAX_OUTER_BYTES, ControlVersion::V2).expect("v2 codec");
        Self {
            socket,
            signer,
            recipient,
            root,
            codec,
        }
    }

    pub(super) async fn publish_offer(&mut self, deadline: Instant) -> Offered {
        let mut route = [0; 32];
        getrandom::fill(&mut route).expect("disposable routing ID");
        let mut room = [0; 16];
        getrandom::fill(&mut room).expect("disposable room ID");
        let routing = NostrRoutingV1::new(route, vec![self.socket.origin().to_owned()]).unwrap();
        let offer = self
            .codec
            .prepare_offer(&self.signer, self.recipient, room, &routing, wall())
            .expect("signed v2 Offer");
        assert!(
            self.socket
                .publish(&offer, offer.flight.flight(), deadline)
                .await
                .expect("correlated Offer acknowledgement")
        );
        Offered(offer.flight)
    }

    pub(super) async fn await_verified_consent(
        &mut self,
        offered: &Offered,
        deadline: Instant,
    ) -> Consented {
        let record = self
            .socket
            .wait_live(deadline)
            .await
            .expect("founder inbox")
            .expect("Consent before deadline");
        assert!(self.socket.accepts_live(&record));
        let arrival = record.arrival();
        let consent = self
            .codec
            .open_consent(
                &self.signer,
                arrival.body.get().as_bytes(),
                &offered.0,
                wall(),
            )
            .expect("v2 Consent and native KeyPackage for the exact signed Offer");
        assert_eq!(consent.flight().carrier_id.to_bytes(), arrival.id);
        assert_eq!(consent.flight().author, self.recipient);
        assert_eq!(consent.flight().recipient, self.signer.public_key());
        assert_eq!(Selector::from(consent.offer()), offered.selector());
        assert_eq!(consent.offer().routing().relays, [self.socket.origin()]);
        Consented(consent)
    }

    pub(super) async fn publish_delivery(
        &mut self,
        consented: &Consented,
        deadline: Instant,
    ) -> Delivered {
        let consent = &consented.0;
        let welcome = timeout_at(
            deadline,
            peer_enrollment_welcome(
                self.root,
                Arc::clone(&self.signer),
                self.recipient,
                consent.offer().routing(),
                consent.offer().room_id(),
                consent.key_package().clone(),
            ),
        )
        .await
        .expect("native Welcome before deadline");

        let now = wall();
        // The codec checks the Consent and KeyPackage lifetimes before signing.
        // Compute the expected Pending expiry from that signed source and the
        // protocol margin. The controller has about five minutes from Delivery.
        let expiry = now.checked_add(DELIVERY_LIFETIME).unwrap();
        let delivery = self
            .codec
            .prepare_delivery(&self.signer, consent, &welcome, now, expiry)
            .expect("signed v2 Delivery and exact native Welcome");
        let flight = delivery.flight.flight();
        assert_eq!(flight.expires_at, expiry);
        assert!(
            self.socket
                .publish(&delivery, flight, deadline)
                .await
                .expect("correlated Delivery acknowledgement")
        );
        Delivered {
            carrier: flight.carrier_id,
            pending_expires_at: expiry - HELD_EXPIRY_MARGIN,
        }
    }
}

fn wall() -> u64 {
    Wall::System.sample().expect("fixture wall").as_secs()
}
