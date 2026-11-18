//! Durable publication attempts. The socket owner checks acceptance before recording an ACK.

use super::super::seconds;
use super::*;
use rusqlite::params;

pub(crate) struct Attempt {
    key: Selector,
    row: Outbox,
}

impl Attempt {
    pub(crate) fn bytes(&self) -> &[u8] {
        self.row.frame.as_slice()
    }

    pub(crate) fn carrier(&self) -> [u8; 32] {
        self.row.carrier
    }
}

impl Checkpoint {
    pub(crate) fn reserve_attempt(
        &mut self,
        source: &Source,
        key: Selector,
        native_id: [u8; 32],
        native_bytes: &[u8],
        wall: u64,
    ) -> Result<Option<Attempt>> {
        let origin = self.origin(source)?;
        if !self.policy.permits(key.server) {
            return Err(Error::Policy);
        }
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::check_wall(&tx, wall)?;
        let invite = Invitation::load(&tx, key, origin)?;
        let Some(mut row) = invite.outbox(&tx)? else {
            return Err(Error::Missing);
        };
        let native_digest: [u8; 32] = Sha256::digest(native_bytes).into();
        if invite.decision_at.is_none()
            || row.package_id != native_id
            || row.package_digest != native_digest
        {
            return Err(Error::Package);
        }
        if wall >= row.expires {
            return Err(Error::Expired);
        }
        if row.acknowledged_at.is_some() || row.retry_at.is_some_and(|retry| retry > wall) {
            return Ok(None);
        }
        row.attempt = row.attempt.checked_add(1).ok_or(CursorError::Range)?;
        let retry = wall.checked_add(60).ok_or(CursorError::Range)?;
        rows::advance_wall(&tx, wall)?;
        let changed = tx.execute(
            "UPDATE peer_consent_outbox SET attempt=?5, retry_at=?6
             WHERE server=?1 AND offer_id=?2 AND offer_revision=?3 AND origin=?4",
            params![
                key.server.to_bytes(),
                key.id,
                key.revision,
                origin,
                seconds(row.attempt)?,
                seconds(retry)?
            ],
        )?;
        if changed != 1 {
            return Err(CursorError::Stale.into());
        }
        row.retry_at = Some(retry);
        tx.commit()?;
        Ok(Some(Attempt { key, row }))
    }

    pub(crate) fn record_acceptance(
        &mut self,
        source: &Source,
        attempt: Attempt,
        wall: u64,
    ) -> Result<()> {
        let origin = self.origin(source)?;
        let Attempt { key, row } = attempt;
        if row.origin != origin {
            return Err(Error::Source);
        }
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::advance_wall(&tx, wall)?;
        if wall >= row.expires {
            return Err(Error::Expired);
        }
        let changed = tx.execute(
            "UPDATE peer_consent_outbox SET acknowledged_at=?10
             WHERE server=?1 AND offer_id=?2 AND offer_revision=?3 AND origin=?4
             AND carrier=?5 AND package_id=?6 AND package_digest=?7 AND attempt=?8
             AND expires=?9 AND acknowledged_at IS NULL",
            params![
                key.server.to_bytes(),
                key.id,
                key.revision,
                origin,
                row.carrier,
                row.package_id,
                row.package_digest,
                seconds(row.attempt)?,
                seconds(row.expires)?,
                seconds(wall)?
            ],
        )?;
        if changed != 1 {
            return Err(CursorError::Stale.into());
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn summaries(
        &mut self,
        local: &Local,
        source: &Source,
        wall: u64,
    ) -> Result<Vec<(Selector, bool)>> {
        self.check_local(local)?;
        let origin = self.origin(source)?;
        let tx = self.cursor.with_wall(origin, wall)?;
        rows::check_wall(&tx, wall)?;
        let mut stmt = tx.prepare(
            "SELECT server, offer_id, revision, decision_at FROM peer_consent_invite
             WHERE origin=?1 AND expires>?2
                 AND NOT EXISTS (SELECT 1 FROM peer_provisioning_slot p
                     WHERE p.server=peer_consent_invite.server)
             ORDER BY server LIMIT 16",
        )?;
        let summaries = stmt
            .query_map(params![origin, seconds(wall)?], |row| {
                let server: [u8; 32] = row.get(0)?;
                let server =
                    PublicKey::from_slice(&server).map_err(|_| rusqlite::Error::InvalidQuery)?;
                Ok((
                    Selector {
                        server,
                        id: row.get(1)?,
                        revision: row.get(2)?,
                    },
                    row.get::<_, Option<i64>>(3)?.is_some(),
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(summaries)
    }
}
