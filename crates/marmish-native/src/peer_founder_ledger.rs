//! The private proof stores one direction and one irreversible founder turn.

use super::*;
use crate::peer_control_version::ControlVersion;
use crate::peer_quota_vfs::dispatch::founder::Image;
use cgka_traits::{EpochId, EpochPin, GroupId, MessageId};
use nostr::{event::Event, key::PublicKey};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::{num::NonZeroU64, path::Path};
use store::consent_checkpoint::wire::{
    Consent, Delivery, Flight, MAX_OUTER_BYTES, Offer, Prepared,
};

const SCHEMA: &str = "
CREATE TABLE turn (
    slot INTEGER PRIMARY KEY CHECK(slot=1),
    scope BLOB NOT NULL CHECK(length(scope)=32),
    generation INTEGER NOT NULL CHECK(generation>0),
    step INTEGER NOT NULL CHECK(step BETWEEN 1 AND 12),
    reserved_at INTEGER,
    device INTEGER,
    inode INTEGER,
    native BLOB
) STRICT;
CREATE TABLE frames (
    phase INTEGER PRIMARY KEY CHECK(phase BETWEEN 1 AND 3),
    carrier BLOB NOT NULL CHECK(length(carrier)=32),
    revision BLOB NOT NULL CHECK(length(revision)=32),
    digest BLOB NOT NULL CHECK(length(digest)=32),
    frame BLOB NOT NULL CHECK(length(frame) BETWEEN 1 AND 65536),
    expires INTEGER NOT NULL CHECK(expires>0)
) STRICT;";

pub(super) type FounderResult<T> = std::result::Result<T, &'static str>;
type Result<T> = FounderResult<T>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i64)]
pub(super) enum Step {
    Offer = 1,
    OfferArmed,
    OfferAccepted,
    Consent,
    Reserved,
    Started,
    Delivery,
    DeliveryArmed,
    DeliveryAccepted,
    NotSent,
    Rejected,
    Uncertain,
}

#[derive(Clone, Copy)]
pub(super) enum Outbound {
    Offer = 1,
    Delivery = 3,
}

impl Outbound {
    pub(super) fn armed(self) -> Step {
        match self {
            Self::Offer => Step::OfferArmed,
            Self::Delivery => Step::DeliveryArmed,
        }
    }

    pub(super) fn accepted(self) -> Step {
        match self {
            Self::Offer => Step::OfferAccepted,
            Self::Delivery => Step::DeliveryAccepted,
        }
    }

    fn ready(self) -> Step {
        match self {
            Self::Offer => Step::Offer,
            Self::Delivery => Step::Delivery,
        }
    }
}

pub(super) struct Ledger {
    db: Connection,
    binding: [u8; 32],
    generation: i64,
    live: bool,
    file: paths::CheckedFile,
}

pub(crate) struct Started {
    generation: NonZeroU64,
    file: (u64, u64),
}

impl Started {
    pub(crate) fn coordinates(self) -> (NonZeroU64, (u64, u64)) {
        (self.generation, self.file)
    }
}

#[derive(PartialEq)]
struct Frame<'a> {
    carrier: [u8; 32],
    revision: [u8; 32],
    digest: [u8; 32],
    frame: &'a [u8],
    expires: i64,
}

impl<'a> Frame<'a> {
    fn checked(flight: Flight, bytes: &'a [u8]) -> Result<Self> {
        let raw: &serde_json::value::RawValue =
            serde_json::from_slice(bytes).map_err(|_| "founder_frame")?;
        let event: Event = serde_json::from_slice(bytes).map_err(|_| "founder_frame")?;
        if bytes.len() > MAX_OUTER_BYTES
            || raw.get().as_bytes() != bytes
            || event.id != flight.carrier_id
            || event.verify().is_err()
        {
            return Err("founder_frame");
        }
        Ok(Self {
            carrier: flight.carrier_id.to_bytes(),
            revision: flight.revision.to_bytes(),
            digest: Sha256::digest(bytes).into(),
            frame: bytes,
            expires: integer(flight.expires_at)?,
        })
    }
}

