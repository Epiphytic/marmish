//! Offline sender-side verification of the durable Consent transcript.

use sha2::{Digest as _, Sha256};

use super::*;
use crate::peer_invitation::HeldEvidence;

pub(crate) struct ClientOuter {
    event: Event,
    frame_digest: [u8; 32],
}

impl ClientOuter {
    pub(crate) fn matches_index(&self, id: [u8; 32], stamp: u64) -> bool {
        self.event.id.to_bytes() == id && self.event.created_at.as_secs() == stamp
    }
}

pub(crate) enum ClientCarrier {
    Offer,
    Delivery(Box<IncomingDelivery>),
}

pub(crate) struct DeliveryOffer {
    pub(crate) version: ControlVersion,
    pub(crate) server: PublicKey,
    pub(crate) id: [u8; 16],
    pub(crate) revision: [u8; 32],
}

pub(crate) struct IncomingDelivery {
    frame_digest: [u8; 32],
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
        binding: PackageBinding,
        lifetime: std::ops::Range<u64>,
        wall: u64,
    ) -> Result<Delivery> {
        self.verify_with(
            DeliveryAuthority {
                offer,
                revision: EventId::from_byte_array(transcript.revision),
                window: transcript.created_at..transcript.expires,
                package: binding,
                lifetime,
            },
            wall,
        )
    }

    fn verify_with(self, authority: DeliveryAuthority<'_>, wall: u64) -> Result<Delivery> {
        if self.offer.version != authority.offer.version() {
            return Err(Error::Binding);
        }
        Codec::bind_delivery(self.carrier, &self.rumor, self.wire, authority, wall)
    }
}

pub(crate) struct VerifiedDeliveryPackage {
    frame_digest: [u8; 32],
    binding: PackageBinding,
    bounds: (u64, u64),
    digest: [u8; 32],
}

impl VerifiedDeliveryPackage {
    pub(crate) fn frame_digest(&self) -> [u8; 32] {
        self.frame_digest
    }

    pub(crate) fn binding(&self) -> PackageBinding {
        self.binding
    }

    pub(crate) fn bounds(&self) -> (u64, u64) {
        self.bounds
    }

    pub(crate) fn digest(&self) -> [u8; 32] {
        self.digest
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
        delivery.verify(offer, self, package.binding, package.lifetime(), wall)
    }

    pub(crate) fn verify_delivery_with_package(
        self,
        signer: &Keys,
        offer: &Offer,
        frame: &[u8],
        exact_package: &[u8],
        wall: u64,
        incoming: IncomingDelivery,
    ) -> Result<Option<(Delivery, VerifiedDeliveryPackage)>> {
        let package = self.verified_package(signer, offer, frame, exact_package, wall)?;
        let frame_digest = incoming.frame_digest;
        let Ok(delivery) = incoming.verify(offer, self, package.binding, package.lifetime(), wall)
        else {
            return Ok(None);
        };
        Ok(Some((
            delivery,
            VerifiedDeliveryPackage {
                frame_digest,
                binding: package.binding,
                bounds: (package.not_before, package.not_after),
                digest: Sha256::digest(exact_package).into(),
            },
        )))
    }

    fn check_signed_consent(
        self,
        signer: &Keys,
        offer: &Offer,
        frame: &[u8],
        now: u64,
    ) -> Result<()> {
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
        Ok(())
    }

