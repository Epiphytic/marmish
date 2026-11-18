//! Terminal decisions and the no-egress joined retirement journal.

use rusqlite::Connection;

use super::*;

const TABLE: &str = "
CREATE TABLE peer_pending_terminal (
    class INTEGER NOT NULL CHECK (class IN (0,1)),
    generation INTEGER NOT NULL CHECK (generation>0),
    account BLOB NOT NULL CHECK (length(account)=32),
    server BLOB NOT NULL CHECK (length(server)=32 AND server!=account),
    origin BLOB NOT NULL CHECK (length(origin)=32),
    offer_id BLOB NOT NULL CHECK (length(offer_id)=16),
    offer_revision BLOB NOT NULL CHECK (length(offer_revision)=32),
    consent_carrier BLOB NOT NULL CHECK (length(consent_carrier)=32),
    delivery_carrier BLOB NOT NULL UNIQUE CHECK (length(delivery_carrier)=32),
    delivery_digest BLOB NOT NULL CHECK (length(delivery_digest)=32),
    outer_stamp INTEGER NOT NULL CHECK (outer_stamp>0),
    welcome_digest BLOB NOT NULL CHECK (length(welcome_digest)=32),
    device BLOB NOT NULL CHECK (length(device)=8),
    inode BLOB NOT NULL CHECK (length(inode)=8),
    group_id BLOB NOT NULL CHECK (length(group_id) BETWEEN 1 AND 255),
    epoch_number INTEGER NOT NULL CHECK (epoch_number>=0),
    epoch_identity BLOB NOT NULL CHECK (length(epoch_identity)=32),
    own_leaf INTEGER NOT NULL CHECK (own_leaf BETWEEN 0 AND 4294967295),
    qualified_at INTEGER NOT NULL CHECK (qualified_at>0),
    pending_deadline INTEGER NOT NULL CHECK (pending_deadline>qualified_at),
    replay_until INTEGER NOT NULL CHECK (replay_until>pending_deadline),
    offer_replay_until INTEGER NOT NULL CHECK (offer_replay_until>0),
    reason INTEGER NOT NULL CHECK (reason IN (0,1)),
    decided_at INTEGER NOT NULL CHECK (decided_at>=qualified_at),
    step INTEGER NOT NULL CHECK (step IN (0,1,2,3,4)),
    outcome INTEGER CHECK (outcome=0),
    witness BLOB CHECK (length(witness)=48 AND substr(witness,33,8)=device AND substr(witness,41,8)=inode),
    retain_until INTEGER,
    PRIMARY KEY (class,generation),
    CHECK ((reason=0 AND decided_at<pending_deadline) OR
           (reason=1 AND decided_at>=pending_deadline)),
    CHECK (
        (step=0 AND outcome IS NULL AND witness IS NULL AND retain_until IS NULL)
        OR (step IN (1,2) AND outcome IS NOT NULL AND witness IS NOT NULL AND retain_until IS NULL)
        OR (step=3 AND outcome IS NOT NULL AND witness IS NULL AND retain_until>decided_at
            AND retain_until>=replay_until AND retain_until>=offer_replay_until)
        OR (step=4 AND witness IS NULL AND retain_until IS NULL)
    )
) STRICT;
CREATE UNIQUE INDEX peer_pending_terminal_charged ON peer_pending_terminal(class)
WHERE step!=3;
CREATE TRIGGER peer_terminal_freeze_stage BEFORE UPDATE ON peer_provisional_stage_v1
WHEN EXISTS (
    SELECT 1 FROM peer_pending_terminal t
    WHERE t.class=OLD.class AND t.generation=OLD.generation AND t.step!=3
)
BEGIN SELECT RAISE(ABORT,'joined stage is terminal'); END;
CREATE TRIGGER peer_terminal_progress BEFORE UPDATE ON peer_pending_terminal
WHEN OLD.class IS NOT NEW.class OR OLD.generation IS NOT NEW.generation
    OR OLD.account IS NOT NEW.account OR OLD.server IS NOT NEW.server OR OLD.origin IS NOT NEW.origin
    OR OLD.offer_id IS NOT NEW.offer_id OR OLD.offer_revision IS NOT NEW.offer_revision
    OR OLD.consent_carrier IS NOT NEW.consent_carrier OR OLD.delivery_carrier IS NOT NEW.delivery_carrier
    OR OLD.delivery_digest IS NOT NEW.delivery_digest OR OLD.outer_stamp IS NOT NEW.outer_stamp
    OR OLD.welcome_digest IS NOT NEW.welcome_digest OR OLD.device IS NOT NEW.device OR OLD.inode IS NOT NEW.inode
    OR OLD.group_id IS NOT NEW.group_id OR OLD.epoch_number IS NOT NEW.epoch_number
    OR OLD.epoch_identity IS NOT NEW.epoch_identity OR OLD.own_leaf IS NOT NEW.own_leaf
    OR OLD.qualified_at IS NOT NEW.qualified_at OR OLD.pending_deadline IS NOT NEW.pending_deadline
    OR OLD.replay_until IS NOT NEW.replay_until OR OLD.offer_replay_until IS NOT NEW.offer_replay_until
    OR OLD.reason IS NOT NEW.reason OR OLD.decided_at IS NOT NEW.decided_at
    OR (OLD.outcome IS NOT NULL AND OLD.outcome IS NOT NEW.outcome)
    OR NOT (
        (OLD.step=0 AND NEW.step=1)
        OR (OLD.step=1 AND NEW.step=2 AND OLD.witness IS NEW.witness)
        OR (OLD.step=2 AND NEW.step=3)
        OR (OLD.step IN (0,1) AND NEW.step=4 AND OLD.outcome IS NEW.outcome)
    )
