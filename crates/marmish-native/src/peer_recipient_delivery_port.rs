//! Qualify stored retry against the current physical recipient without
//! converting the saved bytes into an observed relay record.

use super::*;
use crate::peer_invitation::store::consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::{DeliveryChallenge, DeliveryPort, retry::SavedDelivery};

pub(in crate::peer_invitation) struct CheckedSavedDelivery<'port, 'saved> {
    scope: &'port Arc<Scope>,
    saved: &'saved SavedDelivery,
    wall: u64,
}

pub(in crate::peer_invitation) struct SavedPortWitness(Arc<Scope>);

impl<'saved> CheckedSavedDelivery<'_, 'saved> {
    pub(in crate::peer_invitation) fn observation(&self) -> (&'saved SavedDelivery, u64) {
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
    pub(in crate::peer_invitation) fn use_delivery_native<'turn, T>(
        &mut self,
        challenge: DeliveryChallenge<'turn>,
        use_checked: impl FnOnce(CheckedDeliveryRoute<'_, 'turn>) -> T,
    ) -> Result<T> {
        let scope = match challenge.port() {
            DeliveryPort::Observed(witness) => &witness.scope,
            DeliveryPort::Saved(witness) => &witness.0,
        };
        if !self.0.current(scope) {
            return Err(Failure::Binding);
        }
        self.use_current_native(|route| use_checked(CheckedDeliveryRoute { route, challenge }))
    }

    pub(in crate::peer_invitation) fn use_saved_delivery<'saved, T>(
        &mut self,
        saved: &'saved SavedDelivery,
        use_checked: impl FnOnce(CheckedSavedDelivery<'_, 'saved>) -> T,
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

/// Origin, horizon and cancellation for the same port that supplied the paired carrier.
pub(in crate::peer_invitation) struct CheckedDeliveryRoute<'route, 'turn> {
    route: crate::peer_relay_trust::NativeRouteWindow<'route>,
    challenge: DeliveryChallenge<'turn>,
}

impl<'route, 'turn> CheckedDeliveryRoute<'route, 'turn> {
    pub(in crate::peer_invitation) fn into_parts(
        self,
    ) -> (
        crate::peer_relay_trust::NativeRouteWindow<'route>,
        DeliveryChallenge<'turn>,
    ) {
        (self.route, self.challenge)
    }
}
