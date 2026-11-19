//! Borrowed registered authority for one exact durable Consent attempt.

use super::package::PackageProof;
use super::store::consent_checkpoint::Error as CheckpointError;
use super::store::consent_checkpoint::provisioning::{
    Reservation,
    prepared_consent::{Attempt, ManagedQueued, publication::installed_settlement::SettledConsent},
};
use super::*;
use crate::peer_invitation::socket::owner::recipient::CheckedAck;

pub(in crate::peer_invitation) struct ConsentTurn<'lease, 'turn> {
    attempt: Attempt,
    proof: PackageProof<'lease>,
    window: &'turn NativeWindow,
}

impl ConsentTurn<'_, '_> {
    pub(in crate::peer_invitation) fn attempt(&self) -> &Attempt {
        &self.attempt
    }
}

impl<'lease> RecoveredOwner<'lease> {
    pub(in crate::peer_invitation) fn queue_managed_consent(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        proof: &PackageProof<'_>,
        window: &NativeWindow,
    ) -> Result<ManagedQueued> {
        self.queue_managed_consent_with(quota, reservation, proof, window, || {})
    }

    pub(super) fn queue_managed_consent_with(
        &mut self,
        quota: &ReceiverQuota<'_>,
        reservation: &Reservation,
        proof: &PackageProof<'_>,
        window: &NativeWindow,
        after_commit: impl FnOnce(),
    ) -> Result<ManagedQueued> {
        self.recheck_managed_package(quota, proof, window)?;
        let outbox = self.checkpoint.sign_managed_consent(
            &self.source,
            reservation,
            proof,
            window.sample()?.as_secs(),
        )?;
        #[cfg(test)]
        window.signed_fixture();
        self.recheck_managed_package(quota, proof, window)?;
        let saved = self.checkpoint.queue_managed_consent(
            &self.source,
            reservation,
            outbox,
            window.sample()?.as_secs(),
            || commit_fence(window),
        )?;
        after_commit();
        self.recheck_managed_package(quota, proof, window)?;
        let (binding, _) = saved.stage();
        self.checkpoint
            .read_managed_queued(
                &self.source,
                self.signer.public_key(),
                &self.relay,
                binding.coordinates().3,
                window.sample()?.as_secs(),
            )?
            .filter(|current| current.matches_saved(&saved))
            .ok_or(Error::Binding)
    }

    pub(in crate::peer_invitation) fn reserve_managed_consent<'turn>(
        &mut self,
        quota: &ReceiverQuota<'_>,
        queued: ManagedQueued,
        window: &'turn NativeWindow,
    ) -> Result<Option<ConsentTurn<'lease, 'turn>>> {
        let proof = self.attest_queued_package(quota, &queued, window)?;
        self.recheck_managed_package(quota, &proof, window)?;
        let attempt = self.checkpoint.reserve_managed_consent(
            &self.source,
            queued,
            &proof,
            window.sample()?.as_secs(),
            || commit_fence(window),
        )?;
        Ok(attempt.map(|attempt| ConsentTurn {
            attempt,
            proof,
            window,
        }))
    }

    pub(in crate::peer_invitation) fn managed_before_flush(
        &mut self,
        quota: &ReceiverQuota<'_>,
        turn: &ConsentTurn<'_, '_>,
    ) -> Result<()> {
        self.recheck_managed_package(quota, &turn.proof, turn.window)?;
        let wall = turn.window.sample()?.as_secs();
        self.checkpoint
            .check_installed_prepared_attempt(&self.source, &turn.attempt, wall)?;
        turn.window.sample()?;
        Ok(())
    }

    pub(in crate::peer_invitation) fn settle_managed_consent(
        &mut self,
        quota: &ReceiverQuota<'_>,
        turn: ConsentTurn<'_, '_>,
        ack: CheckedAck<'_>,
    ) -> Result<SettledConsent> {
        self.recheck_managed_package(quota, &turn.proof, turn.window)?;
        let result =
            self.checkpoint
                .settle_managed_consent(&self.source, &turn.attempt, ack, || {
                    commit_fence(turn.window)
                })?;
        Ok(result)
    }
}

fn commit_fence(window: &NativeWindow) -> std::result::Result<(), CheckpointError> {
    window.sample().map_err(|_| CheckpointError::Expired)?;
    Ok(())
}
