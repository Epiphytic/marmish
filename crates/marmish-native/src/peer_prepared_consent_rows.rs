//! Exact encrypted rows, verified under the export owner transaction.

use rusqlite::{OptionalExtension, Transaction, params};
use zeroize::Zeroizing;

use super::super::super::super::{read_optional_seconds, read_seconds};
use super::super::super::{package_digest, wire::Transcript};
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Outbox {
    pub(super) transcript: Transcript,
    pub(super) frame: Zeroizing<Vec<u8>>,
    pub(super) digest: [u8; 32],
    pub(super) attempt: u64,
    pub(super) retry_at: Option<u64>,
    pub(super) ack_at: Option<u64>,
}

impl Outbox {
    pub(super) fn new(prepared: &Candidate, transcript: Transcript, frame: &[u8]) -> Self {
        debug_assert_eq!(
            prepared.account,
            prepared.offer.flight().recipient.to_bytes()
        );
        Self {
            transcript,
            frame: Zeroizing::new(frame.to_vec()),
            digest: package_digest(frame),
            attempt: 0,
            retry_at: None,
            ack_at: None,
        }
    }

    pub(super) fn insert(&self, tx: &Transaction<'_>, prepared: &Candidate) -> Result<()> {
        let binding = prepared.binding;
        let native = prepared.artifact;
        let flight = self.transcript;
        schema::Writer::Insert.guard(tx, false)?;
        rows::require_write(tx.execute(
            "INSERT INTO peer_prepared_consent_outbox
             (class,generation,account,server,offer_id,offer_revision,origin,package_id,
              package_digest,package_expires,carrier,frame,frame_digest,revision,created_at,expires,
              package_ref,package_slot)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params![
                binding.generation.known,
                seconds(binding.generation.count)?,
                prepared.account,
                binding.key.server.to_bytes(),
                binding.key.id,
                binding.key.revision,
                binding.origin,
                native.id,
                native.digest,
                seconds(native.expires)?,
                flight.carrier,
                self.frame.as_slice(),
                self.digest,
                flight.revision,
                seconds(flight.created_at)?,
                seconds(flight.expires)?,
                flight.package_ref,
                flight.package_slot,
            ],
        )?)?;
        schema::Writer::Insert.guard(tx, true)?;
        Ok(())
    }
}

pub(super) fn load(tx: &Transaction<'_>, prepared: &Candidate) -> Result<Option<Outbox>> {
    let binding = prepared.binding;
    let found = tx
        .query_row(
            "SELECT o.carrier,o.frame,o.frame_digest,o.revision,o.created_at,o.expires,
                o.package_ref,o.package_slot,o.attempt,o.retry_at,o.ack_at
         FROM peer_prepared_consent_outbox o
         JOIN peer_provisioning_slot p ON p.class=o.class AND p.generation=o.generation
            AND p.server=o.server AND p.offer_id=o.offer_id AND p.revision=o.offer_revision
            AND p.origin=o.origin
         JOIN peer_consent_invite i ON i.server=p.server AND i.offer_id=p.offer_id
            AND i.revision=p.revision AND i.origin=p.origin AND i.reserved=p.class
         JOIN peer_provisional_stage_v1 s ON s.class=p.class AND s.generation=p.generation
            AND s.server=p.server AND s.offer_id=p.offer_id AND s.revision=p.revision
            AND s.origin=p.origin AND s.state=2
            AND s.package_id=o.package_id AND s.package_digest=o.package_digest
            AND s.package_expires=o.package_expires
         WHERE o.class=?1 AND o.generation=?2 AND o.account=?3 AND o.server=?4
            AND o.offer_id=?5 AND o.offer_revision=?6 AND o.origin=?7 AND i.decision_at IS NULL",
            params![
                binding.generation.known,
                seconds(binding.generation.count)?,
                prepared.account,
                binding.key.server.to_bytes(),
                binding.key.id,
                binding.key.revision,
                binding.origin
            ],
            |row| {
                Ok(Outbox {
                    transcript: Transcript {
                        carrier: row.get(0)?,
                        revision: row.get(3)?,
                        created_at: read_seconds(row, 4)?,
                        expires: read_seconds(row, 5)?,
                        package_ref: row.get(6)?,
                        package_slot: row.get(7)?,
                    },
                    frame: Zeroizing::new(row.get(1)?),
                    digest: row.get(2)?,
                    attempt: read_seconds(row, 8)?,
                    retry_at: read_optional_seconds(row, 9)?,
                    ack_at: read_optional_seconds(row, 10)?,
                })
            },
        )
        .optional()?;
    if let Some(ref row) = found
        && (row.frame.is_empty()
            || row.frame.len() > super::super::super::MAX_OUTER_BYTES
            || package_digest(&row.frame) != row.digest)
    {
        return Err(CursorError::Binding.into());
    }
    Ok(found)
}

pub(super) fn exists(tx: &Transaction<'_>, key: Selector, origin: Origin) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM peer_prepared_consent_outbox
                       WHERE server=?1 AND origin=?2 AND offer_id=?3 AND offer_revision=?4)",
        params![key.server.to_bytes(), origin, key.id, key.revision],
        |r| r.get(0),
    )?)
}

pub(super) const RETRY_SECONDS: u64 = 60;

pub(super) enum Advance {
    Reserve { attempt: u64, retry_at: u64 },
    Acknowledge { wall: u64 },
}

pub(super) fn advance(
    tx: &Transaction<'_>,
    prepared: &Candidate,
    old: &Outbox,
    change: Advance,
) -> Result<()> {
    let binding = prepared.binding;
    let (attempt, retry_at, ack_at) = match change {
        Advance::Reserve { attempt, retry_at } => (attempt, retry_at, None),
        Advance::Acknowledge { wall } => (
            old.attempt,
            old.retry_at.ok_or(CursorError::Stale)?,
            Some(seconds(wall)?),
        ),
    };
    schema::Writer::Update.guard(tx, false)?;
    let changed = tx.execute(
        "UPDATE peer_prepared_consent_outbox SET attempt=?1,retry_at=?2,ack_at=?3
         WHERE class=?4 AND generation=?5 AND account=?6 AND server=?7 AND offer_id=?8
            AND offer_revision=?9 AND origin=?10 AND carrier=?11 AND frame_digest=?12
            AND expires=?13 AND attempt=?14 AND retry_at IS ?15 AND ack_at IS NULL",
        params![
            seconds(attempt)?,
            seconds(retry_at)?,
            ack_at,
            binding.generation.known,
            seconds(binding.generation.count)?,
            prepared.account,
            binding.key.server.to_bytes(),
            binding.key.id,
            binding.key.revision,
            binding.origin,
            old.transcript.carrier,
            old.digest,
            seconds(old.transcript.expires)?,
            seconds(old.attempt)?,
            old.retry_at.map(seconds).transpose()?,
        ],
    )?;
    if changed != 1 {
        return Err(CursorError::Stale.into());
    }
    schema::Writer::Update.guard(tx, true)?;
    Ok(())
}