    fn verified_package(
        self,
        signer: &Keys,
        offer: &Offer,
        frame: &[u8],
        exact_package: &[u8],
        now: u64,
    ) -> Result<VerifiedPackage> {
        self.check_signed_consent(signer, offer, frame, now)?;
        let server = offer.flight.author;
        let native = artifact::verify_package(
            exact_package,
            signer.public_key(),
            self.created_at,
            &offer.routing.relays,
        )?;
        consent_times(
            offer.flight,
            native.lifetime(),
            self.created_at,
            self.expires,
            now,
        )?;
        if native.binding.reference != self.package_ref || native.binding.slot != self.package_slot
        {
            return Err(Error::Binding);
        }
        let wire = ConsentWire {
            v: offer.version() as u8,
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
    pub(crate) fn verify_client_outer(&self, frame: &[u8]) -> Result<ClientOuter> {
        if frame.len() > self.outer_cap {
            return Err(Error::Size);
        }
        Ok(ClientOuter {
            event: seal::verify_signed(frame, Error::Outer)?,
            frame_digest: Sha256::digest(frame).into(),
        })
    }

    pub(crate) fn classify_client(&self, signer: &Keys, frame: &[u8]) -> Result<ClientCarrier> {
        self.classify_verified_client(signer, self.verify_client_outer(frame)?)
    }

    pub(crate) fn classify_verified_client(
        &self,
        signer: &Keys,
        verified: ClientOuter,
    ) -> Result<ClientCarrier> {
        let (carrier, stamp, rumor) = Self::open_outer(signer, verified.event, None)?;
        match rumor.kind {
            kind if kind == OFFER => Ok(ClientCarrier::Offer),
            kind if kind == DELIVERY => {
                let wire: DeliveryWire =
                    serde_json::from_str(&rumor.content).map_err(|_| Error::Body)?;
                check_body(self.version, wire.v, &rumor.content, &wire)?;
                let offer = DeliveryOffer {
                    version: self.version,
                    server: rumor.pubkey,
                    id: fixed_hex(&wire.offer_id, Error::Body)?,
                    revision: event_id(&wire.offer_revision)?.to_bytes(),
                };
                Ok(ClientCarrier::Delivery(Box::new(IncomingDelivery {
                    frame_digest: verified.frame_digest,
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

    // Already-held authority comes only from the checkpoint's checked row
    // witness. It cannot be reconstructed from caller-supplied package bounds.
    pub(crate) fn reopen_custodied_delivery(
        &self,
        signer: &Keys,
        evidence: &HeldEvidence,
        wall: u64,
    ) -> Result<Delivery> {
        let ClientCarrier::Delivery(incoming) =
            self.classify_client(signer, evidence.signed_delivery())?
        else {
            return Err(Error::Rumor);
        };
        if !incoming.matches_index(evidence.delivery_carrier(), evidence.outer_stamp()) {
            return Err(Error::Binding);
        }
        let authority = self.saved_authority(signer, evidence, wall)?;
        let delivery = incoming.verify_with(authority, wall)?;
        let flight = delivery.flight();
        if wall >= evidence.held_deadline()
            || flight.created_at != evidence.flight_created_at()
            || flight.expires_at != evidence.delivery_expires()
            || flight.replay_safe_until != evidence.replay_until()
            || delivery.welcome_id().to_bytes() != evidence.welcome_id()
            || delivery.welcome_digest() != evidence.welcome_digest()
        {
            return Err(Error::Binding);
        }
        Ok(delivery)
    }

    pub(crate) fn verify_custody_conflict(
        &self,
        signer: &Keys,
        evidence: &HeldEvidence,
        incoming: IncomingDelivery,
        wall: u64,
    ) -> Result<bool> {
        // Expiry does not erase the original Consent's authenticated coordinates.
        // Verify the incoming carrier at the actual current wall.
        let authority = self.saved_authority(signer, evidence, evidence.held_at())?;
        Ok(incoming.verify_with(authority, wall).is_ok())
    }

    fn saved_authority<'a>(
        &self,
        signer: &Keys,
        evidence: &'a HeldEvidence,
        wall: u64,
    ) -> Result<DeliveryAuthority<'a>> {
        let offer = evidence.offer();
        if offer.version() != self.version {
            return Err(Error::Binding);
        }
        let transcript = evidence.transcript();
        transcript.check_signed_consent(signer, offer, evidence.signed_consent(), wall)?;
        let package = evidence.package();
        if package.reference != transcript.package_ref || package.slot != transcript.package_slot {
            return Err(Error::Binding);
        }
        let (not_before, not_after) = evidence.package_bounds();
        consent_times(
            offer.flight,
            not_before..not_after,
            transcript.created_at,
            transcript.expires,
            wall,
        )?;
        Ok(DeliveryAuthority {
            offer,
            revision: EventId::from_byte_array(transcript.revision),
            window: transcript.created_at..transcript.expires,
            package,
            lifetime: not_before..not_after,
        })
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
