//! Exact opt-in Manual v2 native Pending transitions; no relay publication.

use super::*;
use crate::{
    peer_invitation::{
        TurnFence, receiver_inventory::v2_pending::PreparedWelcome,
        socket::owner::recipient::PeerRecipientPort,
    },
    peer_quota_vfs::dispatch::ReceiverQuota,
};

impl Checkpoint {
    pub(in crate::peer_invitation) fn read_installed_join_custody(
        &mut self,
        source: &Source,
        port: &mut PeerRecipientPort,
        selector: Selector,
        generation: u64,
    ) -> Result<(HeldEvidence, Delivery, u64)> {
        port.use_current_recipient(|account, origin, wall| {
            self.check_installed_consent_scope(source, account, origin, wall)?;
            let held = self
                .read_custody(source, selector, generation, wall)?
                .ok_or(Error::Missing)?;
            let delivery = self.current_installed_delivery(&held, wall)?;
            Ok((held, delivery, wall))
        })
        .map_err(|_| Error::Source)?
    }

    pub(in crate::peer_invitation) fn start_installed_join(
        &mut self,
        source: &Source,
        port: &mut PeerRecipientPort,
        prior: &HeldEvidence,
        prepared: &PreparedWelcome<'_>,
        quota: &ReceiverQuota<'_>,
        fence: &TurnFence<'_>,
    ) -> Result<JoinStarted> {
        port.use_current_recipient(|account, origin, wall| {
            self.check_installed_consent_scope(source, account, origin, wall)?;
            let delivery = self.current_installed_delivery(prior, wall)?;
            if !prepared.matches_quota(quota) || !prepared.matches(prior, &delivery) {
                return Err(CursorError::Binding.into());
            }
            self.transition_custody_guarded(source, prior, wall, Change::Start, || {
                fence.wall().map_err(|_| Error::Source)?;
                Ok(())
            })?;
            Ok(JoinStarted {
                owner: self.owner.clone(),
                stage: prior.stage,
                carrier: prior.saved.carrier,
            })
        })
        .map_err(|_| Error::Source)?
    }

    /// A current Pending replay verifies the same native coordinates and
    /// leaves custody untouched. JoinStarted can advance exactly once.
    pub(in crate::peer_invitation) fn commit_installed_pending(
        &mut self,
        source: &Source,
        port: &mut PeerRecipientPort,
        prior: &HeldEvidence,
        joined: &InstalledJoinedReadback<'_>,
        quota: &ReceiverQuota<'_>,
        fence: &TurnFence<'_>,
    ) -> Result<()> {
        port.use_current_recipient(|account, origin, wall| {
            self.check_installed_consent_scope(source, account, origin, wall)?;
            let delivery = self.current_installed_delivery(prior, wall)?;
            if !joined.matches_quota(quota) || !joined.matches(prior, &delivery) {
                return Err(CursorError::Binding.into());
            }
            if let Progress::Pending { joined: saved, .. } = &prior.saved.progress {
                let selector = Selector::from(prior.offer());
                let current = self
                    .read_custody(source, selector, prior.stage().0.coordinates().1, wall)?
                    .ok_or(Error::Missing)?;
                if !current.same_turn(prior)
                    || (
                        saved.native.group.as_slice(),
                        saved.native.epoch,
                        saved.native.epoch_identity,
                        saved.native.own_leaf,
                    ) != joined.coordinates()
                {
                    return Err(CursorError::Stale.into());
                }
                fence.wall().map_err(|_| Error::Source)?;
                return Ok(());
            }
            self.transition_custody_guarded(
                source,
                prior,
                wall,
                Change::QualifyInstalled(joined),
                || {
                    fence.wall().map_err(|_| Error::Source)?;
                    Ok(())
                },
            )
        })
        .map_err(|_| Error::Source)?
    }

    fn current_installed_delivery(&self, held: &HeldEvidence, wall: u64) -> Result<Delivery> {
        self.check_local_owner(&held.owner)?;
        if held.is_active()
            || held.saved.automatic
            || held.stage().0.coordinates().0
            || held.offer().version() != ControlVersion::V2
            || !matches!(
                self.custody_mode(held.offer().flight().author)?,
                Mode::Manual
            )
            || !matches!(
                held.state(),
                CustodyState::Held | CustodyState::JoinStarted | CustodyState::Pending
            )
        {
            return Err(Error::Policy);
        }
        if wall
            >= held
                .pending_deadline()
                .unwrap_or_else(|| held.held_deadline())
        {
            return Err(Error::Expired);
        }
        self.codec
            .reopen_custodied_delivery(&self.signer, held, wall)
            .map_err(|_| Error::Package)
    }
}
