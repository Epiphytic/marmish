//! One measured Delivery check against the registered receiver's retained state.

use super::package::PackageProof;
use super::store::consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::{
    Observation, Prepared,
    retry::{
        PreparedSaved, SavedDelivery,
        retirement::{ManagedSaved, Retirement},
    },
};
use super::store::consent_checkpoint::{self, Disposition, Status};
use super::*;
use crate::peer_invitation::socket::owner::recipient::{
    CheckedCarrier, CheckedDeliveryRoute, CheckedExpiredRoute, CheckedSavedDelivery,
};
use nostr::key::PublicKey;

/// Construction requires a charged copy, the exact queued flight, and a fresh
/// measurement of the original on the same quota owner.
pub(in crate::peer_invitation) struct MeasuredDelivery<'lease> {
    package: PackageProof<'lease>,
    window: NativeWindow,
    turn: Arc<()>,
}

impl MeasuredDelivery<'_> {
    pub(in crate::peer_invitation) fn package(&self) -> &PackageProof<'_> {
        &self.package
    }

    pub(in crate::peer_invitation) fn require_turn(
        &self,
        turn: &Arc<()>,
    ) -> consent_checkpoint::Result<()> {
        if !Arc::ptr_eq(&self.turn, turn) {
            return Err(consent_checkpoint::Error::Source);
        }
        Ok(())
    }

    pub(in crate::peer_invitation) fn window(&self) -> &NativeWindow {
        &self.window
    }
}

impl<'lease> RecoveredOwner<'lease> {
    pub(in crate::peer_invitation) fn observe_managed_delivery(
        &mut self,
        carrier: CheckedCarrier<'_>,
    ) -> Result<Observation> {
        self.cursor.unchanged()?;
        let outcome = self
            .checkpoint
            .observe_installed_delivery(&self.source, carrier)?;
        self.cursor.unchanged()?;
        Ok(outcome)
    }

    pub(in crate::peer_invitation) fn due_managed_delivery(
        &mut self,
        account: PublicKey,
        origin: &str,
        wall: u64,
    ) -> Result<Option<SavedDelivery>> {
        self.cursor.unchanged()?;
        let outcome =
            self.checkpoint
                .due_installed_delivery(&self.source, account, origin, wall)?;
        self.cursor.unchanged()?;
        Ok(outcome)
    }

    pub(in crate::peer_invitation) fn observe_managed_saved_delivery<'saved>(
        &mut self,
        checked: CheckedSavedDelivery<'_, 'saved>,
    ) -> Result<ManagedSaved<'saved>> {
        self.cursor.unchanged()?;
        let outcome = self
            .checkpoint
            .observe_managed_saved(&self.source, checked)?;
        self.cursor.unchanged()?;
        Ok(outcome)
    }

    pub(in crate::peer_invitation) fn retire_expired_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        checked: CheckedExpiredRoute<'_, '_>,
        readiness: crate::peer_candidate_startup::Readiness,
        stop: Arc<std::sync::atomic::AtomicBool>,
    ) -> Retirement {
        let (route, expired) = checked.into_parts();
        if let Err(outcome) =
            expired.check_route(route.account(), route.origin(), route.wall().as_secs())
        {
            return outcome;
        }
        // The cutoff authenticates past bytes. Only current route and health
        // ceilings can authorize the bounded local cleanup.
        let Ok(window) = route.window(readiness, stop, u64::MAX) else {
            return Retirement::Transient;
        };
        let retire = || -> Result<()> {
            self.cursor.unchanged()?;
            let image = self.read_retirement_image(quota, &expired, &window)?;
            let proof = self.checkpoint.verify_retirement(expired, &image)?;
            self.recheck_retirement_image(quota, &image, &window)?;
            self.cursor.unchanged()?;
            self.checkpoint
                .finish_retirement(&self.source, proof, &window)?;
            Ok(())
        };
        match retire() {
            Ok(()) => Retirement::Retired,
            Err(_) if window.sample().is_err() => Retirement::Transient,
            Err(error) => Retirement::failure(error),
        }
    }

    pub(in crate::peer_invitation) fn measure_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        route: CheckedDeliveryRoute<'_, '_>,
        readiness: crate::peer_candidate_startup::Readiness,
        stop: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<MeasuredDelivery<'lease>> {
        self.cursor.unchanged()?;
        let (route, challenge) = route.into_parts();
        let wall = route.wall().as_secs();
        let window = route
            .window(readiness, stop, challenge.deadline())
            .map_err(|_| Error::Clock)?;
        let turn = challenge.seal();
        let queued = self.checkpoint.challenge_delivery_package(
            &self.source,
            route.account(),
            route.origin(),
            wall,
            challenge,
        )?;
        let package = self.attest_queued_package(quota, &queued, &window)?;
        self.cursor.unchanged()?;
        window.sample()?;
        Ok(MeasuredDelivery {
            package,
            window,
            turn,
        })
    }

    pub(in crate::peer_invitation) fn finish_measured_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        carrier: CheckedCarrier<'_>,
        prepared: Prepared,
        measured: MeasuredDelivery<'lease>,
    ) -> Result<Disposition> {
        self.cursor.unchanged()?;
        self.recheck_managed_package(quota, &measured.package, &measured.window)?;
        let outcome =
            self.checkpoint
                .finish_managed_delivery(&self.source, carrier, prepared, &measured)?;
        self.cursor.unchanged()?;
        measured.window.sample()?;
        Ok(outcome)
    }

    pub(in crate::peer_invitation) fn finish_measured_saved_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        checked: CheckedSavedDelivery<'_, '_>,
        prepared: PreparedSaved<'_>,
        measured: MeasuredDelivery<'lease>,
    ) -> Result<Option<Status>> {
        self.cursor.unchanged()?;
        self.recheck_managed_package(quota, &measured.package, &measured.window)?;
        let outcome = self.checkpoint.commit_managed_saved_delivery(
            &self.source,
            checked,
            prepared,
            &measured,
        )?;
        self.cursor.unchanged()?;
        measured.window.sample()?;
        Ok(outcome)
    }
}
