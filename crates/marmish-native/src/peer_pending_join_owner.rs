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
        let delivery = Codec::new(MAX_OUTER_BYTES)
            .map_err(|_| Error::Binding)?
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
        let Some((pending, delivery)) = self.current_custody(selector, generation)? else {
            return Ok(CustodyProgress::Missing);
        };
        if pending.state() != CustodyState::Pending {
            return Err(Error::Binding);
        }
        let joined = match self.inspect_joined(pending.stage(), &delivery, pending.package_digest())
        {
            Ok(joined) if pending.matches_pending_readback(&joined) => joined,
            Ok(_) => return Ok(CustodyProgress::Unavailable(Error::StageIdentityMismatch)),
            Err(error) => return Ok(CustodyProgress::Unavailable(error)),
        };
        let wall = self.available_sample()?.as_secs();
        let approval = local.as_ref().map_or(Approval::Automatic, Approval::Manual);
        self.state.checkpoint.commit_custody_activating(
            &self.state.source,
            &pending,
            &joined,
            approval,
            wall,
        )?;
        cut(JoinCut::ActivatingCommitted)?;
        Ok(CustodyProgress::Activating)
    }
}
