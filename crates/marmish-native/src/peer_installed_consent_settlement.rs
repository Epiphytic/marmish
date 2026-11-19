//! Durable Consent authority for a library-checked package and receipt.

use crate::peer_invitation::receiver_inventory::package::PackageProof;
use crate::peer_invitation::socket::owner::recipient::CheckedAck;

use super::*;
use std::sync::Arc;

pub(in crate::peer_invitation) struct SettledConsent {
    owner: Arc<()>,
    selector: Selector,
    carrier: [u8; 32],
    wall: u64,
}

impl SettledConsent {
    pub(in crate::peer_invitation) fn consume(self) -> (Arc<()>, Selector, [u8; 32], u64) {
        (self.owner, self.selector, self.carrier, self.wall)
    }
}

impl Checkpoint {
    #[cfg(test)]
    pub(in crate::peer_invitation) fn settle_installed_prepared_attempt(
        &mut self,
        source: &Source,
        attempt: Attempt,
        ack: CheckedAck<'_>,
    ) -> Result<SettledConsent> {
        let route = self.route(attempt.origin())?;
        let wall = ack.exact_attempt(&attempt, &route.urls[0])?;
        self.check_installed_prepared_attempt(source, &attempt, wall)?;
        let settled = SettledConsent {
            owner: Arc::clone(&source.owner),
            selector: attempt.selector(),
            carrier: attempt.carrier(),
            wall,
        };
        self.settle_attempt(source, attempt, wall)?;
        Ok(settled)
    }
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn sign_managed_consent(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        proof: &PackageProof<'_>,
        wall: u64,
    ) -> Result<Unsaved> {
        let candidate = self.inspect_exact_prepared(source, reservation, wall, true)?;
        let target = proof.target();
        if candidate.stage() != target.stage() || candidate.artifact != target.artifact() {
            return Err(Error::Package);
        }
        let package = proof
            .require(self, target, wall)
            .map_err(|_| Error::Package)?;
        self.sign_exact_prepared(&candidate, package, wall)
    }

    pub(in crate::peer_invitation) fn queue_managed_consent(
        &mut self,
        source: &Source,
        reservation: &Reservation,
        outbox: Unsaved,
        wall: u64,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<Queued> {
        self.queue_exact_prepared(source, reservation, outbox, wall, true, |_| before_commit())
    }

    pub(in crate::peer_invitation) fn reserve_managed_consent(
        &mut self,
        source: &Source,
        managed: ManagedQueued,
        proof: &PackageProof<'_>,
        wall: u64,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<Option<Attempt>> {
        let package = proof
            .require(self, managed.target(), wall)
            .map_err(|_| Error::Package)?;
        let offer = &managed.queued.prepared.offer;
        let row = &managed.queued.row;
        row.transcript
            .verify(&self.signer, offer, &row.frame, package, wall)
            .map_err(|_| Error::Package)?;
        self.reserve_managed_prepared_attempt(source, managed, wall, before_commit)
    }

    pub(in crate::peer_invitation) fn settle_managed_consent(
        &mut self,
        source: &Source,
        attempt: &Attempt,
        ack: CheckedAck<'_>,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<SettledConsent> {
        let route = self.route(attempt.origin())?;
        let wall = ack.exact_attempt(attempt, &route.urls[0])?;
        let settled = SettledConsent {
            owner: Arc::clone(&source.owner),
            selector: attempt.selector(),
            carrier: attempt.carrier(),
            wall,
        };
        self.settle_managed_prepared_attempt(source, attempt, wall, before_commit)?;
        Ok(settled)
    }
}

#[cfg(test)]
impl Checkpoint {
    pub(in crate::peer_invitation) fn consent_fixture_clocks(&self) -> (u64, u64) {
        let (origin, checkpoint): (i64, i64) = self
            .cursor
            .conn
            .query_row(
                "SELECT (SELECT wall FROM peer_cursor_origin),
                    (SELECT wall FROM peer_consent_version)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        (origin.try_into().unwrap(), checkpoint.try_into().unwrap())
    }
}
