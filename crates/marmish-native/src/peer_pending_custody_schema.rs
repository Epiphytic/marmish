//! Exact custody epoch layered on the Prepared Consent inventory.

use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::sync::OnceLock;

use super::super::super::super::super::{HardenedPeerConnection, MAX_ORIGINS, check_hardening};
use super::super::super::retirement::schema::{Entry, inventory};
use super::*;

const DDL: &str = "
CREATE TABLE peer_pending_custody_epoch (
    epoch INTEGER PRIMARY KEY CHECK (epoch=2)
) STRICT;
INSERT INTO peer_pending_custody_epoch VALUES (2);
CREATE TRIGGER peer_custody_epoch_insert BEFORE INSERT ON peer_pending_custody_epoch
BEGIN SELECT RAISE(ABORT,'immutable custody epoch'); END;
CREATE TRIGGER peer_custody_epoch_update BEFORE UPDATE ON peer_pending_custody_epoch
BEGIN SELECT RAISE(ABORT,'immutable custody epoch'); END;
CREATE TRIGGER peer_custody_epoch_delete BEFORE DELETE ON peer_pending_custody_epoch
BEGIN SELECT RAISE(ABORT,'immutable custody epoch'); END;

CREATE TABLE peer_pending_custody (
    class INTEGER PRIMARY KEY CHECK (class IN (0,1)),
    generation INTEGER NOT NULL CHECK (generation>0),
    account BLOB NOT NULL CHECK (length(account)=32),
    server BLOB NOT NULL UNIQUE CHECK (length(server)=32 AND server!=account),
    offer_id BLOB NOT NULL CHECK (length(offer_id)=16),
    offer_revision BLOB NOT NULL CHECK (length(offer_revision)=32),
    origin BLOB NOT NULL CHECK (length(origin)=32),
    consent_carrier BLOB NOT NULL UNIQUE CHECK (length(consent_carrier)=32)
        REFERENCES peer_prepared_consent_outbox(carrier) ON DELETE RESTRICT ON UPDATE RESTRICT,
    consent_revision BLOB NOT NULL CHECK (length(consent_revision)=32),
    delivery_carrier BLOB NOT NULL UNIQUE CHECK (length(delivery_carrier)=32),
    outer_stamp INTEGER NOT NULL CHECK (outer_stamp>0),
    flight_created INTEGER NOT NULL CHECK (flight_created>0),
    expires INTEGER NOT NULL CHECK (expires>flight_created),
    frame BLOB NOT NULL CHECK (length(frame) BETWEEN 1 AND 65536),
    replay_until INTEGER NOT NULL CHECK (replay_until>expires),
    welcome_id BLOB NOT NULL CHECK (length(welcome_id)=32),
    welcome_digest BLOB NOT NULL CHECK (length(welcome_digest)=32),
    package_id BLOB NOT NULL CHECK (length(package_id)=32),
    package_digest BLOB NOT NULL CHECK (length(package_digest)=32),
    package_ref BLOB NOT NULL CHECK (length(package_ref)=32),
    package_slot BLOB NOT NULL CHECK (length(package_slot)=16),
    package_not_before INTEGER NOT NULL CHECK (package_not_before>=0),
    package_not_after INTEGER NOT NULL CHECK (package_not_after>package_not_before),
    mode INTEGER NOT NULL CHECK (mode IN (0,1)),
    held_at INTEGER NOT NULL CHECK (held_at>0 AND held_at>=package_not_before),
    held_deadline INTEGER NOT NULL CHECK (held_deadline>held_at AND
        held_deadline=min(expires,package_not_after)-300),
    state INTEGER NOT NULL CHECK (state IN (0,1,2,3)),
    started_at INTEGER CHECK (started_at>=held_at AND started_at<held_deadline),
    group_id BLOB CHECK (length(group_id) BETWEEN 1 AND 255),
    epoch_number INTEGER CHECK (epoch_number>=0),
    epoch_identity BLOB CHECK (length(epoch_identity)=32),
    own_leaf INTEGER CHECK (own_leaf BETWEEN 0 AND 4294967295),
    qualified_at INTEGER CHECK (qualified_at>=started_at AND qualified_at<held_deadline),
    pending_deadline INTEGER CHECK (pending_deadline>qualified_at AND
        pending_deadline=min(held_deadline,qualified_at+86400)),
    approval INTEGER CHECK (approval IN (0,1) AND (approval=0 OR mode=1)),
    activating_at INTEGER CHECK (activating_at>=qualified_at AND activating_at<pending_deadline),
    CHECK ((state=0 AND started_at IS NULL) OR (state>0 AND started_at IS NOT NULL)),
    CHECK ((state<2 AND group_id IS NULL AND epoch_number IS NULL AND epoch_identity IS NULL
        AND own_leaf IS NULL AND qualified_at IS NULL AND pending_deadline IS NULL)
      OR (state>=2 AND group_id IS NOT NULL AND epoch_number IS NOT NULL
        AND epoch_identity IS NOT NULL AND own_leaf IS NOT NULL AND qualified_at IS NOT NULL
        AND pending_deadline IS NOT NULL)),
    CHECK ((state<3 AND approval IS NULL AND activating_at IS NULL)
      OR (state=3 AND approval IS NOT NULL AND activating_at IS NOT NULL))
) STRICT;

