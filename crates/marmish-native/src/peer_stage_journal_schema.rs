//! Strict schema for the test-only provisional native stage.

use rusqlite::Transaction;

use super::{CLAIM_TRIGGER, CursorError, schema_entry};

const TABLE: &str = "
CREATE TABLE peer_provisional_stage_v1 (
    class INTEGER PRIMARY KEY CHECK (class IN (0, 1)),
    generation INTEGER NOT NULL CHECK (generation > 0),
    server BLOB NOT NULL CHECK (length(server) = 32),
    offer_id BLOB NOT NULL CHECK (length(offer_id) = 16),
    revision BLOB NOT NULL CHECK (length(revision) = 32),
    origin BLOB NOT NULL CHECK (length(origin) = 32),
    state INTEGER NOT NULL CHECK (state IN (0, 1, 2)),
    device BLOB CHECK (length(device) = 8),
    inode BLOB CHECK (length(inode) = 8),
    package_id BLOB CHECK (length(package_id) = 32),
    package_digest BLOB CHECK (length(package_digest) = 32),
    package_expires INTEGER CHECK (package_expires > 0),
    CHECK ((state = 0 AND device IS NULL AND inode IS NULL)
        OR (state IN (1, 2) AND device IS NOT NULL AND inode IS NOT NULL)),
    CHECK ((state IN (0, 1) AND package_id IS NULL AND package_digest IS NULL
            AND package_expires IS NULL)
        OR (state = 2 AND package_id IS NOT NULL AND package_digest IS NOT NULL
            AND package_expires IS NOT NULL))
) STRICT";

// Older builds expect the byte-identical v2 claim trigger and reject this schema.
pub(super) const STAGE_CLAIM_TRIGGER: &str = "
CREATE TRIGGER peer_provisioning_reject_claim BEFORE UPDATE OF server ON peer_provisioning_slot
WHEN EXISTS (
    SELECT 1 FROM peer_consent_outbox o WHERE o.server=NEW.server
        AND o.offer_id=NEW.offer_id AND o.offer_revision=NEW.revision AND o.origin=NEW.origin
)
BEGIN
    SELECT RAISE(ABORT, 'provisioning claim conflict'); -- provisional stage v1
END";

const INSERT_STAGE: &str = "
CREATE TRIGGER peer_stage_insert_guard BEFORE INSERT ON peer_provisional_stage_v1
WHEN NEW.state != 0
    OR EXISTS (SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.class=NEW.class)
    OR NOT EXISTS (
        SELECT 1 FROM peer_provisioning_slot p
        JOIN peer_consent_invite i ON i.server=p.server AND i.offer_id=p.offer_id
            AND i.revision=p.revision AND i.origin=p.origin
        WHERE p.class=NEW.class AND p.generation=NEW.generation AND p.server=NEW.server
            AND p.offer_id=NEW.offer_id AND p.revision=NEW.revision AND p.origin=NEW.origin
    )
BEGIN SELECT RAISE(ABORT, 'stage requires exact new claim'); END";

const UPDATE_STAGE: &str = "
CREATE TRIGGER peer_stage_update_guard BEFORE UPDATE ON peer_provisional_stage_v1
WHEN NEW.class IS NOT OLD.class OR NEW.generation IS NOT OLD.generation
    OR NEW.server IS NOT OLD.server OR NEW.offer_id IS NOT OLD.offer_id
    OR NEW.revision IS NOT OLD.revision OR NEW.origin IS NOT OLD.origin
    OR NEW.state != OLD.state + 1
    OR (OLD.state != 0 AND (NEW.device IS NOT OLD.device OR NEW.inode IS NOT OLD.inode))
BEGIN SELECT RAISE(ABORT, 'stage state is monotonic'); END";

pub(super) const DELETE_STAGE: &str = "
CREATE TRIGGER peer_stage_delete_guard BEFORE DELETE ON peer_provisional_stage_v1
BEGIN SELECT RAISE(ABORT, 'stage is bound'); END";

