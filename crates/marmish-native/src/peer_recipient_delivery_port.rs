//! Qualify stored retry against the current physical recipient without
//! converting the saved bytes into an observed relay record.

use super::*;
use crate::peer_invitation::store::consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::retry::SavedDelivery;

pub(in crate::peer_invitation) struct CheckedSavedDelivery<'a> {
    scope: &'a Arc<Scope>,
    saved: &'a SavedDelivery,
    wall: u64,
}

pub(in crate::peer_invitation) struct SavedPortWitness(Arc<Scope>);

impl<'a> CheckedSavedDelivery<'a> {
    pub(in crate::peer_invitation) fn observation(&self) -> (&'a SavedDelivery, u64) {
        (self.saved, self.wall)
    }

    pub(in crate::peer_invitation) fn witness(&self) -> SavedPortWitness {
        SavedPortWitness(Arc::clone(self.scope))
    }

    pub(in crate::peer_invitation) fn matches(&self, witness: &SavedPortWitness) -> bool {
        Arc::ptr_eq(self.scope, &witness.0)
    }
}

impl PeerRecipientPort {
    pub(in crate::peer_invitation) fn use_saved_delivery<T>(
        &mut self,
        saved: &SavedDelivery,
        use_checked: impl FnOnce(CheckedSavedDelivery<'_>) -> T,
    ) -> Result<T> {
        let (account, wall) = self.check_observation()?;
        if !saved.matches_route(account, &self.0.scope.origin) {
            return Err(Failure::Binding);
        }
        Ok(use_checked(CheckedSavedDelivery {
            scope: &self.0.scope,
            saved,
            wall,
        }))
    }
}
