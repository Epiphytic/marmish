//! Fixture-only publication tables. Native storage retains the signed bytes.

use rusqlite::{Connection, Transaction};

use super::*;

const DDL: &str = "
CREATE TABLE peer_publication_epoch (epoch INTEGER PRIMARY KEY CHECK(epoch=4)) STRICT;
INSERT INTO peer_publication_epoch VALUES(4);

CREATE TABLE peer_activation_publication (
    carrier BLOB NOT NULL REFERENCES peer_pending_custody(delivery_carrier)
        ON UPDATE RESTRICT ON DELETE RESTRICT CHECK(length(carrier)=32),
    kind INTEGER NOT NULL CHECK(kind IN (0,1)),
    generation INTEGER NOT NULL CHECK(generation>0),
    group_id BLOB NOT NULL CHECK(length(group_id) BETWEEN 1 AND 255),
    route BLOB NOT NULL CHECK(length(route)=32),
    semantic_id BLOB NOT NULL CHECK(length(semantic_id) BETWEEN 1 AND 128
        AND (kind=0 OR length(semantic_id)=32)),
    source_epoch BLOB NOT NULL CHECK(length(source_epoch)=8),
    source_identity BLOB NOT NULL CHECK(length(source_identity)=32),
    source_leaf INTEGER NOT NULL CHECK(source_leaf BETWEEN 0 AND 4294967295),
    created_at INTEGER NOT NULL CHECK(created_at>0),
    changed_at INTEGER NOT NULL CHECK(changed_at>=created_at),
    phase INTEGER NOT NULL CHECK(phase BETWEEN 0 AND 5 AND (kind=1 OR phase!=1)),
    attempts INTEGER NOT NULL CHECK(
        (phase<=2 AND attempts=0) OR (phase>=3 AND attempts IN (1,2))),
    armed_at INTEGER CHECK(armed_at>=created_at AND armed_at<=changed_at),
    event_id BLOB UNIQUE CHECK(length(event_id)=32),
    signed_digest BLOB CHECK(length(signed_digest)=32),
    target_epoch BLOB CHECK(length(target_epoch)=8),
    target_identity BLOB CHECK(length(target_identity)=32),
    target_leaf INTEGER CHECK(target_leaf BETWEEN 0 AND 4294967295),
    CHECK((phase<3 AND armed_at IS NULL) OR (phase>=3 AND armed_at IS NOT NULL)),
    CHECK((phase<2 AND event_id IS NULL AND signed_digest IS NULL) OR
          (phase>=2 AND event_id IS NOT NULL AND signed_digest IS NOT NULL)),
    CHECK((kind=0 AND phase>=2 AND target_epoch IS NOT NULL AND target_identity IS NOT NULL
          AND target_leaf IS NOT NULL AND target_epoch>source_epoch
          AND target_identity!=source_identity AND target_leaf=source_leaf) OR
          ((kind=1 OR phase<2) AND target_epoch IS NULL AND target_identity IS NULL
          AND target_leaf IS NULL)),
    PRIMARY KEY(carrier,kind)
) STRICT;

CREATE UNIQUE INDEX peer_activation_single_postjoin
ON peer_activation_publication(kind) WHERE kind=0;

CREATE TABLE peer_activation_active (
    carrier BLOB PRIMARY KEY REFERENCES peer_pending_custody(delivery_carrier)
        ON UPDATE RESTRICT ON DELETE RESTRICT CHECK(length(carrier)=32),
    generation INTEGER NOT NULL CHECK(generation>0),
    activated_at INTEGER NOT NULL CHECK(activated_at>0),
    epoch BLOB NOT NULL CHECK(length(epoch)=8),
    identity BLOB NOT NULL CHECK(length(identity)=32),
    recipient_leaf INTEGER NOT NULL CHECK(recipient_leaf BETWEEN 0 AND 4294967295)
) STRICT;

CREATE TABLE peer_founder_enrollment (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    origin BLOB NOT NULL REFERENCES peer_cursor_origin(origin)
        ON UPDATE RESTRICT ON DELETE RESTRICT CHECK(length(origin)=32),
    founder BLOB NOT NULL CHECK(length(founder)=32),
    recipient BLOB NOT NULL CHECK(length(recipient)=32 AND recipient!=founder),
    expected BLOB NOT NULL CHECK(length(expected) BETWEEN 1 AND 2560),
    group_id BLOB NOT NULL CHECK(length(group_id) BETWEEN 1 AND 255),
    route BLOB NOT NULL CHECK(length(route)=32),
    recipient_leaf INTEGER NOT NULL CHECK(recipient_leaf BETWEEN 0 AND 4294967295),
    valid_from INTEGER NOT NULL CHECK(valid_from>0),
    deadline INTEGER NOT NULL CHECK(deadline>valid_from)
) STRICT;

