//! Serialized adoption of an exact, already-custodied Delivery.

use super::*;
use crate::peer_invitation::store::consent_checkpoint::provisioning::prepared_consent::custody::{
    Approval, CustodyState, HeldEvidence, JoinStarted,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum JoinCut {
    Started,
    OriginalClosed,
    ReadbackVerified,
    PendingCommitted,
    ActivatingCommitted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum CustodyProgress {
    Missing,
    Unavailable(Error),
    Pending,
    Activating,
}

impl PreparedConsentOwner<'_> {
    fn current_custody(
        &mut self,
        selector: Selector,
        generation: u64,
    ) -> Result<Option<(HeldEvidence, Delivery)>> {
        if !self.custody {
            return Err(Error::Binding);
        }
        let wall = self.available_sample()?.as_secs();
        let Some(held) =
            self.state
                .checkpoint
                .read_custody(&self.state.source, selector, generation, wall)?
        else {
            return Ok(None);
        };
        if wall
            >= held
                .pending_deadline()
                .unwrap_or_else(|| held.held_deadline())
        {
            return Err(consent_checkpoint::Error::Expired.into());
        }
        let delivery = self
            .state
            .checkpoint
            .custody_codec()
            .reopen_custodied_delivery(&self.state.signer, &held, wall)
            .map_err(|_| consent_checkpoint::Error::Package)?;
        Ok(Some((held, delivery)))
    }

    pub(in crate::peer_invitation) async fn advance_custody(
        &mut self,
        selector: Selector,
        generation: u64,
        mut cut: impl FnMut(JoinCut) -> Result<()>,
    ) -> Result<CustodyProgress> {
        // Installed v2 cannot use this fixture's uncapped native actor.
        if self.state.checkpoint.is_installed_custody() {
            return Err(Error::Binding);
        }
        let Some((held, delivery)) = self.current_custody(selector, generation)? else {
            return Ok(CustodyProgress::Missing);
        };
        if held.state() == CustodyState::Held {
            let sample = self.available_sample()?;
            let native = self.attest(held.stage(), sample)?;
            native.require(
                held.stage(),
                Artifact {
                    id: held.package().event_id,
                    digest: held.package_digest(),
                    expires: held.package_bounds().1,
                },
                sample.as_secs(),
            )?;
            self.verify_custody(&native)?;
            let wall = self.available_sample()?.as_secs();
            let started =
                self.state
                    .checkpoint
                    .start_custody_join(&self.state.source, &held, wall)?;
            cut(JoinCut::Started)?;
            self.join_once_from_started(started, &held, &delivery, &native)
                .await?;
            cut(JoinCut::OriginalClosed)?;
        }

        let Some((recovered, delivery)) = self.current_custody(selector, generation)? else {
            return Err(Error::Binding);
        };
        let joined =
            match self.inspect_joined(recovered.stage(), &delivery, recovered.package_digest()) {
                Ok(joined) => joined,
                Err(error) => return Ok(CustodyProgress::Unavailable(error)),
            };
        cut(JoinCut::ReadbackVerified)?;
        let progress = match recovered.state() {
            CustodyState::JoinStarted => {
                let wall = self.available_sample()?.as_secs();
                self.state.checkpoint.commit_custody_pending(
                    &self.state.source,
                    &recovered,
                    &joined,
                    wall,
                )?;
                cut(JoinCut::PendingCommitted)?;
                CustodyProgress::Pending
            }
            CustodyState::Pending if recovered.matches_pending_readback(&joined) => {
                CustodyProgress::Pending
            }
            CustodyState::Activating if recovered.matches_pending_readback(&joined) => {
                CustodyProgress::Activating
            }
            _ => CustodyProgress::Unavailable(Error::StageIdentityMismatch),
        };
        Ok(progress)
    }

    async fn join_once_from_started(
        &mut self,
        started: JoinStarted,
        held: &HeldEvidence,
        delivery: &Delivery,
        native: &NativeAttestation,
    ) -> Result<()> {
        if !started.matches(held) {
            return Err(Error::Binding);
        }
        let sample = self.available_sample()?;
        if sample.as_secs() >= held.held_deadline() {
            return Err(consent_checkpoint::Error::Expired.into());
        }
        self.verify_custody(native)?;
        self.join_custody_stage(held.stage(), delivery, held.package_digest(), sample)
            .await
    }

    pub(in crate::peer_invitation) fn read_pending_proof(
        &mut self,
        selector: Selector,
        generation: u64,
    ) -> Result<Option<(HeldEvidence, JoinedReadback)>> {
        let Some((evidence, delivery)) = self.current_custody(selector, generation)? else {
            return Ok(None);
        };
        if !matches!(
            evidence.state(),
            CustodyState::Pending | CustodyState::Activating
        ) {
            return Err(Error::Binding);
        }
        let joined = self.inspect_joined(evidence.stage(), &delivery, evidence.package_digest())?;
        if !evidence.matches_pending_readback(&joined) {
            return Err(Error::StageIdentityMismatch);
        }
        // Snapshot verification can take time. A read-only retry cannot report
        // an expired row just because the snapshot was opened before expiry.
        self.require_pending_live(&evidence)?;
        Ok(Some((evidence, joined)))
    }

    fn require_pending_live(&mut self, evidence: &HeldEvidence) -> Result<u64> {
        let wall = self.available_sample()?.as_secs();
        let deadline = evidence.pending_deadline().ok_or(Error::Binding)?;
        if wall >= deadline {
            return Err(consent_checkpoint::Error::Expired.into());
        }
        Ok(wall)
    }

    pub(in crate::peer_invitation) fn commit_pending_decision(
        &mut self,
        pending: &HeldEvidence,
        joined: &JoinedReadback,
        check_current: impl FnOnce(&Checkpoint) -> Result<CustodyDecision>,
    ) -> Result<u64> {
        if pending.state() != CustodyState::Pending {
            return Err(Error::Binding);
        }
        let wall = self.require_pending_live(pending)?;
        // The paired exchange or private auto cause is checked on the owner
        // thread after readback and directly before the synchronous CAS.
        let cause = check_current(&self.state.checkpoint)?;
        let approval = match &cause {
            CustodyDecision::Manual(local) => Approval::Manual(local),
            CustodyDecision::Automatic => Approval::Automatic,
        };
        self.state.checkpoint.commit_custody_activating(
            &self.state.source,
            pending,
            joined,
            approval,
            wall,
        )?;
        Ok(wall)
    }

    pub(in crate::peer_invitation) fn activate_custody(
        &mut self,
        selector: Selector,
        generation: u64,
        connection: Option<&UnixStream>,
        mut cut: impl FnMut(JoinCut) -> Result<()>,
    ) -> Result<CustodyProgress> {
        let local = connection
            .map(|connection| verified_local(&self.state.checkpoint, connection))
            .transpose()?;
        let (pending, joined) = match self.read_pending_proof(selector, generation) {
            Ok(Some(proof)) => proof,
            Ok(None) => return Ok(CustodyProgress::Missing),
            Err(error @ (Error::Native(_) | Error::Path | Error::StageIdentityMismatch)) => {
                return Ok(CustodyProgress::Unavailable(error));
            }
            Err(error) => return Err(error),
        };
        let cause = local.map_or(CustodyDecision::Automatic, CustodyDecision::Manual);
        self.commit_pending_decision(&pending, &joined, |_| Ok(cause))?;
        cut(JoinCut::ActivatingCommitted)?;
        Ok(CustodyProgress::Activating)
    }
}
