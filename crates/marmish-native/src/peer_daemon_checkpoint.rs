//! Bounded read-only projection for the test-gated Peer daemon owner.

use nostr::key::PublicKey;

use super::*;

const INVENTORY_LIMIT: usize = 16;

#[derive(Clone, Copy)]
pub(crate) struct Inventory {
    pub(crate) key: Selector,
    pub(crate) generation: u64,
    pub(crate) state: CustodyState,
    pub(crate) held_at: u64,
    pub(crate) qualified_at: u64,
    pub(crate) expires_at: u64,
    pub(crate) activating_at: Option<u64>,
}

impl HeldEvidence {
    pub(crate) fn inventory(&self) -> Option<Inventory> {
        let (joined, activating_at) = match &self.saved.progress {
            Progress::Pending { joined, .. } => (joined, None),
            Progress::Activating { joined, at, .. } => (joined, Some(*at)),
            _ => return None,
        };
        Some(Inventory {
            key: self.stage.0.key,
            generation: self.stage.0.generation.count,
            state: self.state(),
            held_at: self.held_at(),
            qualified_at: joined.qualified_at,
            expires_at: joined.deadline,
            activating_at,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Due {
    Provision(Selector),
    Resume(Selector),
    Publish(Selector),
    Join(Selector, u64),
    Automatic(Selector, u64),
}

#[derive(Default)]
pub(crate) struct Rotation {
    class: usize,
    previous: [Option<[u8; 32]>; 5],
}

impl Rotation {
    fn take(&mut self, class: usize, rows: &[(Selector, u64)]) -> Option<(Selector, u64)> {
        let next = rows.iter().copied().find(|(key, _)| {
            self.previous[class].is_none_or(|older| key.server.to_bytes() > older)
        });
        let selected = next.or_else(|| rows.first().copied())?;
        self.previous[class] = Some(selected.0.server.to_bytes());
        self.class = (class + 1) % self.previous.len();
        Some(selected)
    }
}

impl Checkpoint {
    pub(crate) fn daemon_inventory(
        &mut self,
        source: &Source,
        wall: u64,
    ) -> Result<Vec<Inventory>> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        let selected = coordinates(
            &tx,
            "SELECT server,offer_id,offer_revision,generation
             FROM peer_pending_custody WHERE origin=?1 AND ?2>=0 AND state IN (2,3)
             ORDER BY server LIMIT 17",
            cx.origin,
            wall,
        )?;
        let mut inventory = Vec::with_capacity(selected.len());
        for (selector, _) in selected {
            if !self.policy.permits(selector.server) {
                continue;
            }
            let consent = cx
                .exact_consent(&tx, selector)?
                .ok_or(CursorError::Binding)?;
            let evidence = cx
                .read_evidence(&tx, consent)?
                .ok_or(CursorError::Binding)?;
            inventory.push(evidence.inventory().ok_or(CursorError::Binding)?);
        }
        Ok(inventory)
    }

    pub(crate) fn daemon_due(
        &mut self,
        source: &Source,
        wall: u64,
        rotation: &mut Rotation,
        online: bool,
        founder: PublicKey,
    ) -> Result<Option<Due>> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        let slot_available = |known| -> Result<bool> {
            Ok(tx.query_row(
                "SELECT server IS NULL AND next_start_at<=?2
                 FROM peer_provisioning_slot WHERE class=?1",
                params![known, seconds(wall)?],
                |row| row.get(0),
            )?)
        };
        for distance in 0..rotation.previous.len() {
            let class = (rotation.class + distance) % rotation.previous.len();
            let statement = match class {
                0 => PROVISION,
                1 => RESUME,
                2 if online => PUBLISH,
                3 => JOIN,
                4 if matches!(self.policy.mode, Mode::Automatic) => AUTOMATIC,
                _ => continue,
            };
            let mut rows = coordinates(&tx, statement, cx.origin, wall)?;
            rows.retain(|(selector, _)| {
                selector.server == founder && self.policy.permits(selector.server)
            });
            if class == 0 {
                let slots = [slot_available(false)?, slot_available(true)?];
                rows.retain(|(selector, _)| {
                    slots[usize::from(self.policy.reserved.contains(&selector.server))]
                });
            }
            if let Some((selector, generation)) = rotation.take(class, &rows) {
                let due = match class {
                    0 => Due::Provision(selector),
                    1 => Due::Resume(selector),
                    2 => Due::Publish(selector),
                    3 => Due::Join(selector, generation),
                    4 => Due::Automatic(selector, generation),
                    _ => unreachable!("bounded due-work class"),
                };
                return Ok(Some(due));
            }
        }
        Ok(None)
    }
}

fn coordinates(
    tx: &Transaction<'_>,
    statement: &str,
    origin: Origin,
    wall: u64,
) -> Result<Vec<(Selector, u64)>> {
    let mut statement = tx.prepare(statement)?;
    let rows = statement.query_map(params![origin, seconds(wall)?], |row| {
        Ok((
            Selector {
                server: PublicKey::from_byte_array(row.get(0)?),
                id: row.get(1)?,
                revision: row.get(2)?,
            },
            read_seconds(row, 3)?,
        ))
    })?;
    let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.len() > INVENTORY_LIMIT {
        return Err(CursorError::Binding.into());
    }
    Ok(rows)
}

const PROVISION: &str = "
    SELECT i.server,i.offer_id,i.revision,0 FROM peer_consent_invite i
    WHERE i.origin=?1 AND i.expires>?2 AND i.decision_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM peer_provisioning_slot p WHERE p.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_prepared_consent_outbox o WHERE o.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_consent_outbox o WHERE o.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody c WHERE c.server=i.server)
    ORDER BY i.server LIMIT 17";

const RESUME: &str = "
    SELECT p.server,p.offer_id,p.revision,p.generation FROM peer_provisioning_slot p
    JOIN peer_consent_invite i ON i.server=p.server AND i.offer_id=p.offer_id
        AND i.revision=p.revision AND i.origin=p.origin
    WHERE p.origin=?1 AND i.expires>?2 AND i.decision_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM peer_prepared_consent_outbox o WHERE o.class=p.class)
    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody c WHERE c.class=p.class)
    ORDER BY p.server LIMIT 17";

const PUBLISH: &str = "
    SELECT o.server,o.offer_id,o.offer_revision,o.generation FROM peer_prepared_consent_outbox o
    WHERE o.origin=?1 AND o.ack_at IS NULL AND o.expires>?2 AND o.package_expires>?2
    AND (o.retry_at IS NULL OR o.retry_at<=?2)
    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody c WHERE c.class=o.class)
    ORDER BY o.server LIMIT 17";

const JOIN: &str = "
    SELECT server,offer_id,offer_revision,generation FROM peer_pending_custody
    WHERE origin=?1 AND held_deadline>?2 AND state IN (0,1)
    ORDER BY server LIMIT 17";

const AUTOMATIC: &str = "
    SELECT server,offer_id,offer_revision,generation FROM peer_pending_custody
    WHERE origin=?1 AND pending_deadline>?2 AND state=2 AND mode=1
    ORDER BY server LIMIT 17";
