//! Storage-only slot ownership for the provisional Peer lifecycle.

use std::sync::Arc;

use rusqlite::{OptionalExtension, Transaction, params};

use super::super::{read_seconds, seconds};
use super::{
    Checkpoint, CursorError, Error, Invitation, Local, Origin, Result, Selector, Source, rows,
};

#[cfg(test)]
#[path = "peer_stage_journal_schema.rs"]
mod stage_schema;

#[cfg(test)]
#[path = "peer_stage_journal.rs"]
pub(crate) mod stage_journal;

#[path = "peer_retirement_cooldown.rs"]
pub(super) mod cooldown;

#[cfg(test)]
#[path = "peer_stage_retirement_journal.rs"]
pub(crate) mod retirement;

#[cfg(test)]
#[path = "peer_prepared_consent.rs"]
pub(crate) mod prepared_consent;

const START_INTERVAL: u64 = 600;

// Keep persisted SQL stable. A schema change needs an explicit migration.
const SLOT_TABLE: &str = "
CREATE TABLE peer_provisioning_slot (
    class INTEGER PRIMARY KEY CHECK (class IN (0, 1)),
    generation INTEGER NOT NULL DEFAULT 0 CHECK (generation >= 0),
    next_start_at INTEGER NOT NULL DEFAULT 0 CHECK (next_start_at >= 0),
    server BLOB UNIQUE,
    offer_id BLOB,
    revision BLOB,
    origin BLOB,
    CHECK ((generation = 0 AND next_start_at = 0)
        OR (generation > 0 AND next_start_at >= 600)),
    CHECK ((server IS NULL AND offer_id IS NULL AND revision IS NULL AND origin IS NULL)
        OR (server IS NOT NULL AND length(server) = 32
            AND offer_id IS NOT NULL AND length(offer_id) = 16
            AND revision IS NOT NULL AND length(revision) = 32
            AND origin IS NOT NULL AND length(origin) = 32 AND generation > 0)),
    FOREIGN KEY (server, offer_id, revision, origin)
        REFERENCES peer_consent_invite (server, offer_id, revision, origin) ON DELETE RESTRICT
) STRICT";

const OUTBOX_TRIGGER: &str = "
CREATE TRIGGER peer_provisioning_reject_outbox BEFORE INSERT ON peer_consent_outbox
WHEN EXISTS (
    SELECT 1 FROM peer_provisioning_slot p WHERE p.server=NEW.server
        AND p.offer_id=NEW.offer_id AND p.revision=NEW.offer_revision AND p.origin=NEW.origin
)
BEGIN SELECT RAISE(ABORT, 'provisioning outbox conflict'); END";

const CLAIM_TRIGGER: &str = "
CREATE TRIGGER peer_provisioning_reject_claim BEFORE UPDATE OF server ON peer_provisioning_slot
WHEN EXISTS (
    SELECT 1 FROM peer_consent_outbox o WHERE o.server=NEW.server
        AND o.offer_id=NEW.offer_id AND o.offer_revision=NEW.revision AND o.origin=NEW.origin
)
BEGIN SELECT RAISE(ABORT, 'provisioning claim conflict'); END";

pub(super) fn attach(
    tx: &Transaction<'_>,
    has_checkpoint: bool,
) -> std::result::Result<(), CursorError> {
    attach_with_claim_guard(tx, has_checkpoint, CLAIM_TRIGGER)
}

fn attach_with_claim_guard(
    tx: &Transaction<'_>,
    has_checkpoint: bool,
    claim_trigger: &str,
) -> std::result::Result<(), CursorError> {
    let schema = schema_entry(tx, "peer_provisioning_slot")?;
    match schema.as_deref() {
        None if schema_entry(tx, "peer_provisioning_reject_outbox")?.is_none()
            && schema_entry(tx, "peer_provisioning_reject_claim")?.is_none() =>
        {
            for statement in [SLOT_TABLE, OUTBOX_TRIGGER, claim_trigger] {
                tx.execute_batch(statement)?;
            }
            tx.execute(
                "INSERT INTO peer_provisioning_slot (class) VALUES (0), (1)",
                [],
            )?;
        }
        Some(actual) if has_checkpoint && actual == SLOT_TABLE.trim() => {
            for (name, expected) in [
                ("peer_provisioning_reject_outbox", OUTBOX_TRIGGER),
                ("peer_provisioning_reject_claim", claim_trigger),
            ] {
                if schema_entry(tx, name)?.as_deref() != Some(expected.trim()) {
                    return Err(CursorError::Binding);
                }
            }
        }
        _ => return Err(CursorError::Binding),
    }
    let classes: u8 = tx.query_row(
        "SELECT count(*) FROM peer_provisioning_slot WHERE class IN (0,1)",
        [],
        |r| r.get(0),
    )?;
    if classes != 2 {
        return Err(CursorError::Binding);
    }
    Ok(())
}

