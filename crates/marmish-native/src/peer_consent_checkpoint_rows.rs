use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};

use super::super::{read_optional_seconds, read_seconds, seconds};
use super::{CursorError, CursorStore, Error, Offer, Origin, PublicKey, RawRecord, Selector};
use super::{TrustedOutcome, Zeroizing};

type DbResult<T> = std::result::Result<T, CursorError>;

const INVITATIONS_PER_CLASS: u16 = 8;
const FENCES_PER_ORIGIN_CLASS: u16 = 256;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS peer_consent_version (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL CHECK (version = 2),
    account BLOB NOT NULL CHECK (length(account) = 32),
    wall INTEGER NOT NULL DEFAULT 0 CHECK (wall >= 0)
) STRICT;
CREATE TABLE IF NOT EXISTS peer_consent_invite (
    server BLOB PRIMARY KEY CHECK (length(server) = 32),
    offer_id BLOB NOT NULL CHECK (length(offer_id) = 16),
    revision BLOB NOT NULL CHECK (length(revision) = 32),
    origin BLOB NOT NULL REFERENCES peer_cursor_origin(origin),
    carrier BLOB NOT NULL CHECK (length(carrier) = 32),
    expires INTEGER NOT NULL CHECK (expires > 0),
    replay_until INTEGER NOT NULL CHECK (replay_until > expires),
    reserved INTEGER NOT NULL CHECK (reserved IN (0, 1)),
    decision_at INTEGER CHECK (decision_at >= 0 AND decision_at < expires),
    frame BLOB NOT NULL CHECK (length(frame) BETWEEN 1 AND 65536),
    UNIQUE (server, offer_id, revision, origin)
) STRICT;
CREATE TABLE IF NOT EXISTS peer_consent_fence (
    origin BLOB NOT NULL REFERENCES peer_cursor_origin(origin),
    revision BLOB NOT NULL CHECK (length(revision) = 32),
    reserved INTEGER NOT NULL CHECK (reserved IN (0, 1)),
    replay_until INTEGER NOT NULL CHECK (replay_until > 0),
    PRIMARY KEY (origin, revision)
) STRICT;
CREATE INDEX IF NOT EXISTS peer_consent_fence_class ON peer_consent_fence (origin, reserved);
CREATE TABLE IF NOT EXISTS peer_consent_outbox (
    server BLOB PRIMARY KEY CHECK (length(server) = 32),
    offer_id BLOB NOT NULL CHECK (length(offer_id) = 16),
    offer_revision BLOB NOT NULL CHECK (length(offer_revision) = 32),
    origin BLOB NOT NULL,
    carrier BLOB NOT NULL CHECK (length(carrier) = 32),
    revision BLOB NOT NULL CHECK (length(revision) = 32),
    expires INTEGER NOT NULL CHECK (expires > 0),
    frame BLOB NOT NULL CHECK (length(frame) BETWEEN 1 AND 65536),
    package_id BLOB NOT NULL CHECK (length(package_id) = 32),
    package_digest BLOB NOT NULL CHECK (length(package_digest) = 32),
    attempt INTEGER NOT NULL DEFAULT 0 CHECK (attempt >= 0),
    retry_at INTEGER CHECK (retry_at >= 0),
    acknowledged_at INTEGER CHECK (acknowledged_at >= 0 AND acknowledged_at < expires),
    CHECK ((attempt = 0) = (retry_at IS NULL)),
    CHECK (acknowledged_at IS NULL OR attempt > 0),
    FOREIGN KEY (server, offer_id, offer_revision, origin)
        REFERENCES peer_consent_invite (server, offer_id, revision, origin) ON DELETE CASCADE
) STRICT;
";

pub(super) fn attach(cursor: &mut CursorStore, account: PublicKey) -> DbResult<()> {
    attach_with(cursor, account, super::provisioning::attach)
}

