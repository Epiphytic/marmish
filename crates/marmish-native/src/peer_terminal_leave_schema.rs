//! Exact epoch-5 hold inventory. Every writer is closed outside its transaction.

use rusqlite::Connection;

use super::*;

const DDL: &str = "
CREATE TABLE peer_terminal_leave_epoch (epoch INTEGER PRIMARY KEY CHECK (epoch=5)) STRICT;
INSERT INTO peer_terminal_leave_epoch VALUES (5);
CREATE TRIGGER peer_terminal_leave_epoch_insert BEFORE INSERT ON peer_terminal_leave_epoch
BEGIN SELECT RAISE(ABORT,'immutable leave epoch'); END;
CREATE TRIGGER peer_terminal_leave_epoch_update BEFORE UPDATE ON peer_terminal_leave_epoch
BEGIN SELECT RAISE(ABORT,'immutable leave epoch'); END;
CREATE TRIGGER peer_terminal_leave_epoch_delete BEFORE DELETE ON peer_terminal_leave_epoch
BEGIN SELECT RAISE(ABORT,'immutable leave epoch'); END;

CREATE TABLE peer_terminal_native_leave (
    class INTEGER NOT NULL CHECK (class IN (0,1)) UNIQUE,
    generation INTEGER NOT NULL CHECK (generation>0),
    origin BLOB NOT NULL CHECK (length(origin)=32),
    account BLOB NOT NULL CHECK (length(account)=32),
    founder BLOB NOT NULL CHECK (length(founder)=32 AND founder!=account),
    offer_id BLOB NOT NULL CHECK (length(offer_id)=16),
    offer_revision BLOB NOT NULL CHECK (length(offer_revision)=32),
    consent_carrier BLOB NOT NULL CHECK (length(consent_carrier)=32),
    consent_revision BLOB NOT NULL CHECK (length(consent_revision)=32),
    delivery_carrier BLOB NOT NULL CHECK (length(delivery_carrier)=32),
    delivery_digest BLOB NOT NULL CHECK (length(delivery_digest)=32),
    welcome_id BLOB NOT NULL CHECK (length(welcome_id)=32),
    welcome_digest BLOB NOT NULL CHECK (length(welcome_digest)=32),
    device BLOB NOT NULL CHECK (length(device)=8),
    inode BLOB NOT NULL CHECK (length(inode)=8),
    group_id BLOB NOT NULL CHECK (length(group_id) BETWEEN 1 AND 255),
    epoch_number INTEGER NOT NULL CHECK (epoch_number>=0),
    epoch_identity BLOB NOT NULL CHECK (length(epoch_identity)=32),
    own_leaf INTEGER NOT NULL CHECK (own_leaf BETWEEN 0 AND 4294967295),
    route BLOB NOT NULL CHECK (length(route)=32),
    hold_deadline INTEGER NOT NULL CHECK (hold_deadline>0),
    recovery_state INTEGER NOT NULL CHECK (recovery_state IN (0,1,2)),
    recovery_attempts INTEGER NOT NULL CHECK (recovery_attempts BETWEEN 0 AND 3),
    recovery_reserved_at INTEGER CHECK (recovery_reserved_at>0),
    recovery_not_before INTEGER NOT NULL CHECK (recovery_not_before>0),
    PRIMARY KEY (class,generation),
    FOREIGN KEY (class,generation) REFERENCES peer_pending_terminal(class,generation)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CHECK (
        (recovery_state=0 AND recovery_attempts=0 AND recovery_reserved_at IS NULL)
        OR (recovery_state=1 AND recovery_attempts IN (1,2,3) AND recovery_reserved_at IS NOT NULL)
        OR (recovery_state=2 AND recovery_attempts=3 AND recovery_reserved_at IS NOT NULL)
    )
) STRICT;
CREATE TRIGGER peer_terminal_leave_progress BEFORE UPDATE ON peer_terminal_native_leave
WHEN OLD.class IS NOT NEW.class OR OLD.generation IS NOT NEW.generation
    OR OLD.origin IS NOT NEW.origin OR OLD.account IS NOT NEW.account OR OLD.founder IS NOT NEW.founder
    OR OLD.offer_id IS NOT NEW.offer_id OR OLD.offer_revision IS NOT NEW.offer_revision
    OR OLD.consent_carrier IS NOT NEW.consent_carrier OR OLD.consent_revision IS NOT NEW.consent_revision
    OR OLD.delivery_carrier IS NOT NEW.delivery_carrier OR OLD.delivery_digest IS NOT NEW.delivery_digest
    OR OLD.welcome_id IS NOT NEW.welcome_id OR OLD.welcome_digest IS NOT NEW.welcome_digest
    OR OLD.device IS NOT NEW.device OR OLD.inode IS NOT NEW.inode
    OR OLD.group_id IS NOT NEW.group_id OR OLD.epoch_number IS NOT NEW.epoch_number
    OR OLD.epoch_identity IS NOT NEW.epoch_identity OR OLD.own_leaf IS NOT NEW.own_leaf
    OR OLD.route IS NOT NEW.route OR OLD.hold_deadline IS NOT NEW.hold_deadline
    OR NOT (
        (OLD.recovery_state IN (0,1) AND OLD.recovery_attempts<3 AND NEW.recovery_state=1
            AND NEW.recovery_attempts=OLD.recovery_attempts+1
            AND NEW.recovery_reserved_at>=OLD.recovery_not_before
            AND NEW.recovery_reserved_at=(SELECT wall FROM peer_cursor_origin WHERE origin=OLD.origin)
            AND CASE NEW.recovery_attempts
                WHEN 1 THEN NEW.recovery_not_before-NEW.recovery_reserved_at=1
                WHEN 2 THEN NEW.recovery_not_before-NEW.recovery_reserved_at=2
                WHEN 3 THEN NEW.recovery_not_before=OLD.recovery_not_before END)
        OR (OLD.recovery_state=1 AND NEW.recovery_state=2 AND OLD.recovery_attempts=3
            AND OLD.recovery_attempts IS NEW.recovery_attempts
            AND OLD.recovery_reserved_at IS NEW.recovery_reserved_at
            AND OLD.recovery_not_before IS NEW.recovery_not_before
            AND (SELECT wall FROM peer_cursor_origin WHERE origin=OLD.origin)>=OLD.recovery_not_before)
    )
