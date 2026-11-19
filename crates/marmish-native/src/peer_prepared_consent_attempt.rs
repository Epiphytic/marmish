//! Exact owner-local attempts and compare-and-swap acknowledgement.

#[cfg(test)]
use crate::peer_invitation::prepared_consent::{MatchingAck, NativeAttestation};
use nostr::key::PublicKey;
use std::sync::Arc;

use super::*;
use outbox::{Advance, RETRY_SECONDS};

pub(crate) struct Attempt {
    owner: Arc<()>,
    queued: Queued,
}

impl Attempt {
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(super) fn selector(&self) -> Selector {
        self.queued.prepared.binding.key
    }

    pub(crate) fn carrier(&self) -> [u8; 32] {
        self.queued.row.transcript.carrier
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.queued.row.frame
    }

    pub(crate) fn origin(&self) -> Origin {
        self.queued.prepared.binding.origin
    }

    pub(crate) fn account(&self) -> PublicKey {
        self.queued.prepared.offer.flight().recipient
    }

    pub(crate) fn founder(&self) -> PublicKey {
        self.queued.prepared.binding.key.server
    }

    pub(crate) fn deadline(&self) -> u64 {
        self.queued
            .row
            .transcript
            .expires
            .min(self.queued.prepared.offer.flight().expires_at)
            .min(self.queued.prepared.artifact.expires)
    }

    pub(crate) fn sequence(&self) -> u64 {
        self.queued.row.attempt
    }
}

