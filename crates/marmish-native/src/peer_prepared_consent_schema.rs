//! Exact fixture epoch and transactional writer apertures.

use rusqlite::Connection;
#[cfg(test)]
use rusqlite::TransactionBehavior;
#[cfg(test)]
use std::sync::OnceLock;

use super::super::super::super::stored_origins;
#[cfg(test)]
use super::super::super::super::{HardenedPeerConnection, MAX_ORIGINS, check_hardening};
#[cfg(test)]
use super::super::retirement::schema::{Entry, expected as prior_inventory, inventory};
use super::super::stage_schema::STAGE_CLAIM_TRIGGER;
use super::*;

const EPOCH: &str = "
CREATE TABLE peer_prepared_consent_epoch (
    epoch INTEGER PRIMARY KEY CHECK (epoch=1)
) STRICT;
INSERT INTO peer_prepared_consent_epoch VALUES (1);
CREATE TRIGGER peer_prepared_epoch_insert BEFORE INSERT ON peer_prepared_consent_epoch
BEGIN SELECT RAISE(ABORT,'immutable export epoch'); END;
CREATE TRIGGER peer_prepared_epoch_update BEFORE UPDATE ON peer_prepared_consent_epoch
BEGIN SELECT RAISE(ABORT,'immutable export epoch'); END;
CREATE TRIGGER peer_prepared_epoch_delete BEFORE DELETE ON peer_prepared_consent_epoch
BEGIN SELECT RAISE(ABORT,'immutable export epoch'); END;
CREATE TABLE peer_prepared_consent_outbox (
    class INTEGER PRIMARY KEY CHECK (class IN (0,1)),
    generation INTEGER NOT NULL CHECK (generation>0),
    account BLOB NOT NULL CHECK (length(account)=32),
    server BLOB NOT NULL UNIQUE CHECK (length(server)=32 AND server!=account),
    offer_id BLOB NOT NULL CHECK (length(offer_id)=16),
    offer_revision BLOB NOT NULL CHECK (length(offer_revision)=32),
    origin BLOB NOT NULL CHECK (length(origin)=32),
    package_id BLOB NOT NULL CHECK (length(package_id)=32),
    package_digest BLOB NOT NULL CHECK (length(package_digest)=32),
    package_expires INTEGER NOT NULL CHECK (package_expires>0),
    carrier BLOB NOT NULL UNIQUE CHECK (length(carrier)=32),
    frame BLOB NOT NULL CHECK (length(frame) BETWEEN 1 AND 65536),
    frame_digest BLOB NOT NULL CHECK (length(frame_digest)=32),
    revision BLOB NOT NULL CHECK (length(revision)=32),
    created_at INTEGER NOT NULL CHECK (created_at>=0),
    expires INTEGER NOT NULL CHECK (expires>created_at AND expires<=package_expires),
    package_ref BLOB NOT NULL CHECK (length(package_ref)=32),
    package_slot BLOB NOT NULL CHECK (length(package_slot)=16),
    attempt INTEGER NOT NULL DEFAULT 0 CHECK (attempt>=0),
    retry_at INTEGER CHECK (retry_at>=0),
    ack_at INTEGER CHECK (ack_at>=0 AND ack_at<expires),
    CHECK ((attempt=0 AND retry_at IS NULL AND ack_at IS NULL)
        OR (attempt>0 AND retry_at IS NOT NULL))
) STRICT;
CREATE TRIGGER peer_prepared_consent_reference BEFORE INSERT ON peer_prepared_consent_outbox
WHEN NEW.attempt!=0 OR NEW.retry_at IS NOT NULL OR NEW.ack_at IS NOT NULL OR NOT EXISTS (
    SELECT 1 FROM peer_provisioning_slot p
    JOIN peer_consent_invite i ON i.server=p.server AND i.offer_id=p.offer_id
        AND i.revision=p.revision AND i.origin=p.origin AND i.reserved=p.class
    JOIN peer_provisional_stage_v1 s ON s.class=p.class AND s.generation=p.generation
        AND s.server=p.server AND s.offer_id=p.offer_id AND s.revision=p.revision
        AND s.origin=p.origin
    JOIN peer_consent_version c ON c.singleton=1 AND c.account=NEW.account
    WHERE p.class=NEW.class AND p.generation=NEW.generation AND p.server=NEW.server
        AND p.offer_id=NEW.offer_id AND p.revision=NEW.offer_revision AND p.origin=NEW.origin
        AND i.decision_at IS NULL AND s.state=2 AND s.package_id=NEW.package_id
        AND s.package_digest=NEW.package_digest AND s.package_expires=NEW.package_expires
)
BEGIN SELECT RAISE(ABORT,'exact Prepared stage required'); END;
CREATE TRIGGER peer_prepared_consent_immutable BEFORE UPDATE ON peer_prepared_consent_outbox
WHEN OLD.class IS NOT NEW.class OR OLD.generation IS NOT NEW.generation
    OR OLD.account IS NOT NEW.account OR OLD.server IS NOT NEW.server
    OR OLD.offer_id IS NOT NEW.offer_id OR OLD.offer_revision IS NOT NEW.offer_revision
    OR OLD.origin IS NOT NEW.origin OR OLD.package_id IS NOT NEW.package_id
    OR OLD.package_digest IS NOT NEW.package_digest OR OLD.package_expires IS NOT NEW.package_expires
    OR OLD.carrier IS NOT NEW.carrier OR OLD.frame IS NOT NEW.frame
    OR OLD.frame_digest IS NOT NEW.frame_digest OR OLD.revision IS NOT NEW.revision
    OR OLD.created_at IS NOT NEW.created_at OR OLD.expires IS NOT NEW.expires
    OR OLD.package_ref IS NOT NEW.package_ref OR OLD.package_slot IS NOT NEW.package_slot
    OR OLD.ack_at IS NOT NULL
    OR NOT (
        (NEW.attempt=OLD.attempt+1 AND NEW.retry_at IS NOT NULL
            AND (OLD.retry_at IS NULL OR NEW.retry_at>OLD.retry_at) AND NEW.ack_at IS NULL)
        OR (OLD.attempt>0 AND NEW.attempt=OLD.attempt AND NEW.retry_at IS OLD.retry_at
            AND NEW.ack_at IS NOT NULL AND NEW.ack_at>=OLD.retry_at-60)
    )