CREATE TABLE peer_founder_acceptance (
    singleton INTEGER PRIMARY KEY REFERENCES peer_founder_enrollment(singleton)
        ON UPDATE RESTRICT ON DELETE RESTRICT CHECK(singleton=1),
    founder BLOB NOT NULL CHECK(length(founder)=32),
    recipient BLOB NOT NULL CHECK(length(recipient)=32 AND recipient!=founder),
    room_id BLOB NOT NULL CHECK(length(room_id)=16),
    offer_id BLOB NOT NULL CHECK(length(offer_id)=16),
    offer_revision BLOB NOT NULL CHECK(length(offer_revision)=32),
    welcome_id BLOB NOT NULL CHECK(length(welcome_id)=32),
    epoch BLOB NOT NULL CHECK(length(epoch)=8),
    identity BLOB NOT NULL CHECK(length(identity)=32),
    origin BLOB NOT NULL CHECK(length(origin)=32) REFERENCES peer_cursor_origin(origin)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    group_id BLOB NOT NULL CHECK(length(group_id) BETWEEN 1 AND 255),
    route BLOB NOT NULL CHECK(length(route)=32),
    sender_leaf INTEGER NOT NULL CHECK(sender_leaf BETWEEN 0 AND 4294967295),
    native_id BLOB NOT NULL CHECK(length(native_id) BETWEEN 1 AND 128),
    event_id BLOB NOT NULL CHECK(length(event_id)=32),
    authored_at INTEGER NOT NULL CHECK(authored_at>0),
    deadline INTEGER NOT NULL CHECK(deadline>authored_at),
    accepted_at INTEGER NOT NULL CHECK(accepted_at>=authored_at AND accepted_at<deadline)
) STRICT;

CREATE TRIGGER peer_publication_next BEFORE UPDATE ON peer_activation_publication
WHEN OLD.carrier IS NOT NEW.carrier OR OLD.kind IS NOT NEW.kind
    OR OLD.generation IS NOT NEW.generation OR OLD.group_id IS NOT NEW.group_id
    OR OLD.route IS NOT NEW.route OR OLD.semantic_id IS NOT NEW.semantic_id
    OR OLD.source_epoch IS NOT NEW.source_epoch OR OLD.source_identity IS NOT NEW.source_identity
    OR OLD.source_leaf IS NOT NEW.source_leaf OR OLD.created_at IS NOT NEW.created_at
    OR NEW.changed_at<OLD.changed_at
    OR (NEW.phase=3 AND NEW.armed_at IS NOT NEW.changed_at)
    OR (NEW.phase!=3 AND OLD.armed_at IS NOT NEW.armed_at)
    OR (OLD.phase>=2 AND (OLD.event_id IS NOT NEW.event_id OR OLD.signed_digest IS NOT NEW.signed_digest
        OR OLD.target_epoch IS NOT NEW.target_epoch OR OLD.target_identity IS NOT NEW.target_identity
        OR OLD.target_leaf IS NOT NEW.target_leaf))
    OR NOT ((OLD.phase=0 AND NEW.phase=1 AND OLD.kind=1) OR
            (NEW.phase=2 AND ((OLD.phase=0 AND OLD.kind=0) OR (OLD.phase=1 AND OLD.kind=1))) OR
            (OLD.phase=2 AND NEW.phase=3 AND NEW.attempts=1) OR
            (OLD.phase=3 AND NEW.phase=4 AND OLD.attempts=NEW.attempts) OR
            (OLD.phase=4 AND OLD.attempts=1 AND NEW.phase=3 AND NEW.attempts=2) OR
            (OLD.phase IN (3,4) AND NEW.phase=5 AND OLD.attempts=NEW.attempts))
BEGIN SELECT RAISE(ABORT,'exact publication transition required'); END;
";

const WRITES: [(Table, &str); 15] = [
    (Table::Epoch, "INSERT"),
    (Table::Epoch, "UPDATE"),
    (Table::Epoch, "DELETE"),
    (Table::Publication, "INSERT"),
    (Table::Publication, "UPDATE"),
    (Table::Publication, "DELETE"),
    (Table::Active, "INSERT"),
    (Table::Active, "DELETE"),
    (Table::Active, "UPDATE"),
    (Table::FounderEnrollment, "INSERT"),
    (Table::FounderEnrollment, "UPDATE"),
    (Table::FounderEnrollment, "DELETE"),
    (Table::Acceptance, "INSERT"),
    (Table::Acceptance, "UPDATE"),
    (Table::Acceptance, "DELETE"),
];