impl Ledger {
    pub(super) fn open(
        path: &Path,
        key: [u8; 32],
        binding: [u8; 32],
        generation: NonZeroU64,
        new: bool,
    ) -> Result<Self> {
        let file = Paths::check_founder_ledger(path).map_err(|_| "founder_ledger_path")?;
        let db = HardenedPeerConnection::open(path, key)
            .map_err(|_| "founder_ledger_open")?
            .into_founder_ledger();
        file.unchanged().map_err(|_| "founder_ledger_path")?;
        if new {
            db.execute_batch(&format!("BEGIN IMMEDIATE; {SCHEMA} COMMIT;"))
                .map_err(|_| "founder_ledger_schema")?;
        }
        // Compare the persisted SQL text itself, including column constraints.
        let sql = db
            .prepare("SELECT sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%' ORDER BY name")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|_| "founder_ledger_schema")?;
        let mut expected = SCHEMA
            .trim()
            .split(';')
            .map(str::trim)
            .filter(|statement| !statement.is_empty())
            .collect::<Vec<_>>();
        expected.reverse();
        if sql != expected {
            return Err("founder_ledger_schema");
        }
        let ledger = Self {
            db,
            binding,
            generation: integer(generation.get())?,
            live: new,
            file,
        };
        if !new {
            ledger
                .db
                .execute_batch("PRAGMA query_only=ON;")
                .map_err(|_| "founder_history")?;
            ledger.verify_history()?;
        }
        Ok(ledger)
    }

    pub(super) fn claim(&mut self, offer: &Prepared<Offer>) -> Result<()> {
        self.require_live()?;
        if offer.flight.version() != ControlVersion::V2 {
            return Err("founder_version");
        }
        let frame = Frame::checked(offer.flight.flight(), offer.frame())?;
        let tx = self.db.transaction().map_err(|_| "founder_ledger_write")?;
        tx.execute(
            "INSERT INTO turn(slot,scope,generation,step) VALUES(1,?1,?2,?3)",
            params![&self.binding[..], self.generation, Step::Offer as i64],
        )
        .map_err(|_| "founder_ledger_claim")?;
        put_frame(&tx, Outbound::Offer as i64, &frame)?;
        tx.commit().map_err(|_| "founder_ledger_write")
    }

    pub(super) fn arm(
        &mut self,
        which: Outbound,
        flight: Flight,
        bytes: &[u8],
        wall: u64,
    ) -> Result<()> {
        self.require_live()?;
        if !self.current(which, which.ready(), flight, bytes, wall) {
            return Err("founder_arm");
        }
        transition(
            &self.db,
            self.binding,
            self.generation,
            which.ready(),
            which.armed(),
        )
    }

    pub(super) fn current(
        &self,
        which: Outbound,
        step: Step,
        flight: Flight,
        bytes: &[u8],
        wall: u64,
    ) -> bool {
        wall < flight.expires_at
            && self.file.unchanged().is_ok()
            && self.at(step)
            && Frame::checked(flight, bytes)
                .is_ok_and(|exact| has_frame(&self.db, which as i64, &exact))
    }

    pub(super) fn terminal(&mut self, which: Outbound, step: Step) -> Result<()> {
        self.require_live()?;
        if !matches!(step, Step::NotSent | Step::Rejected | Step::Uncertain)
            && step != which.accepted()
        {
            return Err("founder_terminal");
        }
        transition(&self.db, self.binding, self.generation, which.armed(), step)
    }

    pub(super) fn receive_consent(&mut self, consent: &Consent, bytes: &[u8]) -> Result<()> {
        self.require_live()?;
        let frame = Frame::checked(consent.flight(), bytes)?;
        let tx = self.db.transaction().map_err(|_| "founder_ledger_write")?;
        if consent.offer().version() != ControlVersion::V2
            || !has_flight(&tx, Outbound::Offer as i64, consent.offer().flight())
        {
            return Err("founder_ancestry");
        }
        if has_frame(&tx, 2, &frame) {
            return Ok(());
        }
        transition(
            &tx,
            self.binding,
            self.generation,
            Step::OfferAccepted,
            Step::Consent,
        )?;
        put_frame(&tx, 2, &frame)?;
        tx.commit().map_err(|_| "founder_ledger_write")
    }

    pub(super) fn consent_matches(&self, consent: &Consent) -> bool {
        self.file.unchanged().is_ok() && has_flight(&self.db, 2, consent.flight())
    }

    pub(super) fn reserve(&mut self, wall: u64) -> Result<()> {
        self.require_live()?;
        change_one(self.db.execute(
            "UPDATE turn SET step=?1,reserved_at=?2 WHERE slot=1 AND scope=?3 AND generation=?4 AND step=?5",
            params![Step::Reserved as i64, integer(wall)?, self.binding, self.generation, Step::Consent as i64]
        ))
    }

    pub(super) fn start(&mut self, file: (u64, u64)) -> Result<Started> {
        self.require_live()?;
        change_one(self.db.execute(
            "UPDATE turn SET step=?1,device=?2,inode=?3 WHERE slot=1 AND scope=?4 AND generation=?5 AND step=?6",
            params![Step::Started as i64, integer(file.0)?, integer(file.1)?, self.binding, self.generation, Step::Reserved as i64]
        ))?;
        let generation = NonZeroU64::new(self.generation as u64).expect("validated generation");
        Ok(Started { generation, file })
    }

    pub(super) fn freeze(
        &mut self,
        delivery: &Prepared<Delivery>,
        image: &Image,
        room: [u8; 16],
        reference: &[u8],
    ) -> Result<()> {
        self.require_live()?;
        let frame = Frame::checked(delivery.flight.flight(), delivery.frame())?;
        let negotiation = delivery
            .flight
            .status_negotiation()
            .ok_or("founder_version")?;
        let group = image.group.as_slice();
        if group.is_empty()
            || group.len() > 64
            || reference != delivery.flight.binding().reference
            || room != delivery.flight.offer().room_id()
            || image.welcome_id.as_slice() != delivery.flight.welcome_id().to_bytes()
        {
            return Err("founder_group");
        }
        let mut native = Vec::new();
        native.push(group.len() as u8);
        native.extend_from_slice(group);
        native.extend_from_slice(&image.pin.epoch.0.to_be_bytes());
        native.extend_from_slice(&image.pin.identity);
        native.extend_from_slice(&image.founder_leaf.to_be_bytes());
        native.extend_from_slice(&image.recipient_leaf.to_be_bytes());
        native.extend_from_slice(&delivery.flight.welcome_id().to_bytes());
        native.extend_from_slice(&delivery.flight.welcome_digest());
        native.extend_from_slice(&negotiation.app_room());
        native.extend_from_slice(&negotiation.routing().nostr_group_id);
        native.extend_from_slice(&negotiation.receipt_coordinates().offer_revision);
        native.extend_from_slice(&negotiation.consent_revision());
        native.extend_from_slice(&negotiation.delivery_revision());
        native.extend_from_slice(&room);
        native.extend_from_slice(reference);
        let tx = self.db.transaction().map_err(|_| "founder_ledger_write")?;
        if !has_flight(
            &tx,
            Outbound::Offer as i64,
            delivery.flight.offer().flight(),
        ) || !has_revision(&tx, 2, delivery.flight.consent_revision().to_bytes())
        {
            return Err("founder_ancestry");
        }
        change_one(tx.execute(
            "UPDATE turn SET step=?1,native=?2 WHERE slot=1 AND scope=?3 AND generation=?4 AND step=?5",
            params![Step::Delivery as i64, native, self.binding, self.generation, Step::Started as i64]
        ))?;
        put_frame(&tx, Outbound::Delivery as i64, &frame)?;
        tx.commit().map_err(|_| "founder_ledger_write")
    }

    pub(super) fn owns(&self, binding: [u8; 32], generation: NonZeroU64) -> bool {
        self.live && self.binding == binding && integer(generation.get()) == Ok(self.generation)
    }

    fn require_live(&self) -> Result<()> {
        if self.live {
            self.file.unchanged().map_err(|_| "founder_ledger_path")
        } else {
            Err("founder_history_only")
        }
    }

    fn verify_history(&self) -> Result<()> {
        let (step, reserved, device, inode, native) = self.db.query_row(
            "SELECT step,reserved_at,device,inode,native FROM turn WHERE slot=1 AND scope=?1 AND generation=?2",
            params![self.binding, self.generation],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<i64>>(2)?, row.get::<_, Option<i64>>(3)?,
                row.get::<_, Option<Vec<u8>>>(4)?)),
        ).map_err(|_| "founder_history")?;
        let mut stmt = self
            .db
            .prepare("SELECT phase,carrier,digest,frame FROM frames ORDER BY phase")
            .map_err(|_| "founder_history")?;
        let count = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                ))
            })
            .map_err(|_| "founder_history")?
            .enumerate()
            .try_fold(0, |_, (index, row)| {
                let (phase, carrier, digest, bytes) = row.map_err(|_| "founder_history")?;
                let event: Event = serde_json::from_slice(&bytes).map_err(|_| "founder_history")?;
                let raw: &serde_json::value::RawValue =
                    serde_json::from_slice(&bytes).map_err(|_| "founder_history")?;
                if phase != index as i64 + 1
                    || bytes.len() > MAX_OUTER_BYTES
                    || raw.get().as_bytes() != bytes
                    || event.id.to_bytes().as_slice() != carrier
                    || Sha256::digest(&bytes).as_slice() != digest
                    || event.verify().is_err()
                {
                    return Err("founder_history");
                }
                Ok(phase)
            })?;
        let terminal = step >= Step::NotSent as i64;
        let expected_count = if terminal {
            if native.is_some() { 3 } else { 1 }
        } else if step < Step::Consent as i64 {
            1
        } else if step < Step::Delivery as i64 {
            2
        } else {
            3
        };
        let reserved_expected = if terminal {
            count == 3
        } else {
            step >= Step::Reserved as i64
        };
        let inode_expected = if terminal {
            count == 3
        } else {
            step >= Step::Started as i64
        };
        if count != expected_count
            || reserved.is_some_and(|value| value <= 0)
            || reserved.is_some() != reserved_expected
            || device.is_some() != inode_expected
            || inode.is_some_and(|value| value <= 0)
            || inode.is_some() != inode_expected
            || native.is_some() != (count == 3)
            || native
                .as_ref()
                .is_some_and(|field| checked_image_length(field).is_none())
        {
            return Err("founder_history");
        }
        Ok(())
    }

    pub(super) fn historical_image(&self) -> Result<Option<HistoricalImage>> {
        if self.live {
            return Err("founder_history");
        }
        let image: Option<Vec<u8>> = self
            .db
            .query_row(
                "SELECT native FROM turn WHERE slot=1 AND scope=?1 AND generation=?2",
                params![self.binding, self.generation],
                |row| row.get(0),
            )
            .map_err(|_| "founder_history")?;
        image
            .map(|bytes| {
                // verify_history already established exact canonical fixed widths.
                let group_length = checked_image_length(&bytes).ok_or("founder_history")?;
                let (group, mut fields) = bytes[1..].split_at(group_length);
                let epoch = u64::from_be_bytes(take(&mut fields));
                let identity = take(&mut fields);
                let founder_leaf = u32::from_be_bytes(take(&mut fields));
                let recipient_leaf = u32::from_be_bytes(take(&mut fields));
                let welcome = take::<32>(&mut fields);
                let _welcome_digest = take::<32>(&mut fields);
                let _app_room = take::<32>(&mut fields);
                let routing_id = take(&mut fields);
                let offer = take::<32>(&mut fields);
                let consent = take::<32>(&mut fields);
                let delivery = take::<32>(&mut fields);
                let room = take(&mut fields);
                let reference = take(&mut fields);
                let file = self.persisted_file()?.ok_or("founder_history")?;
                if !fields.is_empty()
                    || epoch != 1
                    || founder_leaf == recipient_leaf
                    || !has_revision(&self.db, 1, offer)
                    || !has_revision(&self.db, 2, consent)
                    || !has_revision(&self.db, 3, delivery)
                {
                    return Err("founder_history");
                }
                Ok(HistoricalImage {
                    image: Image {
                        group: GroupId::new(group),
                        pin: EpochPin {
                            epoch: EpochId(epoch),
                            identity,
                        },
                        founder_leaf,
                        recipient_leaf,
                        welcome_id: MessageId::new(welcome),
                    },
                    file,
                    routing_id,
                    room,
                    reference,
                })
            })
            .transpose()
    }

    pub(super) fn at(&self, step: Step) -> bool {
        self.db
            .query_row(
                "SELECT 1 FROM turn WHERE slot=1 AND scope=?1 AND generation=?2 AND step=?3",
                params![self.binding, self.generation, step as i64],
                |_| Ok(()),
            )
            .is_ok()
    }

    pub(super) fn persisted_file(&self) -> Result<Option<(u64, u64)>> {
        self.db.query_row(
            "SELECT device,inode FROM turn WHERE slot=1 AND scope=?1 AND generation=?2 AND inode IS NOT NULL",
            params![self.binding, self.generation],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
        ).optional().map_err(|_| "founder_ledger_read")?
            .map(|(device, inode)| Ok((u64::try_from(device).map_err(|_| "founder_inode")?, u64::try_from(inode).map_err(|_| "founder_inode")?)))
            .transpose()
    }
}

