//! Offline Peer invitation codec. Callers own local authority and relay-origin checks.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use cgka_traits::app_components::{NOSTR_ROUTING_MAX_RELAYS, NostrRoutingV1};
use cgka_traits::engine::KeyPackage;
use nostr::event::{Event, EventBuilder, EventId, FinalizeEvent, Kind, Tag, UnsignedEvent};
use nostr::key::{Keys, PublicKey};
use nostr::nips::nip44::{self, Version};
use nostr::types::Timestamp;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

#[path = "peer_control_wire_artifact.rs"]
mod artifact;
#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
#[path = "peer_prepared_consent_wire.rs"]
mod prepared_consent;
#[path = "peer_control_wire_seal.rs"]
mod seal;
#[cfg(test)]
pub(crate) use prepared_consent::{ClientCarrier, IncomingDelivery, VerifiedDeliveryPackage};

#[cfg(any(
    test,
    all(
        marmish_peer_receiver_candidate,
        target_os = "linux",
        target_arch = "x86_64"
    )
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Transcript {
    pub(crate) carrier: [u8; 32],
    pub(crate) revision: [u8; 32],
    pub(crate) created_at: u64,
    pub(crate) expires: u64,
    pub(crate) package_ref: [u8; 32],
    pub(crate) package_slot: [u8; 16],
}

use crate::peer_control_version::{ControlVersion, ReceiptCoordinates};

pub(crate) use self::artifact::PackageBinding;
use self::artifact::VerifiedPackage;
use self::seal::{BODY_CAP, OUTER_CONTENT_CAP, fixed_hex, is_exact_recipient, is_recipient_tag};

pub(crate) const MAX_OUTER_BYTES: usize = 64 * 1024;
const OUTER: Kind = Kind::Custom(7435);
const OFFER: Kind = Kind::Custom(24865);
const CONSENT: Kind = Kind::Custom(24866);
const DELIVERY: Kind = Kind::Custom(24867);
const FLIGHT_TTL: u64 = 86_400;
const SKEW: u64 = 300;
const REPLAY_MARGIN: u64 = 600;
const OFFSET_WINDOW: u64 = 172_800;
const RETENTION_BUCKET: u64 = 21_600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Size,
    Outer,
    Seal,
    Rumor,
    Body,
    Binding,
    Route,
    Time,
    Package,
    PackageLifetime,
    NativePackageLifetime,
    Welcome,
    Random,
}

type Result<T> = std::result::Result<T, Error>;

#[cfg_attr(
    any(
        test,
        all(
            marmish_peer_receiver_candidate,
            target_os = "linux",
            target_arch = "x86_64"
        )
    ),
    derive(Clone, Copy)
)]
pub(crate) struct Codec {
    outer_cap: usize,
    version: ControlVersion,
}

pub(crate) struct Prepared<T> {
    frame: Vec<u8>,
    pub(crate) flight: T,
}

