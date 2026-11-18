//! Offline sender-side verification of the durable Consent transcript.

use super::*;

pub(crate) enum ClientCarrier {
    Offer,
    Delivery(Box<IncomingDelivery>),
}

pub(crate) struct DeliveryOffer {
    pub(crate) server: PublicKey,
    pub(crate) id: [u8; 16],
    pub(crate) revision: [u8; 32],
}

pub(crate) struct IncomingDelivery {
    carrier: EventId,
    stamp: u64,
    rumor: UnsignedEvent,
    wire: DeliveryWire,
    offer: DeliveryOffer,
}

impl IncomingDelivery {
    pub(crate) fn offer(&self) -> &DeliveryOffer {
        &self.offer
    }

    pub(crate) fn matches_index(&self, id: [u8; 32], stamp: u64) -> bool {
        self.carrier.to_bytes() == id && self.stamp == stamp
    }

    fn verify(
        self,
        offer: &Offer,
        transcript: Transcript,
        package: &VerifiedPackage,
        wall: u64,
    ) -> Result<Delivery> {
        Codec::bind_delivery(
            self.carrier,
            &self.rumor,
            self.wire,
            DeliveryAuthority {
                offer,
                revision: EventId::from_byte_array(transcript.revision),
                window: transcript.created_at..transcript.expires,
                package,
            },
            wall,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Transcript {
    pub(crate) carrier: [u8; 32],
    pub(crate) revision: [u8; 32],
    pub(crate) created_at: u64,
    pub(crate) expires: u64,
    pub(crate) package_ref: [u8; 32],
    pub(crate) package_slot: [u8; 16],
}

impl Transcript {
    pub(crate) fn from_consent(consent: &Consent) -> Self {
        let flight = consent.flight();
        let binding = consent.binding();
        Self {
            carrier: flight.carrier_id.to_bytes(),
            revision: flight.revision.to_bytes(),
            created_at: flight.created_at,
            expires: flight.expires_at,
            package_ref: binding.reference,
            package_slot: binding.slot,
        }
    }

    pub(crate) fn verify(
        self,
        signer: &Keys,
        offer: &Offer,
        frame: &[u8],
        exact_package: &[u8],
        now: u64,
    ) -> Result<()> {
        self.verified_package(signer, offer, frame, exact_package, now)
            .map(|_| ())
    }

    pub(crate) fn verify_delivery(
        self,
        signer: &Keys,
        offer: &Offer,
        frame: &[u8],
        exact_package: &[u8],
        wall: u64,
        delivery: IncomingDelivery,
    ) -> Result<Delivery> {
        let package = self.verified_package(signer, offer, frame, exact_package, wall)?;
        delivery.verify(offer, self, &package, wall)
    }

    fn verified_package(
        self,
        signer: &Keys,
        offer: &Offer,
        frame: &[u8],
        exact_package: &[u8],
        now: u64,
    ) -> Result<VerifiedPackage> {
        if signer.public_key() != offer.flight.recipient
            || now >= self.expires
            || self.created_at > deadline(now, SKEW)?
            || frame.len() > MAX_OUTER_BYTES
        {
            return Err(Error::Binding);
        }
        let server = offer.flight.author;
        let outer = seal::verify_signed(frame, Error::Outer)?;
        let mut tags = outer.tags.iter();
        if outer.id.to_bytes() != self.carrier
            || outer.kind != OUTER
            || [server, signer.public_key()].contains(&outer.pubkey)
            || outer.content.len() > OUTER_CONTENT_CAP
            || outer.tags.len() != 2
            || !tags.next().is_some_and(|tag| is_recipient_tag(tag, server))
            || !tags.next().is_some_and(valid_retention_tag)
        {
            return Err(Error::Outer);
        }
        let native = artifact::verify_package(
            exact_package,
            signer.public_key(),
            self.created_at,
            &offer.routing.relays,
        )?;
        consent_times(offer.flight, &native, self.created_at, self.expires, now)?;
        if native.binding.reference != self.package_ref || native.binding.slot != self.package_slot
        {
            return Err(Error::Binding);
        }
        let wire = ConsentWire {
            v: 1,
            expires_at: self.expires,
            offer_id: hex::encode(offer.offer_id),
            offer_revision: offer.flight.revision.to_hex(),
            package_event_id: hex::encode(native.binding.event_id),
            package_ref: hex::encode(native.binding.reference),
            package_slot: hex::encode(native.binding.slot),
            package: STANDARD.encode(exact_package),
        };
        let rumor = create_rumor(CONSENT, signer, server, self.created_at, &wire)?;
        if rumor.id.map(EventId::to_bytes) != Some(self.revision) {
            return Err(Error::Binding);
        }
        Ok(native)
    }
}

impl Codec {
    pub(crate) fn classify_client(&self, signer: &Keys, frame: &[u8]) -> Result<ClientCarrier> {
        let (carrier, stamp, rumor) = self.open_authenticated(signer, frame, None)?;
        match rumor.kind {
            kind if kind == OFFER => Ok(ClientCarrier::Offer),
            kind if kind == DELIVERY => {
                let wire: DeliveryWire =
                    serde_json::from_str(&rumor.content).map_err(|_| Error::Body)?;
                let offer = DeliveryOffer {
                    server: rumor.pubkey,
                    id: fixed_hex(&wire.offer_id, Error::Body)?,
                    revision: event_id(&wire.offer_revision)?.to_bytes(),
                };
                Ok(ClientCarrier::Delivery(Box::new(IncomingDelivery {
                    carrier,
                    stamp,
                    rumor,
                    wire,
                    offer,
                })))
            }
            _ => Err(Error::Rumor),
        }
    }

    pub(crate) fn prepare_capped_fixture_consent(
        &self,
        signer: &Keys,
        offer: &Offer,
        exact_package: &[u8],
        event_expires: u64,
        wall: u64,
    ) -> Result<Prepared<Consent>> {
        let created_at = child_time(offer.flight, wall)?;
        let package = artifact::verify_package(
            exact_package,
            signer.public_key(),
            created_at,
            &offer.routing.relays,
        )?;
        // Matches InvitationLane::publish_approved's owned wall + 86,400 material cap.
        const LEGACY_MATERIAL_TTL: u64 = 86_400;
        let expires = package
            .not_after
            .min(event_expires)
            .min(deadline(created_at, FLIGHT_TTL)?)
            .min(deadline(wall, LEGACY_MATERIAL_TTL)?);
        self.prepare_consent(signer, offer, exact_package, wall, expires)
    }
}
