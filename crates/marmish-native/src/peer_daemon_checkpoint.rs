//! Bounded read-only projection for the test-gated Peer daemon owner.

use nostr::key::PublicKey;

use super::*;

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

impl Checkpoint {
    pub(crate) fn daemon_inventory(
        &mut self,
        source: &Source,
        wall: u64,
        founder: PublicKey,
    ) -> Result<Vec<Inventory>> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        if !self.policy.permits(founder) {
            return Ok(Vec::new());
        }
        let Some((selector, _)) = coordinates(
            &tx,
            "SELECT server,offer_id,offer_revision,generation
             FROM peer_pending_custody
             WHERE origin=?1 AND pending_deadline>?2 AND server=?3 AND state IN (2,3)",
            cx.origin,
            wall,
            founder,
        )?
        else {
            return Ok(Vec::new());
        };
        let consent = cx
            .exact_consent(&tx, selector)?
            .ok_or(CursorError::Binding)?;
        let evidence = cx
            .read_evidence(&tx, consent)?
            .ok_or(CursorError::Binding)?;
        Ok(vec![evidence.inventory().ok_or(CursorError::Binding)?])
    }

    pub(crate) fn daemon_due(
        &mut self,
        source: &Source,
        wall: u64,
        online: bool,
        founder: PublicKey,
    ) -> Result<Option<Due>> {
        let cx = self.custody_context(source)?;
        let tx = self.cursor.with_wall(cx.origin, wall)?;
        cx.verify(&tx, wall)?;
        if !self.policy.permits(founder) {
            return Ok(None);
        }
        // This fixture binds one founder with one unique invite, slot, outbox
        // and custody row. The checked schema makes these work classes exclusive.
        let find = |statement| coordinates(&tx, statement, cx.origin, wall, founder);
        if let Some((key, generation)) = find(JOIN)? {
            return Ok(Some(Due::Join(key, generation)));
        }
        if matches!(self.policy.mode, Mode::Automatic)
            && let Some((key, generation)) = find(AUTOMATIC)?
        {
            return Ok(Some(Due::Automatic(key, generation)));
        }
        if online && let Some((key, _)) = find(PUBLISH)? {
            return Ok(Some(Due::Publish(key)));
        }
        if let Some((key, _)) = find(RESUME)? {
            return Ok(Some(Due::Resume(key)));
        }
        if let Some((key, _)) = find(PROVISION)? {
            let slot_available: bool = tx.query_row(
                "SELECT server IS NULL AND next_start_at<=?2
                 FROM peer_provisioning_slot WHERE class=?1",
                params![self.policy.reserved.contains(&founder), seconds(wall)?],
                |row| row.get(0),
            )?;
            if slot_available {
                return Ok(Some(Due::Provision(key)));
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
    founder: PublicKey,
) -> Result<Option<(Selector, u64)>> {
    Ok(tx
        .query_row(
            statement,
            params![origin, seconds(wall)?, founder.to_bytes()],
            |row| {
                Ok((
                    Selector {
                        server: PublicKey::from_byte_array(row.get(0)?),
                        id: row.get(1)?,
                        revision: row.get(2)?,
                    },
                    read_seconds(row, 3)?,
                ))
            },
        )
        .optional()?)
}

const PROVISION: &str = "
    SELECT i.server,i.offer_id,i.revision,0 FROM peer_consent_invite i
    WHERE i.origin=?1 AND i.expires>?2 AND i.server=?3 AND i.decision_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM peer_provisioning_slot p WHERE p.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_prepared_consent_outbox o WHERE o.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_consent_outbox o WHERE o.server=i.server)
    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody c WHERE c.server=i.server)";

const RESUME: &str = "
    SELECT p.server,p.offer_id,p.revision,p.generation FROM peer_provisioning_slot p
    JOIN peer_consent_invite i ON i.server=p.server AND i.offer_id=p.offer_id
        AND i.revision=p.revision AND i.origin=p.origin
    WHERE p.origin=?1 AND i.expires>?2 AND p.server=?3 AND i.decision_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM peer_prepared_consent_outbox o WHERE o.class=p.class)
    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody c WHERE c.class=p.class)";

const PUBLISH: &str = "
    SELECT o.server,o.offer_id,o.offer_revision,o.generation FROM peer_prepared_consent_outbox o
    WHERE o.origin=?1 AND o.server=?3 AND o.ack_at IS NULL AND o.expires>?2 AND o.package_expires>?2
    AND (o.retry_at IS NULL OR o.retry_at<=?2)
    AND NOT EXISTS (SELECT 1 FROM peer_pending_custody c WHERE c.class=o.class)";

const JOIN: &str = "
    SELECT server,offer_id,offer_revision,generation FROM peer_pending_custody
    WHERE origin=?1 AND held_deadline>?2 AND server=?3 AND state IN (0,1)";

const AUTOMATIC: &str = "
    SELECT server,offer_id,offer_revision,generation FROM peer_pending_custody
    WHERE origin=?1 AND pending_deadline>?2 AND server=?3 AND state=2 AND mode=1";