CREATE TRIGGER peer_custody_insert_held BEFORE INSERT ON peer_pending_custody
WHEN NEW.state!=0
BEGIN SELECT RAISE(ABORT,'custody starts Held'); END;

CREATE TRIGGER peer_custody_next BEFORE UPDATE ON peer_pending_custody
WHEN NEW.state!=OLD.state+1 OR OLD.state=3
    OR OLD.class IS NOT NEW.class OR OLD.generation IS NOT NEW.generation
    OR OLD.account IS NOT NEW.account OR OLD.server IS NOT NEW.server
    OR OLD.offer_id IS NOT NEW.offer_id OR OLD.offer_revision IS NOT NEW.offer_revision
    OR OLD.origin IS NOT NEW.origin OR OLD.consent_carrier IS NOT NEW.consent_carrier
    OR OLD.consent_revision IS NOT NEW.consent_revision
    OR OLD.delivery_carrier IS NOT NEW.delivery_carrier OR OLD.outer_stamp IS NOT NEW.outer_stamp
    OR OLD.flight_created IS NOT NEW.flight_created OR OLD.expires IS NOT NEW.expires
    OR OLD.frame IS NOT NEW.frame OR OLD.replay_until IS NOT NEW.replay_until
    OR OLD.welcome_id IS NOT NEW.welcome_id OR OLD.welcome_digest IS NOT NEW.welcome_digest
    OR OLD.package_id IS NOT NEW.package_id OR OLD.package_digest IS NOT NEW.package_digest
    OR OLD.package_ref IS NOT NEW.package_ref OR OLD.package_slot IS NOT NEW.package_slot
    OR OLD.package_not_before IS NOT NEW.package_not_before
    OR OLD.package_not_after IS NOT NEW.package_not_after
    OR OLD.mode IS NOT NEW.mode OR OLD.held_at IS NOT NEW.held_at
    OR OLD.held_deadline IS NOT NEW.held_deadline
    OR (OLD.state>=1 AND OLD.started_at IS NOT NEW.started_at)
    OR (OLD.state>=2 AND (OLD.group_id IS NOT NEW.group_id
        OR OLD.epoch_number IS NOT NEW.epoch_number OR OLD.epoch_identity IS NOT NEW.epoch_identity
        OR OLD.own_leaf IS NOT NEW.own_leaf OR OLD.qualified_at IS NOT NEW.qualified_at
        OR OLD.pending_deadline IS NOT NEW.pending_deadline))
BEGIN SELECT RAISE(ABORT,'exact custody transition required'); END;
";

// Prepared epoch verification checks this outbox against the exact existing
// account, origin, slot, invitation and stage. This predicate pins the remaining
// custody coordinates and positive settlement to that same outbox.
const EXACT_OUTBOX: &str = "
    SELECT 1 FROM peer_prepared_consent_outbox o
    WHERE o.carrier=c.consent_carrier AND o.revision=c.consent_revision
        AND o.class=c.class AND o.generation=c.generation AND o.account=c.account
        AND o.server=c.server AND o.offer_id=c.offer_id AND o.offer_revision=c.offer_revision
        AND o.origin=c.origin AND o.package_id=c.package_id AND o.package_digest=c.package_digest
        AND o.package_ref=c.package_ref AND o.package_slot=c.package_slot
        AND c.expires<=o.expires AND c.flight_created>=o.created_at
        AND c.held_at<o.expires AND o.package_expires=c.package_not_after
        AND o.ack_at IS NOT NULL AND o.ack_at<=c.held_at";