fn schema_entry(tx: &Transaction<'_>, name: &str) -> rusqlite::Result<Option<String>> {
    tx.query_row("SELECT sql FROM sqlite_schema WHERE name=?1", [name], |r| {
        r.get(0)
    })
    .optional()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Generation {
    pub(super) known: bool,
    pub(super) count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(super) key: Selector,
    pub(super) origin: Origin,
    pub(super) generation: Generation,
}

#[cfg(test)]
impl Binding {
    pub(crate) fn coordinates(self) -> (bool, u64, Origin, Selector) {
        (
            self.generation.known,
            self.generation.count,
            self.origin,
            self.key,
        )
    }
}

#[derive(Debug)]
pub(crate) struct Reservation {
    pub(super) owner: Arc<()>,
    pub(super) binding: Binding,
}

pub(crate) struct RetirementTarget {
    owner: Arc<()>,
    binding: Binding,
}

fn generation(
    conn: &rusqlite::Connection,
    key: Selector,
    origin: Origin,
) -> rusqlite::Result<Option<Generation>> {
    conn.query_row(
        "SELECT class, generation FROM peer_provisioning_slot
            WHERE server=?1 AND offer_id=?2 AND revision=?3 AND origin=?4",
        params![key.server.to_bytes(), key.id, key.revision, origin],
        |r| {
            Ok(Generation {
                known: r.get(0)?,
                count: read_seconds(r, 1)?,
            })
        },
    )
    .optional()
}

fn has_outbox(tx: &Transaction<'_>, key: Selector, origin: Origin) -> rusqlite::Result<bool> {
    tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM peer_consent_outbox
            WHERE server=?1 AND offer_id=?2 AND offer_revision=?3 AND origin=?4)",
        params![key.server.to_bytes(), key.id, key.revision, origin],
        |r| r.get(0),
    )
}

impl Invitation {
    pub(super) fn load_unowned(
        tx: &Transaction<'_>,
        key: Selector,
        origin: Origin,
    ) -> Result<Self> {
        let invitation = Self::load(tx, key, origin)?;
        if generation(tx, key, origin)?.is_some() {
            return Err(CursorError::Conflict.into());
        }
        Ok(invitation)
    }
}

fn acquire(
    tx: &Transaction<'_>,
    key: Selector,
    origin: Origin,
    known: bool,
    wall: u64,
) -> Result<Generation> {
    let (occupied, previous, next_start_at) = tx.query_row(
        "SELECT server IS NOT NULL, generation, next_start_at
            FROM peer_provisioning_slot WHERE class=?1",
        [known],
        |r| {
            Ok((
                r.get::<_, bool>(0)?,
                read_seconds(r, 1)?,
                read_seconds(r, 2)?,
            ))
        },
    )?;
    if occupied || wall < next_start_at {
        return Err(CursorError::Backlog.into());
    }
    let (count, next) = next_charge(previous, wall)?;
    rows::require_write(tx.execute(
        "UPDATE peer_provisioning_slot SET generation=?2, next_start_at=?3,
            server=?4, offer_id=?5, revision=?6, origin=?7 WHERE class=?1 AND server IS NULL",
        params![
            known,
            count,
            next,
            key.server.to_bytes(),
            key.id,
            key.revision,
            origin
        ],
    )?)?;
    rows::require_write(tx.execute(
        "UPDATE peer_consent_invite SET decision_at=NULL
            WHERE server=?1 AND offer_id=?2 AND revision=?3 AND origin=?4",
        params![key.server.to_bytes(), key.id, key.revision, origin],
    )?)?;
    let count = u64::try_from(count).map_err(|_| CursorError::Range)?;
    Ok(Generation { known, count })
}

fn next_charge(previous: u64, wall: u64) -> Result<(i64, i64)> {
    let generation = previous.checked_add(1).ok_or(CursorError::Range)?;
    let next_start_at = wall.checked_add(START_INTERVAL).ok_or(CursorError::Range)?;
    Ok((seconds(generation)?, seconds(next_start_at)?))
}

#[cfg(test)]
#[test]
fn peer_provisioning_charge_handles_both_sql_bounds() {
    let ceiling = i64::MAX as u64;
    let error = Err(Error::Cursor(CursorError::Range));
    assert_eq!(next_charge(ceiling, 0), error);
    assert_eq!(next_charge(0, ceiling - START_INTERVAL + 1), error);
    assert_eq!(
        next_charge(ceiling - 1, ceiling - START_INTERVAL),
        Ok((i64::MAX, i64::MAX))
    );
}

impl Checkpoint {
    pub(crate) fn reserve_provisioning(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
    ) -> Result<Reservation> {
        self.live_reservation(
            source,
            key,
            wall,
            Some(self.policy.reserved.contains(&key.server)),
        )
    }

    pub(crate) fn recover_provisioning(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
    ) -> Result<Reservation> {
        self.live_reservation(source, key, wall, None)
    }

