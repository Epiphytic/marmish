//! Persist opaque deferred records. Only the owner can freshly classify their authority.

use zeroize::Zeroizing;

use super::*;

const BACKOFF: [u64; 7] = [1, 2, 4, 8, 16, 32, 60];
const OUTER_HORIZON: u64 = 90 * 3600;

#[repr(i64)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Auth {
    Unverified = 0,
    OuterVerified = 1,
    FlightVerified = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Metadata {
    pub(crate) auth: Auth,
    pub(crate) expires: u64,
    pub(crate) prerequisite: Option<[u8; 32]>,
}

impl Metadata {
    fn check(self, wall: u64) -> Result<()> {
        if self.prerequisite.is_some() && self.auth != Auth::FlightVerified
            || seconds(self.expires)? > after(wall, OUTER_HORIZON)?
        {
            return Err(Error::Range);
        }
        Ok(())
    }
}

pub(crate) struct Snapshot {
    pub(crate) origin: Origin,
    id: EventId,
    stamp: u64,
    bytes: Zeroizing<Vec<u8>>,
    pub(crate) metadata: Metadata,
    pub(crate) retry_at: u64,
    step: usize,
    revision: i64,
}

impl Snapshot {
    pub(crate) fn record(&self) -> RawRecord<'_> {
        RawRecord {
            id: self.id,
            stamp: self.stamp,
            bytes: self.bytes.as_slice(),
        }
    }

    /// Supply the owner's fresh verifier. The resulting proof is valid only for this snapshot.
    pub(crate) fn reverify(
        &self,
        classify: impl FnOnce(RawRecord<'_>) -> Option<RetryFinding>,
    ) -> Option<RetryProof> {
        let finding = classify(self.record())?;
        Some(RetryProof {
            origin: self.origin,
            id: self.id,
            digest: self.record().digest(),
            revision: self.revision,
            finding,
        })
    }
}

pub(crate) enum RetryFinding {
    Invalid,
    Expired { verified_expiry: u64 },
}