pub(super) struct HistoricalImage {
    pub(super) image: Image,
    pub(super) file: (u64, u64),
    pub(super) routing_id: [u8; 32],
    pub(super) room: [u8; 16],
    pub(super) reference: [u8; 32],
}

fn checked_image_length(bytes: &[u8]) -> Option<usize> {
    let group = usize::from(*bytes.first()?);
    ((1..=64).contains(&group) && bytes.len() == 1 + group + 8 + 32 + 8 + 7 * 32 + 16 + 32)
        .then_some(group)
}

fn take<const N: usize>(bytes: &mut &[u8]) -> [u8; N] {
    let (field, rest) = bytes.split_at(N);
    *bytes = rest;
    field
        .try_into()
        .expect("verified fixed-width founder evidence")
}

pub(super) fn ledger_binding(founder: PublicKey, recipient: PublicKey, origin: &str) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"marmish/peer/founder-ledger-binding/1");
    hash.update(founder.to_bytes());
    hash.update(recipient.to_bytes());
    hash.update(origin.as_bytes());
    hash.finalize().into()
}

fn has_frame(db: &Connection, phase: i64, frame: &Frame<'_>) -> bool {
    db.query_row(
        "SELECT 1 FROM frames WHERE phase=?1 AND carrier=?2 AND revision=?3 AND digest=?4 AND frame=?5 AND expires=?6",
        params![phase, frame.carrier, frame.revision, frame.digest, frame.frame, frame.expires],
        |_| Ok(()),
    ).is_ok()
}