BEGIN SELECT RAISE(ABORT,'exact terminal progress required'); END;
CREATE TRIGGER peer_terminal_prune BEFORE DELETE ON peer_pending_terminal
WHEN OLD.step!=3 OR OLD.retain_until>(SELECT wall FROM peer_cursor_origin WHERE origin=OLD.origin)
BEGIN SELECT RAISE(ABORT,'terminal history is protected'); END;
";

const LIVE_PAIR: &str = "
    SELECT 1 FROM peer_pending_custody c
    JOIN peer_provisioning_slot p ON p.class=c.class AND p.generation=c.generation
        AND p.server=c.server AND p.offer_id=c.offer_id AND p.revision=c.offer_revision AND p.origin=c.origin
    JOIN peer_provisional_stage_v1 s ON s.class=p.class AND s.generation=p.generation
        AND s.server=p.server AND s.offer_id=p.offer_id AND s.revision=p.revision AND s.origin=p.origin
    JOIN peer_consent_invite i ON i.server=p.server AND i.offer_id=p.offer_id
        AND i.revision=p.revision AND i.origin=p.origin
    WHERE c.state=2 AND s.state=2 AND c.class=t.class AND c.generation=t.generation
        AND c.account=t.account AND c.server=t.server AND c.origin=t.origin
        AND c.offer_id=t.offer_id AND c.offer_revision=t.offer_revision
        AND c.consent_carrier=t.consent_carrier AND c.delivery_carrier=t.delivery_carrier
        AND c.outer_stamp=t.outer_stamp AND c.welcome_digest=t.welcome_digest
        AND s.device=t.device AND s.inode=t.inode AND c.group_id=t.group_id
        AND c.epoch_number=t.epoch_number AND c.epoch_identity=t.epoch_identity AND c.own_leaf=t.own_leaf
        AND c.qualified_at=t.qualified_at AND c.pending_deadline=t.pending_deadline
        AND c.replay_until=t.replay_until AND i.replay_until=t.offer_replay_until
";

const LIVE_ASSOCIATION: &str = "
    SELECT 1 FROM peer_pending_custody c
        WHERE (c.class=t.class AND c.generation=t.generation) OR c.delivery_carrier=t.delivery_carrier
    UNION ALL SELECT 1 FROM peer_prepared_consent_outbox o WHERE o.carrier=t.consent_carrier
    UNION ALL SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.class=t.class AND s.generation=t.generation
    UNION ALL SELECT 1 FROM peer_provisioning_slot p
        WHERE p.class=t.class AND p.generation=t.generation AND p.server IS NOT NULL
    UNION ALL SELECT 1 FROM peer_consent_invite i
        WHERE i.server=t.server AND i.offer_id=t.offer_id AND i.revision=t.offer_revision AND i.origin=t.origin
";

#[derive(Clone, Copy)]
pub(super) enum Writer {
    Insert,
    Progress,
    Prune,
}

impl Writer {
    pub(super) fn guard(self, conn: &Connection, install: bool) -> Result<()> {
        let verb = match self {
            Self::Insert => "INSERT",
            Self::Progress => "UPDATE",
            Self::Prune => "DELETE",
        };
        if install {
            conn.execute_batch(&format!(
                "CREATE TRIGGER peer_terminal_deny_{verb} BEFORE {verb} ON peer_pending_terminal
                 BEGIN SELECT RAISE(ABORT,'terminal custody owner required'); END"
            ))?;
        } else {
            conn.execute_batch(&format!("DROP TRIGGER peer_terminal_deny_{verb}"))?;
        }
        Ok(())
    }
}

pub(super) fn install(conn: &Connection) -> Result<()> {
    conn.execute_batch(TABLE)?;
    conn.execute_batch(&format!(
        "CREATE TRIGGER peer_terminal_insert BEFORE INSERT ON peer_pending_terminal
         WHEN NEW.step!=0 OR NOT EXISTS ({})
         BEGIN SELECT RAISE(ABORT,'exact joined Pending required'); END",
        LIVE_PAIR.replace("t.", "NEW.")
    ))?;
    conn.execute_batch(&format!(
        "CREATE TRIGGER peer_terminal_released BEFORE UPDATE ON peer_pending_terminal
         WHEN NEW.step=3 AND EXISTS ({})
         BEGIN SELECT RAISE(ABORT,'terminal release retains live rows'); END",
        LIVE_ASSOCIATION.replace("t.", "NEW.")
    ))?;
    for writer in [Writer::Insert, Writer::Progress, Writer::Prune] {
        writer.guard(conn, true)?;
    }
    Ok(())
}

pub(super) fn verify_rows(tx: &Transaction<'_>, account: [u8; 32]) -> Result<()> {
    let broken: bool = tx.query_row(
        &format!(
            "SELECT (SELECT count(*) FROM peer_pending_terminal)>?2
               OR (SELECT count(*) FROM peer_retirement_cooldown)>?2
               OR EXISTS (
                SELECT 1 FROM peer_pending_terminal t WHERE t.account!=?1
                    OR NOT EXISTS (SELECT 1 FROM peer_cursor_origin o WHERE o.origin=t.origin)
                    OR NOT EXISTS (SELECT 1 FROM peer_provisioning_slot p
                        WHERE p.class=t.class AND p.generation>=t.generation)
                    OR (t.step!=3 AND NOT EXISTS ({LIVE_PAIR}))
                    OR (t.step=3 AND EXISTS ({LIVE_ASSOCIATION}))
            )"
        ),
        params![account, super::super::super::cooldown::HISTORY_LIMIT],
        |row| row.get(0),
    )?;
    if broken {
        return Err(CursorError::Binding.into());
    }
    Ok(())
}