#[derive(Clone, Copy)]
pub(super) enum Table {
    Epoch,
    Publication,
    Active,
    FounderEnrollment,
    Acceptance,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Self::Epoch => "peer_publication_epoch",
            Self::Publication => "peer_activation_publication",
            Self::Active => "peer_activation_active",
            Self::FounderEnrollment => "peer_founder_enrollment",
            Self::Acceptance => "peer_founder_acceptance",
        }
    }

    fn install_guard(self, conn: &Connection, operation: &str) -> rusqlite::Result<()> {
        let table = self.name();
        conn.execute_batch(&format!(
            "CREATE TRIGGER {table}_deny_{operation} BEFORE {operation} ON {table}
             BEGIN SELECT RAISE(ABORT,'publication owner required'); END"
        ))
    }

    pub(super) fn checked_write<T>(
        self,
        tx: &Transaction<'_>,
        operation: &'static str,
        execute: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let table = self.name();
        tx.execute_batch(&format!("DROP TRIGGER {table}_deny_{operation}"))?;
        let result = execute()?;
        self.install_guard(tx, operation)?;
        Ok(result)
    }
}

pub(super) fn install(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(DDL)?;
    for (table, operation) in WRITES {
        table.install_guard(conn, operation)?;
    }
    Ok(())
}

pub(super) fn active_exists(tx: &Transaction<'_>, carrier: [u8; 32]) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM peer_activation_active WHERE carrier=?1)",
        [carrier],
        |row| row.get(0),
    )?)
}

pub(super) fn postjoin_exists(tx: &Transaction<'_>) -> Result<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM peer_activation_publication WHERE kind=0)",
        [],
        |row| row.get(0),
    )?)
}

pub(super) fn verify_rows(tx: &Transaction<'_>, account: [u8; 32]) -> Result<()> {
    let marker: i64 = tx.query_row(
        "SELECT count(*) FROM peer_publication_epoch WHERE epoch=4",
        [],
        |row| row.get(0),
    )?;
    let broken: bool = tx.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM peer_activation_publication p
            LEFT JOIN peer_pending_custody c ON p.carrier=c.delivery_carrier
            WHERE c.delivery_carrier IS NULL OR c.state!=3 OR p.generation!=c.generation
                OR p.group_id!=c.group_id OR p.created_at<c.activating_at
                OR p.changed_at>=c.pending_deadline
                OR EXISTS(SELECT 1 FROM peer_pending_terminal t
                          WHERE t.class=c.class AND t.generation=c.generation)
                OR (p.kind=1 AND NOT EXISTS (
                    SELECT 1 FROM peer_activation_active a WHERE a.carrier=p.carrier))
        ) OR EXISTS (
            SELECT 1 FROM peer_activation_active a
            LEFT JOIN peer_pending_custody c ON a.carrier=c.delivery_carrier
            WHERE c.delivery_carrier IS NULL OR c.state!=3 OR a.generation!=c.generation
                OR a.activated_at<c.activating_at OR a.activated_at>=c.pending_deadline
                OR NOT EXISTS(SELECT 1 FROM peer_activation_publication p
                    WHERE p.carrier=a.carrier AND p.kind=0 AND p.phase=5
                        AND a.epoch=p.target_epoch AND a.identity=p.target_identity
                        AND a.recipient_leaf=p.target_leaf AND a.activated_at>=p.changed_at)
                OR NOT EXISTS(SELECT 1 FROM peer_activation_publication p
                    WHERE p.carrier=a.carrier AND p.kind=1 AND p.created_at=a.activated_at
                        AND p.source_epoch=a.epoch AND p.source_identity=a.identity
                        AND p.source_leaf=a.recipient_leaf)
        ) OR EXISTS (SELECT 1 FROM peer_founder_enrollment WHERE founder!=?1)
          OR EXISTS (
            SELECT 1 FROM peer_founder_acceptance a
            LEFT JOIN peer_founder_enrollment e ON e.singleton=a.singleton AND e.origin=a.origin
            WHERE a.founder!=?1 OR e.singleton IS NULL OR a.founder!=e.founder
                OR a.recipient!=e.recipient OR a.group_id!=e.group_id OR a.route!=e.route
                OR a.sender_leaf!=e.recipient_leaf OR a.deadline!=e.deadline
                OR a.authored_at<e.valid_from
        )",
        [account],
        |row| row.get(0),
    )?;
    if marker != 1 || broken {
        return Err(CursorError::Binding.into());
    }
    Ok(())
}