BEGIN SELECT RAISE(ABORT,'exact leave recovery transition required'); END;
CREATE TRIGGER peer_terminal_leave_fence BEFORE UPDATE ON peer_pending_terminal
WHEN EXISTS (SELECT 1 FROM peer_terminal_native_leave h
             WHERE h.class=OLD.class AND h.generation=OLD.generation)
BEGIN SELECT RAISE(ABORT,'terminal leave hold requires reconciled authority'); END;
";

/// Match every coordinate supplied by a retained row to its parent decision
/// and the exact joined Pending custody. The authenticated route is separately
/// checked from the retained signed Offer by the journal owner.
const EXACT_PAIR: &str = "
SELECT 1 FROM peer_pending_terminal t
JOIN peer_pending_custody c ON c.class=t.class AND c.generation=t.generation
WHERE t.class={hold}.class AND t.generation={hold}.generation AND t.step=0 AND c.state=2
    AND t.origin={hold}.origin AND t.account={hold}.account AND t.server={hold}.founder
    AND t.offer_id={hold}.offer_id AND t.offer_revision={hold}.offer_revision
    AND t.consent_carrier={hold}.consent_carrier AND c.consent_revision={hold}.consent_revision
    AND t.delivery_carrier={hold}.delivery_carrier AND t.delivery_digest={hold}.delivery_digest
    AND c.welcome_id={hold}.welcome_id AND t.welcome_digest={hold}.welcome_digest
    AND t.device={hold}.device AND t.inode={hold}.inode
    AND t.group_id={hold}.group_id AND t.epoch_number={hold}.epoch_number
    AND t.epoch_identity={hold}.epoch_identity AND t.own_leaf={hold}.own_leaf
    AND t.decided_at<=9223372036854689407
    AND {hold}.hold_deadline=t.decided_at+86400 AND {hold}.hold_deadline>t.decided_at
    AND {hold}.recovery_not_before>=t.decided_at
    AND ({hold}.recovery_reserved_at IS NULL OR {hold}.recovery_reserved_at>=t.decided_at)
    AND CASE {hold}.recovery_attempts
        WHEN 0 THEN {hold}.recovery_not_before=t.decided_at
        WHEN 1 THEN {hold}.recovery_not_before-{hold}.recovery_reserved_at=1
        WHEN 2 THEN {hold}.recovery_not_before-{hold}.recovery_reserved_at=2
        WHEN 3 THEN {hold}.recovery_not_before<={hold}.recovery_reserved_at END
";

#[derive(Clone, Copy)]
pub(super) enum Writer {
    Insert,
    Update,
}

impl Writer {
    pub(super) fn guard(self, conn: &Connection, install: bool) -> Result<()> {
        let operation = match self {
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
        };
        if install {
            deny(conn, operation)?;
        } else {
            conn.execute_batch(&format!(
                "DROP TRIGGER peer_terminal_leave_deny_{operation}"
            ))?;
        }
        Ok(())
    }
}

fn deny(conn: &Connection, operation: &str) -> rusqlite::Result<()> {
    conn.execute_batch(&format!(
        "CREATE TRIGGER peer_terminal_leave_deny_{operation}
         BEFORE {operation} ON peer_terminal_native_leave
         BEGIN SELECT RAISE(ABORT,'terminal leave owner required'); END"
    ))
}

pub(super) fn install(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(DDL)?;
    conn.execute_batch(&format!(
        "CREATE TRIGGER peer_terminal_leave_insert BEFORE INSERT ON peer_terminal_native_leave
         WHEN NEW.recovery_state!=0 OR NEW.recovery_not_before IS NOT
            (SELECT t.decided_at FROM peer_pending_terminal t
             WHERE t.class=NEW.class AND t.generation=NEW.generation)
            OR NOT EXISTS ({})
         BEGIN SELECT RAISE(ABORT,'exact terminal leave hold required'); END",
        EXACT_PAIR.replace("{hold}", "NEW")
    ))?;
    for operation in ["INSERT", "UPDATE", "DELETE"] {
        deny(conn, operation)?;
    }
    Ok(())
}

pub(super) fn verify_rows(tx: &Transaction<'_>, account: [u8; 32]) -> Result<()> {
    let pair = EXACT_PAIR.replace("{hold}", "h");
    let matches: bool = tx.query_row(
        &format!(
            "SELECT (SELECT count(*) FROM peer_terminal_leave_epoch WHERE epoch=5)=1
             AND NOT EXISTS (
                SELECT 1 FROM peer_terminal_native_leave h
                WHERE h.account!=?1 OR NOT EXISTS ({pair})
                  OR NOT EXISTS (SELECT 1 FROM peer_cursor_origin o WHERE o.origin=h.origin
                                 AND (h.recovery_reserved_at IS NULL OR o.wall>=h.recovery_reserved_at))
             )"
        ),
        [account],
        |row| row.get(0),
    )?;
    if !matches {
        return Err(CursorError::Binding.into());
    }
    Ok(())
}
