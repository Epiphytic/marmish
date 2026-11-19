//! Qualify stored retry against the current physical recipient without
//! converting the saved bytes into an observed relay record.

use super::*;
use crate::peer_invitation::store::consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::{DeliveryChallenge, DeliveryPort, retry::{SavedDelivery, retirement::ExpiredSaved}};

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

    pub(in crate::peer_invitation) fn use_expired_delivery_native<'saved, T>(
        &mut self,
        expired: ExpiredSaved<'saved>,
        use_checked: impl FnOnce(CheckedExpiredRoute<'_, 'saved>) -> T,
    ) -> Result<T> {
        if !self.0.current(&expired.port().0) {
            return Err(Failure::Binding);
        }
        self.use_current_native(|route| use_checked(CheckedExpiredRoute { route, expired }))
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) fn fixture_foreign_saved_scope(
        &mut self,
        expired: ExpiredSaved<'_>,
    ) -> Result<()> {
        let scope = &expired.port().0;
        let distinct = Scope {
            origin: scope.origin.clone(),
            recipient: scope.recipient,
            namespace: scope.namespace,
            live: std::sync::atomic::AtomicBool::new(true),
        };
        self.use_expired_delivery_native(
            expired.fixture_with_port(SavedPortWitness(Arc::new(distinct))),
            |_| panic!("a structurally equal foreign scope cannot authorize native work"),
        )
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
    pub(in crate::peer_invitation) fn route(
        &self,
    ) -> &crate::peer_relay_trust::NativeRouteWindow<'_> {
        &self.route
    }

    pub(in crate::peer_invitation) fn challenge_deadline(&self) -> u64 {
        self.challenge.deadline()
    }

    pub(in crate::peer_invitation) fn into_parts(
        self,
    ) -> (
        crate::peer_relay_trust::NativeRouteWindow<'route>,
        DeliveryChallenge<'turn>,
    ) {
        (self.route, self.challenge)
    }
}

/// The exact observed expired flight paired with its current authenticated route.
pub(in crate::peer_invitation) struct CheckedExpiredRoute<'route, 'saved> {
    route: crate::peer_relay_trust::NativeRouteWindow<'route>,
    expired: ExpiredSaved<'saved>,
}

impl<'route, 'saved> CheckedExpiredRoute<'route, 'saved> {
    pub(in crate::peer_invitation) fn into_parts(
        self,
    ) -> (
        crate::peer_relay_trust::NativeRouteWindow<'route>,
        ExpiredSaved<'saved>,
    ) {
        (self.route, self.expired)
    }
}