impl<T> Prepared<T> {
    pub(crate) fn frame(&self) -> &[u8] {
        &self.frame
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Flight {
    pub(crate) carrier_id: EventId,
    pub(crate) revision: EventId,
    pub(crate) author: PublicKey,
    pub(crate) recipient: PublicKey,
    pub(crate) created_at: u64,
    pub(crate) expires_at: u64,
    pub(crate) replay_safe_until: u64,
}

#[derive(Clone)]
pub(crate) struct Offer {
    version: ControlVersion,
    flight: Flight,
    offer_id: [u8; 16],
    room_id: [u8; 16],
    routing: NostrRoutingV1,
}

impl Offer {
    pub(crate) fn version(&self) -> ControlVersion {
        self.version
    }

    pub(crate) fn flight(&self) -> Flight {
        self.flight
    }

    pub(crate) fn offer_id(&self) -> [u8; 16] {
        self.offer_id
    }

    pub(crate) fn room_id(&self) -> [u8; 16] {
        self.room_id
    }

    pub(crate) fn routing(&self) -> &NostrRoutingV1 {
        &self.routing
    }
}

pub(crate) struct Consent {
    flight: Flight,
    offer: Offer,
    native: VerifiedPackage,
    package: Zeroizing<Vec<u8>>,
}

impl Consent {
    pub(crate) fn flight(&self) -> Flight {
        self.flight
    }

    pub(crate) fn offer(&self) -> &Offer {
        &self.offer
    }

    pub(crate) fn binding(&self) -> PackageBinding {
        self.native.binding
    }

    pub(crate) fn key_package(&self) -> &KeyPackage {
        &self.native.package
    }

    pub(crate) fn package_bytes(&self) -> &[u8] {
        &self.package
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    pub(crate) fn fixture_flip_package_reference(&mut self) {
        self.native.binding.reference[0] ^= 1;
    }
}

pub(crate) struct Delivery {
    flight: Flight,
    offer: Offer,
    consent_revision: EventId,
    package: PackageBinding,
    welcome: Zeroizing<Vec<u8>>,
    welcome_id: EventId,
    welcome_digest: [u8; 32],
}

impl Delivery {
    pub(crate) fn flight(&self) -> Flight {
        self.flight
    }

    pub(crate) fn offer(&self) -> &Offer {
        &self.offer
    }

    pub(crate) fn consent_revision(&self) -> EventId {
        self.consent_revision
    }

    pub(crate) fn binding(&self) -> PackageBinding {
        self.package
    }

    pub(crate) fn welcome_bytes(&self) -> &[u8] {
        &self.welcome
    }

    pub(crate) fn welcome_id(&self) -> EventId {
        self.welcome_id
    }

    pub(crate) fn welcome_digest(&self) -> [u8; 32] {
        self.welcome_digest
    }

    pub(crate) fn status_negotiation(&self) -> Option<StatusNegotiation> {
        if self.offer.version() != ControlVersion::V2 {
            return None;
        }
        let offer = &self.offer;
        let receipt = ReceiptCoordinates {
            founder: offer.flight.author,
            recipient: offer.flight.recipient,
            room_id: offer.room_id,
            offer_id: offer.offer_id,
            offer_revision: offer.flight.revision.to_bytes(),
            welcome_id: self.welcome_id.to_bytes(),
        };
        Some(StatusNegotiation {
            app_room: status_app_room(&receipt),
            receipt,
            consent_revision: self.consent_revision.to_bytes(),
            delivery_revision: self.flight.revision.to_bytes(),
            routing: offer.routing.clone(),
        })
    }
}

/// Signed enrollment coordinates. The installed owner still has to verify
/// native acceptance, the authenticated MLS sender, and current group authority.
pub(crate) struct StatusNegotiation {
    receipt: ReceiptCoordinates,
    consent_revision: [u8; 32],
    delivery_revision: [u8; 32],
    routing: NostrRoutingV1,
    app_room: [u8; 32],
}

impl StatusNegotiation {
    pub(crate) fn receipt_coordinates(&self) -> &ReceiptCoordinates {
        &self.receipt
    }

    pub(crate) fn consent_revision(&self) -> [u8; 32] {
        self.consent_revision
    }

    pub(crate) fn delivery_revision(&self) -> [u8; 32] {
        self.delivery_revision
    }

    pub(crate) fn routing(&self) -> &NostrRoutingV1 {
        &self.routing
    }

    pub(crate) fn app_room(&self) -> [u8; 32] {
        self.app_room
    }
}

fn status_app_room(receipt: &ReceiptCoordinates) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"marmish/peer/status-room/v1\0");
    digest.update(receipt.founder.to_bytes());
    digest.update(receipt.recipient.to_bytes());
    digest.update(receipt.room_id);
    digest.update(receipt.offer_id);
    digest.finalize().into()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferWire {
    v: u8,
    expires_at: u64,
    offer_id: String,
    room_id: String,
    transport_group_id: String,
    relays: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConsentWire {
    v: u8,
    expires_at: u64,
    offer_id: String,
    offer_revision: String,
    package_event_id: String,
    package_ref: String,
    package_slot: String,
    package: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeliveryWire {
    v: u8,
    expires_at: u64,
    offer_id: String,
    offer_revision: String,
    consent_revision: String,
    package_event_id: String,
    package_ref: String,
    package_slot: String,
    welcome_event_id: String,
    welcome_sha256: String,
    welcome: String,
}

impl Codec {
    pub(crate) fn new(outer_cap: usize) -> Result<Self> {
        Self::for_version(outer_cap, ControlVersion::V1)
    }

    pub(crate) fn for_version(outer_cap: usize, version: ControlVersion) -> Result<Self> {
        if !(1..=MAX_OUTER_BYTES).contains(&outer_cap) {
            return Err(Error::Size);
        }
        Ok(Self { outer_cap, version })
    }

    pub(crate) fn prepare_offer(
        &self,
        signer: &Keys,
        client: PublicKey,
        room_id: [u8; 16],
        routing: &NostrRoutingV1,
        now: u64,
    ) -> Result<Prepared<Offer>> {
        let mut offer_id = [0; 16];
        getrandom::fill(&mut offer_id).map_err(|_| Error::Random)?;
        let routing = validate_routing(routing.nostr_group_id, &routing.relays)?;
        let expires_at = deadline(now, FLIGHT_TTL)?;
        let wire = OfferWire {
            v: self.version as u8,
            expires_at,
            offer_id: hex::encode(offer_id),
            room_id: hex::encode(room_id),
            transport_group_id: hex::encode(routing.nostr_group_id),
            relays: routing.relays.clone(),
        };
        let rumor = create_rumor(OFFER, signer, client, now, &wire)?;
        let (frame, flight) = self.prepare(signer, client, &rumor, expires_at)?;
        Ok(Prepared {
            frame,
            flight: Offer {
                version: self.version,
                flight,
                offer_id,
                room_id,
                routing,
            },
        })
    }

    pub(crate) fn open_offer(
        &self,
        local: &Keys,
        frame: &[u8],
        expected_server: Option<PublicKey>,
        configured_relays: &[String],
        now: u64,
    ) -> Result<Offer> {
        let (carrier_id, rumor) = self.open(local, frame, expected_server, OFFER)?;
        self.qualify_offer(
            carrier_id,
            local.public_key(),
            &rumor,
            configured_relays,
            now,
        )
    }

    fn qualify_offer(
        &self,
        carrier_id: EventId,
        recipient: PublicKey,
        rumor: &UnsignedEvent,
        configured_relays: &[String],
        now: u64,
    ) -> Result<Offer> {
        let wire: OfferWire = serde_json::from_str(&rumor.content).map_err(|_| Error::Body)?;
        check_body(self.version, wire.v, &rumor.content, &wire)?;
        let flight = flight(carrier_id, rumor, recipient, wire.expires_at, now)?;
        if wire.expires_at != deadline(flight.created_at, FLIGHT_TTL)? {
            return Err(Error::Time);
        }
        let routing = validate_routing(
            fixed_hex(&wire.transport_group_id, Error::Body)?,
            &wire.relays,
        )?;
        validate_relays(configured_relays)?;
        if routing.relays != configured_relays {
            return Err(Error::Route);
        }
        Ok(Offer {
            version: self.version,
            flight,
            offer_id: fixed_hex(&wire.offer_id, Error::Body)?,
            room_id: fixed_hex(&wire.room_id, Error::Body)?,
            routing,
        })
    }

    pub(crate) fn prepare_consent(
        &self,
        signer: &Keys,
        offer: &Offer,
        exact_package: &[u8],
        now: u64,
        expires_at: u64,
    ) -> Result<Prepared<Consent>> {
        if self.version != offer.version() || signer.public_key() != offer.flight.recipient {
            return Err(Error::Binding);
        }
        let created_at = child_time(offer.flight, now)?;
        let native = artifact::verify_package(
            exact_package,
            signer.public_key(),
            created_at,
            &offer.routing.relays,
        )?;
        consent_times(offer.flight, native.lifetime(), created_at, expires_at, now)?;
        let wire = ConsentWire {
            v: self.version as u8,
            expires_at,
            offer_id: hex::encode(offer.offer_id),
            offer_revision: offer.flight.revision.to_hex(),
            package_event_id: hex::encode(native.binding.event_id),
            package_ref: hex::encode(native.binding.reference),
            package_slot: hex::encode(native.binding.slot),
            package: STANDARD.encode(exact_package),
        };
        let server = offer.flight.author;
        let rumor = create_rumor(CONSENT, signer, server, created_at, &wire)?;
        let (frame, flight) = self.prepare(signer, server, &rumor, expires_at)?;
        Ok(Prepared {
            frame,
            flight: Consent {
                flight,
                offer: offer.clone(),
                native,
                package: Zeroizing::new(exact_package.to_vec()),
            },
        })
    }

    pub(crate) fn open_consent(
        &self,
        local: &Keys,
        frame: &[u8],
        offer: &Offer,
        now: u64,
    ) -> Result<Consent> {
        if self.version != offer.version() || local.public_key() != offer.flight.author {
            return Err(Error::Binding);
        }
        let (carrier_id, rumor) = self.open(local, frame, Some(offer.flight.recipient), CONSENT)?;
        let wire: ConsentWire = serde_json::from_str(&rumor.content).map_err(|_| Error::Body)?;
        check_body(self.version, wire.v, &rumor.content, &wire)?;
        match_offer(offer, &wire.offer_id, &wire.offer_revision)?;
        let flight = flight(carrier_id, &rumor, local.public_key(), wire.expires_at, now)?;
        stage_window(offer.flight, flight.created_at)?;
        let claimed = package_binding(
            &wire.package_event_id,
            &wire.package_ref,
            &wire.package_slot,
        )?;
        let package = artifact::decode(&wire.package, Error::Package)?;
        let native = artifact::verify_package(
            &package,
            rumor.pubkey,
            flight.created_at,
            &offer.routing.relays,
        )?;
        if claimed != native.binding {
            return Err(Error::Binding);
        }
        consent_times(
            offer.flight,
            native.lifetime(),
            flight.created_at,
            wire.expires_at,
            now,
        )?;
        Ok(Consent {
            flight,
            offer: offer.clone(),
            native,
            package,
        })
    }

    pub(crate) fn prepare_delivery(
        &self,
        signer: &Keys,
        consent: &Consent,
        exact_welcome: &[u8],
        now: u64,
        expires_at: u64,
    ) -> Result<Prepared<Delivery>> {
        if self.version != consent.offer.version()
            || signer.public_key() != consent.flight.recipient
        {
            return Err(Error::Binding);
        }
        let created_at = child_time(consent.flight, now)?;
        delivery_times(
            consent.flight.created_at..consent.flight.expires_at,
            consent.native.lifetime(),
            created_at,
            expires_at,
            now,
        )?;
        let client = consent.flight.author;
        let (welcome_id, welcome_digest) = artifact::verify_welcome(exact_welcome, client)?;
        let package = consent.native.binding;
        let wire = DeliveryWire {
            v: self.version as u8,
            expires_at,
            offer_id: hex::encode(consent.offer.offer_id),
            offer_revision: consent.offer.flight.revision.to_hex(),
            consent_revision: consent.flight.revision.to_hex(),
            package_event_id: hex::encode(package.event_id),
            package_ref: hex::encode(package.reference),
            package_slot: hex::encode(package.slot),
            welcome_event_id: welcome_id.to_hex(),
            welcome_sha256: hex::encode(welcome_digest),
            welcome: STANDARD.encode(exact_welcome),
        };
        let rumor = create_rumor(DELIVERY, signer, client, created_at, &wire)?;
        let (frame, flight) = self.prepare(signer, client, &rumor, expires_at)?;
        Ok(Prepared {
            frame,
            flight: Delivery {
                flight,
                offer: consent.offer.clone(),
                consent_revision: consent.flight.revision,
                package,
                welcome: Zeroizing::new(exact_welcome.to_vec()),
                welcome_id,
                welcome_digest,
            },
        })
    }

    pub(crate) fn open_delivery(
        &self,
        local: &Keys,
        frame: &[u8],
        consent: &Consent,
        now: u64,
    ) -> Result<Delivery> {
        if self.version != consent.offer.version() || local.public_key() != consent.flight.author {
            return Err(Error::Binding);
        }
        let (carrier_id, rumor) =
            self.open(local, frame, Some(consent.flight.recipient), DELIVERY)?;
        let wire = serde_json::from_str(&rumor.content).map_err(|_| Error::Body)?;
        Self::bind_delivery(
            carrier_id,
            &rumor,
            wire,
            DeliveryAuthority {
                offer: &consent.offer,
                revision: consent.flight.revision,
                window: consent.flight.created_at..consent.flight.expires_at,
                package: consent.native.binding,
                lifetime: consent.native.lifetime(),
            },
            now,
        )
    }

    fn bind_delivery(
        carrier_id: EventId,
        rumor: &UnsignedEvent,
        wire: DeliveryWire,
        authority: DeliveryAuthority<'_>,
        now: u64,
    ) -> Result<Delivery> {
        check_body(authority.offer.version(), wire.v, &rumor.content, &wire)?;
        match_offer(authority.offer, &wire.offer_id, &wire.offer_revision)?;
        let consent_revision = event_id(&wire.consent_revision)?;
        let package = package_binding(
            &wire.package_event_id,
            &wire.package_ref,
            &wire.package_slot,
        )?;
        if rumor.pubkey != authority.offer.flight.author
            || consent_revision != authority.revision
            || package != authority.package
        {
            return Err(Error::Binding);
        }
        let client = authority.offer.flight.recipient;
        let flight = flight(carrier_id, rumor, client, wire.expires_at, now)?;
        delivery_times(
            authority.window,
            authority.lifetime,
            flight.created_at,
            flight.expires_at,
            now,
        )?;
        let welcome = artifact::decode(&wire.welcome, Error::Welcome)?;
        let (welcome_id, welcome_digest) = artifact::verify_welcome(&welcome, client)?;
        if welcome_id != event_id(&wire.welcome_event_id)?
            || welcome_digest != fixed_hex(&wire.welcome_sha256, Error::Body)?
        {
            return Err(Error::Welcome);
        }
        Ok(Delivery {
            flight,
            offer: authority.offer.clone(),
            consent_revision,
            package,
            welcome,
            welcome_id,
            welcome_digest,
        })
    }

    fn prepare(
        &self,
        signer: &Keys,
        recipient: PublicKey,
        rumor: &UnsignedEvent,
        expires_at: u64,
    ) -> Result<(Vec<u8>, Flight)> {
        let outer_signer = Keys::generate();
        let choices = Cover {
            seal_offset: uniform(OFFSET_WINDOW)?,
            outer_offset: uniform(OFFSET_WINDOW)?,
            retention_buckets: 1 + uniform(2)?,
        };
        self.prepare_with_cover(signer, &outer_signer, recipient, rumor, expires_at, choices)
    }

    fn prepare_with_cover(
        &self,
        signer: &Keys,
        outer_signer: &Keys,
        recipient: PublicKey,
        rumor: &UnsignedEvent,
        expires_at: u64,
        choices: Cover,
    ) -> Result<(Vec<u8>, Flight)> {
        if [signer.public_key(), recipient].contains(&outer_signer.public_key()) {
            return Err(Error::Random);
        }
        let created_at = rumor.created_at.as_secs();
        let seal = seal::sign_seal(
            signer,
            recipient,
            rumor,
            created_at.saturating_sub(choices.seal_offset),
        )?;
        let housekeeping = retention(expires_at, choices.retention_buckets)?;
        let content = nip44::encrypt(outer_signer.secret_key(), &recipient, &*seal, Version::V2)
            .map_err(|_| Error::Outer)?;
        if content.len() > OUTER_CONTENT_CAP {
            return Err(Error::Size);
        }
        let expiry_tag = Tag::parse(["expiration".to_owned(), housekeeping.to_string()])
            .map_err(|_| Error::Outer)?;
        let outer = EventBuilder::new(OUTER, content)
            .tags([Tag::public_key(recipient), expiry_tag])
            .custom_created_at(Timestamp::from_secs(
                created_at.saturating_sub(choices.outer_offset),
            ))
            .finalize(outer_signer)
            .map_err(|_| Error::Outer)?;
        let frame = serde_json::to_vec(&outer).map_err(|_| Error::Outer)?;
        if frame.len() > self.outer_cap {
            return Err(Error::Size);
        }
        Ok((
            frame,
            flight(outer.id, rumor, recipient, expires_at, created_at)?,
        ))
    }

    fn open(
        &self,
        local: &Keys,
        frame: &[u8],
        expected_author: Option<PublicKey>,
        expected_kind: Kind,
    ) -> Result<(EventId, UnsignedEvent)> {
        let (carrier, _, rumor) = self.open_authenticated(local, frame, expected_author)?;
        if rumor.kind != expected_kind {
            return Err(Error::Rumor);
        }
        Ok((carrier, rumor))
    }

    fn open_authenticated(
        &self,
        local: &Keys,
        frame: &[u8],
        expected_author: Option<PublicKey>,
    ) -> Result<(EventId, u64, UnsignedEvent)> {
        if frame.len() > self.outer_cap {
            return Err(Error::Size);
        }
        let outer = seal::verify_signed(frame, Error::Outer)?;
        Self::open_outer(local, outer, expected_author)
    }

    fn open_outer(
        local: &Keys,
        outer: Event,
        expected_author: Option<PublicKey>,
    ) -> Result<(EventId, u64, UnsignedEvent)> {
        if outer.content.len() > OUTER_CONTENT_CAP {
            return Err(Error::Size);
        }
        let recipient = local.public_key();
        let mut tags = outer.tags.iter();
        if outer.kind != OUTER
            || outer.pubkey == recipient
            || outer.tags.len() != 2
            || !tags
                .next()
                .is_some_and(|tag| is_recipient_tag(tag, recipient))
            || !tags.next().is_some_and(valid_retention_tag)
        {
            return Err(Error::Outer);
        }
        let rumor = seal::open_signed_seal(&outer, local, expected_author)?;
        if !is_exact_recipient(&rumor.tags, recipient) {
            return Err(Error::Rumor);
        }
        Ok((outer.id, outer.created_at.as_secs(), rumor))
    }
}

fn check_body(
    version: ControlVersion,
    wire_version: u8,
    content: &str,
    body: &impl Serialize,
) -> Result<()> {
    if wire_version != version as u8
        || (version == ControlVersion::V2
            && serde_json::to_string(body).map_err(|_| Error::Body)? != content)
    {
        return Err(Error::Body);
    }
    Ok(())
}

fn create_rumor<T: Serialize>(
    kind: Kind,
    signer: &Keys,
    recipient: PublicKey,
    created_at: u64,
    body: &T,
) -> Result<UnsignedEvent> {
    if signer.public_key() == recipient || recipient.xonly().is_err() {
        return Err(Error::Binding);
    }
    let content = serde_json::to_string(body).map_err(|_| Error::Body)?;
    if content.len() > BODY_CAP {
        return Err(Error::Size);
    }
    let mut rumor = UnsignedEvent::new(
        signer.public_key(),
        Timestamp::from_secs(created_at),
        kind,
        [Tag::public_key(recipient)],
        content,
    );
    rumor.ensure_id();
    Ok(rumor)
}

fn event_id(value: &str) -> Result<EventId> {
    Ok(EventId::from_byte_array(fixed_hex(value, Error::Body)?))
}

fn match_offer(offer: &Offer, id: &str, revision: &str) -> Result<()> {
    if fixed_hex::<16>(id, Error::Body)? != offer.offer_id
        || event_id(revision)? != offer.flight.revision
    {
        return Err(Error::Binding);
    }
    Ok(())
}

fn package_binding(id: &str, reference: &str, slot: &str) -> Result<PackageBinding> {
    Ok(PackageBinding {
        event_id: fixed_hex(id, Error::Body)?,
        reference: fixed_hex(reference, Error::Body)?,
        slot: fixed_hex(slot, Error::Body)?,
    })
}

fn validate_relays(relays: &[String]) -> Result<()> {
    if !(1..=NOSTR_ROUTING_MAX_RELAYS).contains(&relays.len())
        || relays.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::Route);
    }
    for value in relays {
        cgka_traits::app_components::validate_nostr_relay_url(value).map_err(|_| Error::Route)?;
        let url = url::Url::parse(value).map_err(|_| Error::Route)?;
        if url.as_str() != value || url.query().is_some() {
            return Err(Error::Route);
        }
    }
    Ok(())
}

fn validate_routing(group: [u8; 32], relays: &[String]) -> Result<NostrRoutingV1> {
    validate_relays(relays)?;
    NostrRoutingV1::new(group, relays.to_vec()).map_err(|_| Error::Route)
}

fn deadline(start: u64, delta: u64) -> Result<u64> {
    start.checked_add(delta).ok_or(Error::Time)
}

fn flight(
    carrier_id: EventId,
    rumor: &UnsignedEvent,
    recipient: PublicKey,
    expires_at: u64,
    now: u64,
) -> Result<Flight> {
    let created_at = rumor.created_at.as_secs();
    if created_at >= expires_at
        || created_at > deadline(now, SKEW)?
        || now >= deadline(expires_at, SKEW)?
    {
        return Err(Error::Time);
    }
    Ok(Flight {
        carrier_id,
        revision: rumor.id.ok_or(Error::Rumor)?,
        author: rumor.pubkey,
        recipient,
        created_at,
        expires_at,
        replay_safe_until: deadline(expires_at, SKEW + REPLAY_MARGIN)?,
    })
}

fn stage_window(parent: Flight, created_at: u64) -> Result<()> {
    if !(parent.created_at..parent.expires_at).contains(&created_at) {
        return Err(Error::Time);
    }
    Ok(())
}

fn child_time(parent: Flight, now: u64) -> Result<u64> {
    let created_at = now.max(parent.created_at);
    if created_at > deadline(now, SKEW)? {
        return Err(Error::Time);
    }
    stage_window(parent, created_at)?;
    Ok(created_at)
}

fn package_window(
    lifetime: std::ops::Range<u64>,
    created_at: u64,
    expires_at: u64,
    now: u64,
) -> Result<()> {
    if !lifetime.contains(&created_at) || !lifetime.contains(&now) || expires_at > lifetime.end {
        return Err(Error::PackageLifetime);
    }
    Ok(())
}

fn consent_times(
    offer: Flight,
    lifetime: std::ops::Range<u64>,
    created_at: u64,
    expires_at: u64,
    now: u64,
) -> Result<()> {
    stage_window(offer, created_at)?;
    if expires_at <= created_at || expires_at > deadline(created_at, FLIGHT_TTL)? {
        return Err(Error::Time);
    }
    package_window(lifetime, created_at, expires_at, now)
}

struct DeliveryAuthority<'a> {
    offer: &'a Offer,
    revision: EventId,
    window: std::ops::Range<u64>,
    package: PackageBinding,
    lifetime: std::ops::Range<u64>,
}

fn delivery_times(
    window: std::ops::Range<u64>,
    lifetime: std::ops::Range<u64>,
    created_at: u64,
    expires_at: u64,
    now: u64,
) -> Result<()> {
    if !window.contains(&created_at)
        || expires_at <= created_at
        || expires_at > window.end
        || expires_at > deadline(created_at, FLIGHT_TTL)?
    {
        return Err(Error::Time);
    }
    package_window(lifetime, created_at, expires_at, now)
}

#[derive(Clone, Copy)]
struct Cover {
    seal_offset: u64,
    outer_offset: u64,
    retention_buckets: u64,
}

fn uniform(range: u64) -> Result<u64> {
    let reject_below = range.wrapping_neg() % range;
    loop {
        let sample = getrandom::u64().map_err(|_| Error::Random)?;
        if sample >= reject_below {
            return Ok(sample % range);
        }
    }
}

fn retention(expires_at: u64, buckets: u64) -> Result<u64> {
    let admit_until = deadline(expires_at, SKEW)?;
    let round_up = (RETENTION_BUCKET - (admit_until % RETENTION_BUCKET)) % RETENTION_BUCKET;
    deadline(deadline(admit_until, round_up)?, buckets * RETENTION_BUCKET)
}

fn valid_retention_tag(tag: &Tag) -> bool {
    let [name, encoded] = tag.as_slice() else {
        return false;
    };
    name == "expiration"
        && encoded.parse::<u64>().is_ok_and(|time| {
            time >= RETENTION_BUCKET && time % RETENTION_BUCKET == 0 && time.to_string() == *encoded
        })
}

#[cfg(test)]
#[path = "peer_control_wire_tests.rs"]
mod tests;
#[cfg(test)]
pub(crate) use tests::replace_offer_relays;