pub(crate) struct RetryProof {
    origin: Origin,
    id: EventId,
    digest: [u8; 32],
    revision: i64,
    finding: RetryFinding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrustedOutcome {
    Applied,
    AlreadyApplied,
}

impl CursorStore {
    pub(crate) fn commit_trusted(
        &mut self,
        origin: Origin,
        record: RawRecord<'_>,
        wall: u64,
        write_effect_and_fence: impl FnOnce(&Transaction<'_>) -> Result<TrustedOutcome>,
    ) -> Result<(Receipt, TrustedOutcome)> {
        record.check()?;
        let tx = self.with_wall(origin, wall)?;
        // This is a trusted, synchronous owner seam. Its callback uses only owner
        // tables and cannot control this transaction. An unwind drops the transaction.
        let outcome = write_effect_and_fence(&tx)?;
        tx.execute(
            "DELETE FROM peer_cursor_deferred WHERE origin=?1 AND id=?2",
            params![origin, record.id],
        )?;
        tx.commit()?;
        Ok((Receipt::new(origin, record, ReceiptKind::Accepted), outcome))
    }

    pub(crate) fn commit_invalid(
        &mut self,
        origin: Origin,
        record: RawRecord<'_>,
        wall: u64,
    ) -> Result<Receipt> {
        record.check()?;
        let tx = self.with_wall(origin, wall)?;
        tx.execute(
            "DELETE FROM peer_cursor_deferred WHERE origin=?1 AND id=?2 AND stamp=?3 AND frame=?4",
            params![origin, record.id, seconds(record.stamp)?, record.bytes],
        )?;
        tx.commit()?;
        Ok(Receipt::new(origin, record, ReceiptKind::Invalid))
    }

    pub(crate) fn defer(
        &mut self,
        origin: Origin,
        record: RawRecord<'_>,
        metadata: Metadata,
        wall: u64,
    ) -> Result<Receipt> {
        record.check()?;
        let tx = self.with_wall(origin, wall)?;
        metadata.check(wall)?;
        if let Some(saved) = load(&tx, origin, record.id)? {
            if !same_record(saved.record(), record) {
                return Err(Error::Conflict);
            }
            let new_prerequisite = metadata.auth == Auth::FlightVerified
                && saved.metadata.auth == Auth::FlightVerified
                && metadata.prerequisite != saved.metadata.prerequisite;
            if metadata.auth > saved.metadata.auth || new_prerequisite {
                if new_prerequisite && metadata.expires != saved.metadata.expires {
                    return Err(Error::Conflict);
                }
                update_record(&tx, &saved, record, metadata)?;
            }
        } else {
            check_capacity(&tx, origin, 0, record.bytes.len())?;
            tx.execute(
                "INSERT INTO peer_cursor_deferred VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0,?9)",
                params![
                    origin,
                    record.id,
                    seconds(record.stamp)?,
                    record.bytes,
                    metadata.auth as i64,
                    seconds(metadata.expires)?,
                    metadata.prerequisite,
                    after(wall, BACKOFF[0])?,
                    next_revision(&tx, origin)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(Receipt::new(origin, record, ReceiptKind::Deferred))
    }

    pub(crate) fn capacity(&mut self, origin: Origin) -> Result<bool> {
        let tx = self.begin(origin)?;
        let (items, bytes) = usage(&tx, origin)?;
        let available = items < VISIBLE && bytes < DEFERRED_BYTES;
        tx.commit()?;
        Ok(available)
    }

    pub(crate) fn existing(&mut self, origin: Origin, id: EventId) -> Result<Option<Snapshot>> {
        let tx = self.begin(origin)?;
        let snapshot = load(&tx, origin, id)?;
        tx.commit()?;
        Ok(snapshot)
    }

    pub(crate) fn replace_conflict(
        &mut self,
        snapshot: &Snapshot,
        incoming: RawRecord<'_>,
        metadata: Metadata,
        wall: u64,
    ) -> Result<Receipt> {
        incoming.check()?;
        let tx = self.with_wall(snapshot.origin, wall)?;
        metadata.check(wall)?;
        if metadata.auth < Auth::OuterVerified
            || metadata.auth < snapshot.metadata.auth
            || incoming.id != snapshot.id
            || same_record(incoming, snapshot.record())
            || (snapshot.metadata.auth >= Auth::OuterVerified && incoming.stamp != snapshot.stamp)
        {
            return Err(Error::Classification);
        }
        if !record_matches(&tx, snapshot)? {
            return Err(Error::Stale);
        }
        check_capacity(
            &tx,
            snapshot.origin,
            snapshot.bytes.len(),
            incoming.bytes.len(),
        )?;
        update_record(&tx, snapshot, incoming, metadata)?;
        tx.commit()?;
        Ok(Receipt::new(
            snapshot.origin,
            incoming,
            ReceiptKind::Deferred,
        ))
    }

    pub(crate) fn due(&mut self, origin: Origin, wall: u64) -> Result<Option<Snapshot>> {
        let tx = self.with_wall(origin, wall)?;
        let snapshot = tx.query_row(
            "SELECT id, stamp, frame, auth, expires, prerequisite, retry_at, step, revision
             FROM peer_cursor_deferred WHERE origin=?1 AND retry_at<=?2 ORDER BY retry_at,id LIMIT 1",
            params![origin, seconds(wall)?],
            |row| read_snapshot(origin, row),
        ).optional()?;
        tx.commit()?;
        Ok(snapshot)
    }

    pub(crate) fn retry_again(&mut self, snapshot: &Snapshot, wall: u64) -> Result<()> {
        let tx = self.with_wall(snapshot.origin, wall)?;
        if snapshot.retry_at > wall || !record_matches(&tx, snapshot)? {
            return Err(Error::Stale);
        }
        let step = (snapshot.step + 1).min(BACKOFF.len() - 1);
        tx.execute(
            "UPDATE peer_cursor_deferred SET retry_at=?3, step=?4, revision=?5 WHERE origin=?1 AND id=?2",
            params![snapshot.origin, snapshot.id, after(wall, BACKOFF[step])?, i64::try_from(step).map_err(|_| Error::Range)?, next_revision(&tx, snapshot.origin)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn wake(
        &mut self,
        origin: Origin,
        prerequisite: [u8; 32],
        wall: u64,
    ) -> Result<usize> {
        let tx = self.with_wall(origin, wall)?;
        let updated = tx.execute(
            "UPDATE peer_cursor_deferred SET retry_at=?3,revision=?4
                WHERE origin=?1 AND auth=2 AND prerequisite=?2 AND retry_at>?3",
            params![
                origin,
                prerequisite,
                seconds(wall)?,
                next_revision(&tx, origin)?
            ],
        )?;
        tx.commit()?;
        Ok(updated)
    }

    pub(crate) fn finish_retry(&mut self, proof: RetryProof, wall: u64) -> Result<()> {
        let tx = self.with_wall(proof.origin, wall)?;
        let snapshot = load(&tx, proof.origin, proof.id)?.ok_or(Error::Stale)?;
        if snapshot.revision != proof.revision || snapshot.record().digest() != proof.digest {
            return Err(Error::Stale);
        }
        if matches!(proof.finding, RetryFinding::Expired { verified_expiry } if wall < verified_expiry)
        {
            return Err(Error::Classification);
        }
        tx.execute(
            "DELETE FROM peer_cursor_deferred WHERE origin=?1 AND id=?2",
            params![proof.origin, proof.id],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn next_revision(tx: &Transaction<'_>, origin: Origin) -> Result<i64> {
    Ok(tx.query_row(
        "UPDATE peer_cursor_origin SET retry_revision=retry_revision+1 WHERE origin=?1 RETURNING retry_revision",
        [origin], |row| row.get(0),
    )?)
}

pub(super) fn rebase_due(tx: &Transaction<'_>, origin: Origin, wall: u64) -> Result<()> {
    tx.execute(
        "UPDATE peer_cursor_deferred SET retry_at=?2,revision=?3 WHERE origin=?1 AND retry_at>?2",
        params![origin, seconds(wall)?, next_revision(tx, origin)?],
    )?;
    Ok(())
}

fn usage(tx: &Transaction<'_>, origin: Origin) -> Result<(usize, usize)> {
    Ok(tx.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(frame)),0) FROM peer_cursor_deferred WHERE origin=?1",
        [origin],
        |row| {
            Ok((
                usize::from(row.get::<_, u16>(0)?),
                row.get::<_, u32>(1)? as usize,
            ))
        },
    )?)
}

fn check_capacity(
    tx: &Transaction<'_>,
    origin: Origin,
    existing: usize,
    incoming: usize,
) -> Result<()> {
    let (count, bytes) = usage(tx, origin)?;
    let revised = bytes
        .checked_sub(existing)
        .and_then(|size| size.checked_add(incoming))
        .ok_or(Error::Storage)?;
    if count + usize::from(existing == 0) > VISIBLE || revised > DEFERRED_BYTES {
        return Err(Error::Backlog);
    }
    Ok(())
}

fn read_snapshot(origin: Origin, row: &rusqlite::Row<'_>) -> rusqlite::Result<Snapshot> {
    let auth = match row.get::<_, i64>(3)? {
        0 => Auth::Unverified,
        1 => Auth::OuterVerified,
        2 => Auth::FlightVerified,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    let step = usize::from(row.get::<_, u8>(7)?);
    if step >= BACKOFF.len() {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(Snapshot {
        origin,
        id: row.get(0)?,
        stamp: read_seconds(row, 1)?,
        bytes: Zeroizing::new(row.get(2)?),
        metadata: Metadata {
            auth,
            expires: read_seconds(row, 4)?,
            prerequisite: row.get(5)?,
        },
        retry_at: read_seconds(row, 6)?,
        step,
        revision: row.get(8)?,
    })
}

fn load(tx: &Transaction<'_>, origin: Origin, id: EventId) -> Result<Option<Snapshot>> {
    Ok(tx.query_row(
        "SELECT id,stamp,frame,auth,expires,prerequisite,retry_at,step,revision FROM peer_cursor_deferred
         WHERE origin=?1 AND id=?2",
        params![origin, id], |row| read_snapshot(origin, row),
    ).optional()?)
}

fn same_record(left: RawRecord<'_>, right: RawRecord<'_>) -> bool {
    left.id == right.id && left.stamp == right.stamp && left.bytes == right.bytes
}

pub(super) fn record_exists(
    tx: &Transaction<'_>,
    origin: Origin,
    record: RawRecord<'_>,
) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_cursor_deferred WHERE origin=?1 AND id=?2 AND stamp=?3 AND frame=?4)",
        params![origin, record.id, seconds(record.stamp)?, record.bytes], |row| row.get(0),
    )?)
}

fn record_matches(tx: &Transaction<'_>, snapshot: &Snapshot) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_cursor_deferred WHERE origin=?1 AND id=?2 AND stamp=?3 AND frame=?4 AND revision=?5)",
        params![snapshot.origin, snapshot.id, seconds(snapshot.stamp)?, snapshot.bytes.as_slice(), snapshot.revision],
        |row| row.get(0),
    )?)
}

fn update_record(
    tx: &Transaction<'_>,
    snapshot: &Snapshot,
    incoming: RawRecord<'_>,
    metadata: Metadata,
) -> Result<()> {
    tx.execute(
        "UPDATE peer_cursor_deferred SET stamp=?3, frame=?4, auth=?5, expires=?6,
            prerequisite=?7, revision=?8 WHERE origin=?1 AND id=?2",
        params![
            snapshot.origin,
            snapshot.id,
            seconds(incoming.stamp)?,
            incoming.bytes,
            metadata.auth as i64,
            seconds(metadata.expires)?,
            metadata.prerequisite,
            next_revision(tx, snapshot.origin)?
        ],
    )?;
    Ok(())
}