BEGIN SELECT RAISE(ABORT,'exact Consent is immutable'); END;
";

#[derive(Clone, Copy)]
pub(super) enum Writer {
    Insert,
    Update,
    Delete,
}

impl Writer {
    pub(super) fn guard(
        self,
        conn: &Connection,
        install: bool,
    ) -> std::result::Result<(), CursorError> {
        let verb = match self {
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
        };
        let sql = if install {
            format!(
                "CREATE TRIGGER peer_prepared_deny_{verb} BEFORE {verb}
                     ON peer_prepared_consent_outbox
                     BEGIN SELECT RAISE(ABORT,'export owner required'); END"
            )
        } else {
            format!("DROP TRIGGER peer_prepared_deny_{verb}")
        };
        conn.execute_batch(&sql)?;
        Ok(())
    }
}

pub(super) fn install(conn: &Connection) -> std::result::Result<(), CursorError> {
    conn.execute_batch("DROP TRIGGER peer_provisioning_reject_claim")?;
    conn.execute_batch(&STAGE_CLAIM_TRIGGER.replace(
        "provisional stage v1",
        "provisional Consent export fixture 1",
    ))?;
    conn.execute_batch(EPOCH)?;
    for writer in [Writer::Insert, Writer::Update, Writer::Delete] {
        writer.guard(conn, true)?;
    }
    Ok(())
}

#[cfg(test)]
fn expected() -> std::result::Result<&'static [Entry], CursorError> {
    static EXPORT: OnceLock<Vec<Entry>> = OnceLock::new();
    if let Some(entries) = EXPORT.get() {
        return Ok(entries);
    }
    let template = template()?;
    let entries = inventory(&template).map_err(cursor_error)?;
    Ok(EXPORT.get_or_init(|| entries))
}

pub(super) fn template() -> std::result::Result<Connection, CursorError> {
    let template = Connection::open_in_memory()?;
    for ddl in [
        super::super::super::super::SCHEMA,
        rows::SCHEMA,
        super::SLOT_TABLE,
        super::OUTBOX_TRIGGER,
        STAGE_CLAIM_TRIGGER,
    ] {
        template.execute_batch(ddl)?;
    }
    for (_, ddl) in super::stage_schema::SCHEMA {
        template.execute_batch(ddl)?;
    }
    install(&template)?;
    Ok(template)
}