const INSERT_SLOT: &str = "
CREATE TRIGGER peer_stage_slot_insert_guard BEFORE INSERT ON peer_provisioning_slot
WHEN EXISTS (
    SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.class=NEW.class OR s.server=NEW.server
)
BEGIN SELECT RAISE(ABORT, 'slot is bound'); END";

const UPDATE_SLOT: &str = "
CREATE TRIGGER peer_stage_slot_update_guard BEFORE UPDATE ON peer_provisioning_slot
WHEN EXISTS (
    SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.class=OLD.class
        OR s.class=NEW.class OR s.server=NEW.server
)
BEGIN SELECT RAISE(ABORT, 'slot is bound'); END";

const DELETE_SLOT: &str = "
CREATE TRIGGER peer_stage_slot_delete_guard BEFORE DELETE ON peer_provisioning_slot
WHEN EXISTS (SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.class=OLD.class)
BEGIN SELECT RAISE(ABORT, 'slot is bound'); END";

const INSERT_INVITE: &str = "
CREATE TRIGGER peer_stage_invite_insert_guard BEFORE INSERT ON peer_consent_invite
WHEN EXISTS (SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.server=NEW.server)
BEGIN SELECT RAISE(ABORT, 'invite is bound'); END";

const UPDATE_INVITE: &str = "
CREATE TRIGGER peer_stage_invite_update_guard BEFORE UPDATE ON peer_consent_invite
WHEN EXISTS (
    SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.server=OLD.server OR s.server=NEW.server
)
BEGIN SELECT RAISE(ABORT, 'invite is bound'); END";

const DELETE_INVITE: &str = "
CREATE TRIGGER peer_stage_invite_delete_guard BEFORE DELETE ON peer_consent_invite
WHEN EXISTS (SELECT 1 FROM peer_provisional_stage_v1 s WHERE s.server=OLD.server)
BEGIN SELECT RAISE(ABORT, 'invite is bound'); END";

pub(super) const SCHEMA: &[(&str, &str)] = &[
    ("peer_provisional_stage_v1", TABLE),
    ("peer_stage_insert_guard", INSERT_STAGE),
    ("peer_stage_update_guard", UPDATE_STAGE),
    ("peer_stage_delete_guard", DELETE_STAGE),
    ("peer_stage_slot_insert_guard", INSERT_SLOT),
    ("peer_stage_slot_update_guard", UPDATE_SLOT),
    ("peer_stage_slot_delete_guard", DELETE_SLOT),
    ("peer_stage_invite_insert_guard", INSERT_INVITE),
    ("peer_stage_invite_update_guard", UPDATE_INVITE),
    ("peer_stage_invite_delete_guard", DELETE_INVITE),
];

pub(super) fn attach(tx: &Transaction<'_>, has_checkpoint: bool) -> Result<(), CursorError> {
    let marker = schema_entry(tx, "peer_provisioning_reject_claim")?;
    if marker.as_deref() == Some(STAGE_CLAIM_TRIGGER.trim()) {
        super::attach_with_claim_guard(tx, has_checkpoint, STAGE_CLAIM_TRIGGER)?;
        for (name, sql) in SCHEMA {
            if schema_entry(tx, name)?.as_deref() != Some(sql.trim()) {
                return Err(CursorError::Binding);
            }
        }
        return Ok(());
    }

    for (name, _) in SCHEMA {
        if schema_entry(tx, name)?.is_some() {
            return Err(CursorError::Binding);
        }
    }
    super::attach_with_claim_guard(tx, has_checkpoint, CLAIM_TRIGGER)?;
    tx.execute_batch("DROP TRIGGER peer_provisioning_reject_claim")?;
    tx.execute_batch(STAGE_CLAIM_TRIGGER)?;
    for (_, sql) in SCHEMA {
        tx.execute_batch(sql)?;
    }
    Ok(())
}