    fn live_reservation(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
        new_class: Option<bool>,
    ) -> Result<Reservation> {
        #[cfg(test)]
        let retirement_epoch = self.retirement_epoch;
        #[cfg(test)]
        if retirement_epoch {
            retirement::verify_epoch(&self.cursor.conn)?;
        }
        let (tx, origin, _) = self.live_provisioning_offer(source, key, wall)?;
        if has_outbox(&tx, key, origin)? {
            return Err(CursorError::Conflict.into());
        }
        let generation = match generation(&tx, key, origin)? {
            Some(generation) => generation,
            None => acquire(&tx, key, origin, new_class.ok_or(Error::Missing)?, wall)?,
        };
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(Reservation {
            owner: self.owner.clone(),
            binding: Binding {
                key,
                origin,
                generation,
            },
        })
    }

    fn live_provisioning_offer(
        &mut self,
        source: &Source,
        key: Selector,
        wall: u64,
    ) -> Result<(Transaction<'_>, Origin, u64)> {
        let origin = self.origin(source)?;
        #[cfg(test)]
        self.custody_mode(key.server)?;
        #[cfg(not(test))]
        if !self.policy.permits(key.server) {
            return Err(Error::Policy);
        }
        let route = self
            .routes
            .iter()
            .find(|route| route.id == origin)
            .ok_or(Error::Source)?;
        #[cfg(test)]
        let custody = match self.export_epoch {
            Some(prepared_consent::Epoch::Custody(epoch)) => {
                Some((epoch, self.cursor.origins.clone()))
            }
            _ => None,
        };
        let cooldown_attached = self.cooldown_attached();
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::check_wall(&tx, wall)?;
        #[cfg(test)]
        if let Some((epoch, origins)) = &custody {
            epoch.verify(&tx, self.signer.public_key().to_bytes(), origins)?;
        }
        if cooldown_attached && cooldown::under_cooldown(&tx, key.server.to_bytes(), wall)? {
            return Err(Error::Cooldown);
        }
        #[cfg(test)]
        if custody.is_some() && prepared_consent::custody::owns(&tx, key, origin)? {
            return Err(Error::Custodied);
        }
        let row = Invitation::load(&tx, key, origin)?;
        if wall >= row.expires {
            rows::advance_wall(&tx, wall)?;
            tx.commit()?;
            return Err(Error::Expired);
        }
        Self::check_signed_offer(&self.codec, &self.signer, route, &row, wall)?;
        let quarantined: bool = tx.query_row(
            "SELECT quarantine_until IS NOT NULL FROM peer_cursor_origin WHERE origin=?1",
            [origin],
            |r| r.get(0),
        )?;
        if quarantined {
            return Err(CursorError::RelayContract.into());
        }
        Ok((tx, origin, row.expires))
    }

    pub(crate) fn retirement_target(
        &self,
        local: &Local,
        key: Selector,
    ) -> Result<RetirementTarget> {
        #[cfg(test)]
        if self.is_installed_custody() {
            return Err(Error::Layout);
        }
        self.check_local(local)?;
        let origin = rows::stored_origin(&self.cursor.conn, key)?;
        let generation = generation(&self.cursor.conn, key, origin)?.ok_or(Error::Missing)?;
        Ok(RetirementTarget {
            owner: self.owner.clone(),
            binding: Binding {
                key,
                origin,
                generation,
            },
        })
    }

    pub(crate) fn retire_provisioning(
        &mut self,
        local: &Local,
        target: RetirementTarget,
        wall: u64,
    ) -> Result<()> {
        #[cfg(test)]
        if self.is_installed_custody() {
            return Err(Error::Layout);
        }
        self.check_local(local)?;
        if !Arc::ptr_eq(&self.owner, &target.owner) {
            return Err(Error::LocalOwner);
        }
        let Binding {
            key,
            origin,
            generation,
        } = target.binding;
        let tx = self.cursor.with_wall(origin, wall)?;
        #[cfg(test)]
        if self.provisional_schema && stage_journal::load(&tx, target.binding)?.is_some() {
            return Err(Error::Bound);
        }
        rows::advance_wall(&tx, wall)?;
        let row = Invitation::load(&tx, key, origin)?;
        if has_outbox(&tx, key, origin)? {
            return Err(CursorError::Conflict.into());
        }
        let changed = tx.execute(
            "UPDATE peer_provisioning_slot SET server=NULL, offer_id=NULL, revision=NULL, origin=NULL
                WHERE class=?1 AND generation=?2 AND server=?3 AND offer_id=?4 AND revision=?5 AND origin=?6",
            params![generation.known, seconds(generation.count)?, key.server.to_bytes(), key.id, key.revision, origin],
        )?;
        if changed != 1 {
            return Err(CursorError::Stale.into());
        }
        row.remove(&tx)?;
        tx.commit()?;
        Ok(())
    }
}