pub(super) fn cursor_error(error: Error) -> CursorError {
    match error {
        Error::Cursor(cause) => cause,
        _ => CursorError::Binding,
    }
}

#[cfg(test)]
pub(super) fn verify(
    conn: &rusqlite::Transaction<'_>,
    account: [u8; 32],
    origins: &[Origin],
) -> std::result::Result<(), CursorError> {
    if inventory(conn).map_err(cursor_error)? != expected()? {
        return Err(CursorError::Binding);
    }
    verify_rows(conn, account, origins)
}

#[cfg(test)]
pub(super) fn verify_for_epoch(
    conn: &rusqlite::Transaction<'_>,
    account: [u8; 32],
    origins: &[Origin],
    epoch: &Epoch,
) -> std::result::Result<(), CursorError> {
    match epoch {
        Epoch::Prepared => verify(conn, account, origins),
        Epoch::Custody(epoch) => epoch.verify(conn, account, origins),
    }
}

pub(super) fn verify_rows(
    conn: &rusqlite::Transaction<'_>,
    account: [u8; 32],
    origins: &[Origin],
) -> std::result::Result<(), CursorError> {
    let marker: i64 = conn.query_row(
        "SELECT count(*) FROM peer_prepared_consent_epoch WHERE epoch=1",
        [],
        |row| row.get(0),
    )?;
    if marker != 1 {
        return Err(CursorError::Binding);
    }
    verify_owner(conn, account, origins)?;
    let invalid: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM peer_prepared_consent_outbox o
            WHERE o.account!=?1 OR NOT EXISTS (
                SELECT 1 FROM peer_provisioning_slot p
                JOIN peer_consent_invite i ON i.server=p.server AND i.offer_id=p.offer_id
                    AND i.revision=p.revision AND i.origin=p.origin AND i.reserved=p.class
                JOIN peer_cursor_origin r ON r.origin=p.origin
                JOIN peer_provisional_stage_v1 s ON s.class=p.class AND s.generation=p.generation
                    AND s.server=p.server AND s.offer_id=p.offer_id AND s.revision=p.revision
                    AND s.origin=p.origin
                WHERE p.class=o.class AND p.generation=o.generation AND p.server=o.server
                    AND p.offer_id=o.offer_id AND p.revision=o.offer_revision AND p.origin=o.origin
                    AND i.decision_at IS NULL AND s.state=2 AND s.package_id=o.package_id
                    AND s.package_digest=o.package_digest AND s.package_expires=o.package_expires
            )
        )",
        [account],
        |row| row.get(0),
    )?;
    if invalid {
        return Err(CursorError::Binding);
    }
    Ok(())
}

fn verify_owner(
    conn: &rusqlite::Transaction<'_>,
    account: [u8; 32],
    origins: &[Origin],
) -> std::result::Result<(), CursorError> {
    for query in [
        "SELECT account FROM peer_cursor_version WHERE singleton=1 AND version=1",
        "SELECT account FROM peer_consent_version WHERE singleton=1 AND version=2",
    ] {
        let stored = conn.query_row(query, [], |row| row.get::<_, [u8; 32]>(0))?;
        if stored != account {
            return Err(CursorError::Binding);
        }
    }
    let slot_count: i64 = conn.query_row(
        "SELECT count(*) FROM peer_provisioning_slot WHERE class IN (0,1)",
        [],
        |row| row.get(0),
    )?;
    if slot_count != 2 || stored_origins(conn)? != origins {
        return Err(CursorError::Binding);
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn open(
    HardenedPeerConnection(mut conn): HardenedPeerConnection,
    account: [u8; 32],
    mut origins: Vec<Origin>,
) -> std::result::Result<CursorStore, CursorError> {
    if !(1..=MAX_ORIGINS).contains(&origins.len()) {
        return Err(CursorError::Binding);
    }
    origins.sort_unstable();
    check_hardening(&conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = inventory(&tx).map_err(cursor_error)?;
    if current == prior_inventory(false).map_err(cursor_error)? {
        verify_owner(&tx, account, &origins)?;
        install(&tx)?;
    }
    verify(&tx, account, &origins)?;
    tx.commit()?;
    Ok(CursorStore { conn, origins })
}