fn has_flight(db: &Connection, phase: i64, flight: Flight) -> bool {
    integer(flight.expires_at).is_ok_and(|expiry| {
        db.query_row(
            "SELECT 1 FROM frames WHERE phase=?1 AND carrier=?2 AND revision=?3 AND expires=?4",
            params![
                phase,
                flight.carrier_id.to_bytes(),
                flight.revision.to_bytes(),
                expiry
            ],
            |_| Ok(()),
        )
        .is_ok()
    })
}

fn has_revision(db: &Connection, phase: i64, revision: [u8; 32]) -> bool {
    db.query_row(
        "SELECT 1 FROM frames WHERE phase=?1 AND revision=?2",
        params![phase, revision],
        |_| Ok(()),
    )
    .is_ok()
}

fn put_frame(db: &Connection, phase: i64, frame: &Frame<'_>) -> Result<()> {
    change_one(db.execute(
        "INSERT INTO frames (phase,carrier,revision,digest,frame,expires) VALUES (?1,?2,?3,?4,?5,?6)",
        params![phase, &frame.carrier[..], &frame.revision[..], &frame.digest[..], frame.frame, frame.expires],
    ))
}

fn transition(
    db: &Connection,
    binding: [u8; 32],
    generation: i64,
    from: Step,
    to: Step,
) -> Result<()> {
    change_one(db.execute(
        "UPDATE turn SET step=?1 WHERE slot=1 AND scope=?2 AND generation=?3 AND step=?4",
        params![to as i64, binding, generation, from as i64],
    ))
}

fn change_one(result: rusqlite::Result<usize>) -> Result<()> {
    if result == Ok(1) {
        Ok(())
    } else {
        Err("founder_ledger_cas")
    }
}

fn integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| "founder_ledger_integer")
}
