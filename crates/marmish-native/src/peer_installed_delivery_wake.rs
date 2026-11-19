//! Advance existing exact deferred Delivery work after durable Consent acceptance.

use super::super::super::super::installed;
use super::super::publication::installed_settlement::SettledConsent;
use super::*;

#[cfg(test)]
use super::super::super::super::super::EventId;

impl Checkpoint {
    #[cfg(test)]
    pub(in crate::peer_invitation) fn wake_installed_delivery(
        &mut self,
        source: &Source,
        settled: SettledConsent,
        account: PublicKey,
        origin: &str,
        wall: u64,
    ) -> Result<usize> {
        self.wake_installed_delivery_fenced(source, settled, account, origin, wall, || Ok(()))
    }

    pub(in crate::peer_invitation) fn wake_installed_delivery_fenced(
        &mut self,
        source: &Source,
        settled: SettledConsent,
        account: PublicKey,
        origin: &str,
        wall: u64,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<usize> {
        let (owner, selector, carrier, ack_wall) = settled.consume();
        self.check_local_owner(&owner)?;
        self.check_installed_consent_scope(source, account, origin, wall)?;
        let cx = self.custody_context(source)?;
        let fence = |tx: &Transaction<'_>| -> Result<()> {
            cx.verify(tx, wall)?;
            installed::check_parent(tx, &cx.codec, &cx.signer, &cx.route, selector)?;
            if terminalized(tx, selector, cx.origin)? || owns(tx, selector, cx.origin)? {
                return Err(CursorError::Stale.into());
            }
            let queued = cx.exact_consent(tx, selector)?.ok_or(CursorError::Stale)?;
            if queued.row.transcript.carrier != carrier
                || queued.row.ack_at != Some(ack_wall)
                || ack_wall > wall
            {
                return Err(CursorError::Stale.into());
            }
            Ok(rows::advance_wall(tx, wall)?)
        };
        Ok(self.cursor.wake_with_commit_fence(
            cx.origin,
            carrier,
            wall,
            |tx| fence(tx).map_err(schema::cursor_error),
            || before_commit().map_err(schema::cursor_error),
        )?)
    }
}

#[cfg(test)]
impl Checkpoint {
    pub(in crate::peer_invitation) fn fixture_retry_row(
        &self,
        source: &Source,
        id: EventId,
    ) -> Result<(u64, i64, [u8; 32])> {
        let cx = self.custody_context(source)?;
        let (retry, revision, prerequisite): (u64, i64, Vec<u8>) = self.cursor.conn.query_row(
            "SELECT retry_at, revision, prerequisite FROM peer_cursor_deferred
                 WHERE origin=?1 AND id=?2 AND auth=2",
            params![cx.origin, id],
            |row| Ok((read_seconds(row, 0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok((
            retry,
            revision,
            prerequisite.try_into().map_err(|_| Error::Index)?,
        ))
    }

    pub(in crate::peer_invitation) fn fixture_other_retry(
        &self,
        source: &Source,
        matched: EventId,
        other: EventId,
        unrelated: EventId,
    ) -> Result<()> {
        let cx = self.custody_context(source)?;
        let added = self.cursor.conn.execute(
            "INSERT INTO peer_cursor_deferred
             (origin, id, stamp, frame, auth, expires, prerequisite, retry_at, step, revision)
             SELECT origin, ?3, stamp, X'01', auth, expires, ?4, retry_at, step, revision
             FROM peer_cursor_deferred WHERE origin=?1 AND id=?2 AND auth=2",
            params![cx.origin, matched, other, unrelated],
        )?;
        if added != 1 {
            return Err(Error::Index);
        }
        Ok(())
    }
}