#[derive(Clone, Copy)]
pub(super) enum Writer {
    Insert,
    Update,
}

impl Writer {
    fn verb(self) -> &'static str {
        match self {
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
        }
    }

    // DDL is transactional. If the write or post-check fails, the owner turns
    // roll back both the write and the temporary removal of the denial trigger.
    pub(super) fn guard(self, tx: &Transaction<'_>, install: bool) -> Result<()> {
        let verb = self.verb();
        if install {
            install_guard(tx, verb)?;
        } else {
            tx.execute_batch(&format!("DROP TRIGGER peer_custody_deny_{verb}"))?;
        }
        Ok(())
    }
}

fn install_guard(conn: &Connection, verb: &str) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "CREATE TRIGGER peer_custody_deny_{verb} BEFORE {verb} ON peer_pending_custody
         BEGIN SELECT RAISE(ABORT,'custody owner required'); END"
    ))
}

fn install(conn: &Connection) -> std::result::Result<(), CursorError> {
    conn.execute_batch(DDL)?;
    let predicate = EXACT_OUTBOX.replace("c.", "NEW.");
    for writer in [Writer::Insert, Writer::Update] {
        let verb = writer.verb();
        conn.execute_batch(&format!(
            "CREATE TRIGGER peer_custody_reference_{verb} BEFORE {verb} ON peer_pending_custody
             WHEN NOT EXISTS ({predicate})
             BEGIN SELECT RAISE(ABORT,'exact acknowledged Consent required'); END"
        ))?;
        install_guard(conn, verb)?;
    }
    install_guard(conn, "DELETE")?;
    Ok(())
}

fn expected() -> std::result::Result<&'static [Entry], CursorError> {
    static CUSTODY: OnceLock<Vec<Entry>> = OnceLock::new();
    if let Some(entries) = CUSTODY.get() {
        return Ok(entries);
    }
    let template = super::schema::template()?;
    install(&template)?;
    let entries = inventory(&template).map_err(super::schema::cursor_error)?;
    Ok(CUSTODY.get_or_init(|| entries))
}

#[derive(Clone, Copy)]
pub(crate) struct Epoch(());

impl Epoch {
    pub(crate) fn verify(
        self,
        tx: &Transaction<'_>,
        account: [u8; 32],
        origins: &[Origin],
    ) -> std::result::Result<(), CursorError> {
        verify_hardening(tx)?;
        if inventory(tx).map_err(super::schema::cursor_error)? != expected()? {
            return Err(CursorError::Binding);
        }
        super::schema::verify_rows(tx, account, origins)?;
        let marker: i64 = tx.query_row(
            "SELECT count(*) FROM peer_pending_custody_epoch WHERE epoch=2",
            [],
            |row| row.get(0),
        )?;
        let broken: bool = tx.query_row(
            &format!(
                "SELECT EXISTS(
                    SELECT 1 FROM peer_pending_custody c
                    WHERE c.account!=?1 OR NOT EXISTS ({EXACT_OUTBOX})
                )"
            ),
            [account],
            |row| row.get(0),
        )?;
        if marker != 1 || broken {
            return Err(CursorError::Binding);
        }
        Ok(())
    }
}

fn verify_hardening(conn: &Connection) -> std::result::Result<(), CursorError> {
    check_hardening(conn)?;
    let enforcing: i64 = conn.query_row("PRAGMA ignore_check_constraints", [], |row| row.get(0))?;
    if enforcing != 0 {
        return Err(CursorError::Storage);
    }
    Ok(())
}

pub(super) fn open(
    HardenedPeerConnection(mut conn): HardenedPeerConnection,
    account: [u8; 32],
    mut origins: Vec<Origin>,
) -> std::result::Result<(CursorStore, Epoch), CursorError> {
    if !(1..=MAX_ORIGINS).contains(&origins.len()) {
        return Err(CursorError::Binding);
    }
    origins.sort_unstable();
    verify_hardening(&conn)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if inventory(&tx).map_err(super::schema::cursor_error)? != expected()? {
        super::schema::verify(&tx, account, &origins)?;
        install(&tx)?;
    }
    let epoch = Epoch(());
    epoch.verify(&tx, account, &origins)?;
    tx.commit()?;
    Ok((CursorStore { conn, origins }, epoch))
}