pub(super) fn attach_with(
    cursor: &mut CursorStore,
    account: PublicKey,
    provisioning: impl FnOnce(&Transaction<'_>, bool) -> DbResult<()>,
) -> DbResult<()> {
    let tx = cursor
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let account = account.to_bytes();
    let existing: [u8; 32] = tx.query_row(
        "SELECT account FROM peer_cursor_version WHERE singleton=1 AND version=1",
        [],
        |row| row.get(0),
    )?;
    if existing != account {
        return Err(CursorError::Binding);
    }
    tx.execute_batch(SCHEMA)?;
    let checkpoint: Option<(i64, [u8; 32])> = tx
        .query_row(
            "SELECT version, account FROM peer_consent_version WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    provisioning(&tx, checkpoint.is_some())?;
    match checkpoint {
        None => {
            tx.execute(
                "INSERT INTO peer_consent_version (singleton, version, account) VALUES (1, 2, ?1)",
                [account],
            )?;
        }
        Some((2, stored)) if stored == account => {}
        _ => return Err(CursorError::Binding),
    }
    tx.commit()?;
    Ok(())
}

pub(super) fn check_wall(conn: &Connection, wall: u64) -> DbResult<()> {
    let previous = conn.query_row(
        "SELECT wall FROM peer_consent_version WHERE singleton=1",
        [],
        |row| read_seconds(row, 0),
    )?;
    if wall < previous {
        return Err(CursorError::ClockRegressed);
    }
    Ok(())
}

pub(super) fn advance_wall(tx: &Transaction<'_>, wall: u64) -> DbResult<()> {
    let count = tx.execute(
        "UPDATE peer_consent_version SET wall=?1 WHERE singleton=1 AND wall<=?1",
        [seconds(wall)?],
    )?;
    if count != 1 {
        return Err(CursorError::ClockRegressed);
    }
    Ok(())
}

pub(super) fn sweep(tx: &Transaction<'_>, wall: u64) -> DbResult<()> {
    advance_wall(tx, wall)?;
    let wall = seconds(wall)?;
    tx.execute(
        "DELETE FROM peer_consent_invite WHERE expires<=?1
            AND NOT EXISTS (SELECT 1 FROM peer_consent_outbox o
                WHERE o.server=peer_consent_invite.server AND o.expires>?1)
            AND NOT EXISTS (SELECT 1 FROM peer_provisioning_slot p
                WHERE p.server=peer_consent_invite.server)",
        [wall],
    )?;
    tx.execute(
        "DELETE FROM peer_consent_fence WHERE replay_until<=?1",
        [wall],
    )?;
    Ok(())
}

pub(super) fn admit(
    tx: &Transaction<'_>,
    origin: Origin,
    record: RawRecord<'_>,
    offer: &Offer,
    reserved: bool,
    decision_at: Option<u64>,
) -> DbResult<TrustedOutcome> {
    let key = Selector::from(offer);
    let flight = offer.flight();
    let replay = tx
        .query_row(
            "SELECT 1 FROM peer_consent_fence WHERE origin=?1 AND revision=?2",
            params![origin, key.revision],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false);
    if replay {
        return Ok(TrustedOutcome::AlreadyApplied);
    }
    check_capacity(tx, origin, key.server, reserved)?;
    require_write(tx.execute(
        "INSERT INTO peer_consent_invite
            (server, offer_id, revision, origin, carrier, expires, replay_until, reserved,
             decision_at, frame)
            VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            key.server.to_bytes(),
            key.id,
            key.revision,
            origin,
            record.id,
            seconds(flight.expires_at)?,
            seconds(flight.replay_safe_until)?,
            reserved,
            decision_at.map(seconds).transpose()?,
            record.bytes
        ],
    )?)?;
    require_write(tx.execute(
        "INSERT INTO peer_consent_fence (origin, revision, reserved, replay_until)
            VALUES (?1,?2,?3,?4)",
        params![
            origin,
            key.revision,
            reserved,
            seconds(flight.replay_safe_until)?
        ],
    )?)?;
    Ok(TrustedOutcome::Applied)
}

pub(super) fn require_write(changed: usize) -> DbResult<()> {
    if changed != 1 {
        return Err(CursorError::Storage);
    }
    Ok(())
}

fn check_capacity(
    tx: &Transaction<'_>,
    origin: Origin,
    server: PublicKey,
    reserved: bool,
) -> DbResult<()> {
    let (active, invitations, fences): (bool, u16, u16) = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_consent_invite WHERE server=?1),
                (SELECT count(*) FROM peer_consent_invite WHERE reserved=?2),
                (SELECT count(*) FROM peer_consent_fence WHERE origin=?3 AND reserved=?2)",
        params![server.to_bytes(), reserved, origin],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if active || invitations >= INVITATIONS_PER_CLASS || fences >= FENCES_PER_ORIGIN_CLASS {
        return Err(CursorError::Backlog);
    }
    Ok(())
}

pub(super) fn stored_origin(conn: &Connection, key: Selector) -> super::Result<Origin> {
    conn.query_row(
        "SELECT origin FROM peer_consent_invite WHERE server=?1 AND offer_id=?2 AND revision=?3",
        params![key.server.to_bytes(), key.id, key.revision],
        |row| row.get(0),
    )
    .optional()?
    .ok_or(Error::Missing)
}