impl Checkpoint {
    #[cfg(test)]
    fn change_queued<T>(
        &mut self,
        source: &Source,
        prior: &Queued,
        wall: u64,
        work: impl FnOnce(&Transaction<'_>, &Candidate, &outbox::Outbox) -> Result<T>,
    ) -> Result<T> {
        let origin = self.queued_origin(source, prior)?;
        let account = prior.prepared.account;
        let epoch = self.export_epoch.ok_or(CursorError::Binding)?;
        let origins = self.cursor.origins.clone();
        let tx = self.cursor.with_wall(origin, wall)?;
        let current = exact_row(&tx, &epoch, &origins, prior, wall)?;
        let result = work(&tx, &prior.prepared, &current)?;
        schema::verify_for_epoch(&tx, account, &origins, &epoch)?;
        rows::advance_wall(&tx, wall)?;
        tx.commit()?;
        Ok(result)
    }

    fn queued_origin(&self, source: &Source, prior: &Queued) -> Result<Origin> {
        let binding = prior.prepared.binding;
        if self.origin(source)? != binding.origin
            || prior.prepared.account != self.signer.public_key().to_bytes()
        {
            return Err(CursorError::Binding.into());
        }
        if !self.policy.permits(binding.key.server) {
            return Err(Error::Policy);
        }
        Ok(binding.origin)
    }

    pub(crate) fn check_installed_prepared_attempt(
        &mut self,
        source: &Source,
        attempt: &Attempt,
        wall: u64,
    ) -> Result<()> {
        self.check_local_owner(&attempt.owner)?;
        let prior = &attempt.queued;
        let origin = self.queued_origin(source, prior)?;
        let key = prior.prepared.binding.key;
        let url = self.route(origin)?.urls[0].clone();
        self.read_installed_consent(source, attempt.account(), &url, wall, |tx, cx| {
            if custody::owns(tx, key, origin)? {
                return Err(Error::Custodied);
            }
            let current = cx.exact_consent(tx, key)?.ok_or(CursorError::Stale)?;
            if current.stage() != prior.stage()
                || current.prepared.artifact != prior.prepared.artifact
                || current.prepared.offer.flight().carrier_id
                    != prior.prepared.offer.flight().carrier_id
                || current.row != prior.row
                || current.row.ack_at.is_some()
                || current.row.attempt == 0
            {
                return Err(CursorError::Stale.into());
            }
            if wall >= attempt.deadline() {
                return Err(Error::Expired);
            }
            Ok(())
        })
    }

    #[cfg(test)]
    pub(crate) fn reserve_prepared_attempt(
        &mut self,
        source: &Source,
        mut queued: Queued,
        native: &NativeAttestation,
        wall: u64,
    ) -> Result<Option<Attempt>> {
        self.verify_queued_consent(&queued, native, wall)?;
        let reservation = self.change_queued(source, &queued, wall, |tx, prepared, row| {
            reserve_row(tx, prepared, row, wall)
        })?;
        Ok(reservation.map(|(attempt, retry_at)| {
            queued.row.attempt = attempt;
            queued.row.retry_at = Some(retry_at);
            Attempt {
                owner: self.owner.clone(),
                queued,
            }
        }))
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fn reserve_managed_prepared_attempt(
        &mut self,
        source: &Source,
        managed: ManagedQueued,
        wall: u64,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<Option<Attempt>> {
        self.require_managed_package_target(&managed.target)?;
        let mut queued = managed.queued;
        let reservation = self.change_installed_consent(
            source,
            &queued,
            wall,
            |tx, current| reserve_row(tx, &current.prepared, &current.row, wall),
            before_commit,
        )?;
        Ok(reservation.map(|(attempt, retry_at)| {
            queued.row.attempt = attempt;
            queued.row.retry_at = Some(retry_at);
            Attempt {
                owner: self.owner.clone(),
                queued,
            }
        }))
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    fn settle_managed_prepared_attempt(
        &mut self,
        source: &Source,
        attempt: &Attempt,
        wall: u64,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        self.check_local_owner(&attempt.owner)?;
        self.change_installed_consent(
            source,
            &attempt.queued,
            wall,
            |tx, current| {
                if current.row.attempt == 0 || current.row.ack_at.is_some() {
                    return Err(CursorError::Stale.into());
                }
                outbox::advance(
                    tx,
                    &current.prepared,
                    &current.row,
                    Advance::Acknowledge { wall },
                )?;
                Ok(Some(()))
            },
            before_commit,
        )?
        .ok_or(CursorError::Stale)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn check_current_attempt(
        &mut self,
        source: &Source,
        attempt: &Attempt,
        wall: u64,
    ) -> Result<()> {
        self.check_local_owner(&attempt.owner)?;
        self.change_queued(source, &attempt.queued, wall, |_, _, _| Ok(()))
    }

    #[cfg(test)]
    pub(crate) fn settle_prepared_ack(
        &mut self,
        source: &Source,
        receipt: MatchingAck,
        wall: u64,
    ) -> Result<()> {
        self.settle_attempt(source, receipt.into_attempt(), wall)
    }

    #[cfg(test)]
    fn settle_attempt(&mut self, source: &Source, attempt: Attempt, wall: u64) -> Result<()> {
        self.check_local_owner(&attempt.owner)?;
        self.change_queued(source, &attempt.queued, wall, |tx, prepared, row| {
            outbox::advance(tx, prepared, row, Advance::Acknowledge { wall })
        })
    }
}

fn reserve_row(
    tx: &Transaction<'_>,
    prepared: &Candidate,
    row: &outbox::Outbox,
    wall: u64,
) -> Result<Option<(u64, u64)>> {
    if row.ack_at.is_some() || row.retry_at.is_some_and(|retry| retry > wall) {
        return Ok(None);
    }
    let attempt = row.attempt.checked_add(1).ok_or(CursorError::Range)?;
    let retry_at = wall.checked_add(RETRY_SECONDS).ok_or(CursorError::Range)?;
    outbox::advance(tx, prepared, row, Advance::Reserve { attempt, retry_at })?;
    Ok(Some((attempt, retry_at)))
}

#[cfg(test)]
fn exact_row(
    tx: &Transaction<'_>,
    epoch: &Epoch,
    origins: &[Origin],
    prior: &Queued,
    wall: u64,
) -> Result<outbox::Outbox> {
    let binding = prior.prepared.binding;
    rows::check_wall(tx, wall)?;
    schema::verify_for_epoch(tx, prior.prepared.account, origins, epoch)?;
    if matches!(epoch, Epoch::Custody(_)) && custody::owns(tx, binding.key, binding.origin)? {
        return Err(Error::Custodied);
    }
    let quarantined: bool = tx.query_row(
        "SELECT quarantine_until IS NOT NULL FROM peer_cursor_origin WHERE origin=?1",
        [binding.origin],
        |r| r.get(0),
    )?;
    if quarantined {
        return Err(CursorError::RelayContract.into());
    }
    let stage = State::Prepared(prior.prepared.witness, prior.prepared.artifact);
    if stage_journal::load(tx, binding)? != Some(stage) {
        return Err(CursorError::Stale.into());
    }
    let current = outbox::load(tx, &prior.prepared)?.ok_or(CursorError::Stale)?;
    if current != prior.row {
        return Err(CursorError::Stale.into());
    }
    if wall >= current.transcript.expires {
        return Err(Error::Expired);
    }
    Ok(current)
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
native_consent_publication_fixture!();

#[cfg(all(
    not(test),
    marmish_peer_receiver_candidate,
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "peer_installed_consent_settlement.rs"]
pub(in crate::peer_invitation) mod installed_settlement;