pub(super) struct Invitation {
    pub(super) key: Selector,
    pub(super) origin: Origin,
    pub(super) carrier: [u8; 32],
    pub(super) expires: u64,
    pub(super) replay_until: u64,
    pub(super) decision_at: Option<u64>,
    pub(super) frame: Zeroizing<Vec<u8>>,
}

pub(crate) struct Outbox {
    pub(crate) origin: Origin,
    pub(crate) carrier: [u8; 32],
    pub(crate) revision: [u8; 32],
    pub(crate) expires: u64,
    pub(crate) frame: Zeroizing<Vec<u8>>,
    pub(crate) package_id: [u8; 32],
    pub(crate) package_digest: [u8; 32],
    pub(crate) attempt: u64,
    pub(crate) retry_at: Option<u64>,
    pub(crate) acknowledged_at: Option<u64>,
}

impl Invitation {
    pub(super) fn load(tx: &Transaction<'_>, key: Selector, origin: Origin) -> super::Result<Self> {
        tx.query_row(
            "SELECT carrier, expires, replay_until, decision_at, frame FROM peer_consent_invite
                WHERE server=?1 AND offer_id=?2 AND revision=?3 AND origin=?4",
            params![key.server.to_bytes(), key.id, key.revision, origin],
            |row| {
                Ok(Self {
                    key,
                    origin,
                    carrier: row.get(0)?,
                    expires: read_seconds(row, 1)?,
                    replay_until: read_seconds(row, 2)?,
                    decision_at: read_optional_seconds(row, 3)?,
                    frame: Zeroizing::new(row.get(4)?),
                })
            },
        )
        .optional()?
        .ok_or(Error::Missing)
    }

    pub(super) fn approve(&self, tx: &Transaction<'_>, wall: u64) -> DbResult<()> {
        require_write(tx.execute(
            "UPDATE peer_consent_invite SET decision_at=coalesce(decision_at, ?5)
                WHERE server=?1 AND offer_id=?2 AND revision=?3 AND origin=?4",
            params![
                self.key.server.to_bytes(),
                self.key.id,
                self.key.revision,
                self.origin,
                seconds(wall)?
            ],
        )?)?;
        Ok(())
    }

    pub(super) fn remove(&self, tx: &Transaction<'_>) -> DbResult<()> {
        require_write(tx.execute(
            "DELETE FROM peer_consent_invite
                WHERE server=?1 AND offer_id=?2 AND revision=?3 AND origin=?4",
            params![
                self.key.server.to_bytes(),
                self.key.id,
                self.key.revision,
                self.origin
            ],
        )?)?;
        Ok(())
    }

    pub(super) fn outbox(&self, tx: &Transaction<'_>) -> DbResult<Option<Outbox>> {
        Ok(tx
            .query_row(
                "SELECT carrier, revision, expires, frame, package_id, package_digest,
                        attempt, retry_at, acknowledged_at FROM peer_consent_outbox
                WHERE server=?1 AND offer_id=?2 AND offer_revision=?3 AND origin=?4",
                params![
                    self.key.server.to_bytes(),
                    self.key.id,
                    self.key.revision,
                    self.origin
                ],
                |row| {
                    Ok(Outbox {
                        origin: self.origin,
                        carrier: row.get(0)?,
                        revision: row.get(1)?,
                        expires: read_seconds(row, 2)?,
                        frame: Zeroizing::new(row.get(3)?),
                        package_id: row.get(4)?,
                        package_digest: row.get(5)?,
                        attempt: read_seconds(row, 6)?,
                        retry_at: read_optional_seconds(row, 7)?,
                        acknowledged_at: read_optional_seconds(row, 8)?,
                    })
                },
            )
            .optional()?)
    }

    pub(super) fn save_outbox(&self, tx: &Transaction<'_>, outbox: &Outbox) -> DbResult<()> {
        if self.decision_at.is_none() || self.origin != outbox.origin {
            return Err(CursorError::Binding);
        }
        require_write(tx.execute(
            "INSERT INTO peer_consent_outbox
                (server, offer_id, offer_revision, origin, carrier, revision, expires, frame, package_id, package_digest)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                self.key.server.to_bytes(),
                self.key.id,
                self.key.revision,
                self.origin,
                outbox.carrier,
                outbox.revision,
                seconds(outbox.expires)?,
                outbox.frame.as_slice(),
                outbox.package_id,
                outbox.package_digest
            ],
        )?)?;
        Ok(())
    }
}
