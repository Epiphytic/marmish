//! One measured Delivery check against the registered receiver's retained state.

use super::package::PackageProof;
use super::store::consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::{
    Observation, Prepared,
    retry::{
        PreparedSaved, SavedDelivery,
        retirement::{ExpiredSaved, ManagedSaved, Retirement},
    },
};
use super::store::consent_checkpoint::{self, Disposition, Status};
use super::*;
use crate::peer_invitation::socket::owner::recipient::{
    CheckedCarrier, CheckedDeliveryRoute, CheckedExpiredRoute, CheckedSavedDelivery,
    PeerRecipientPort,
};
use crate::{
    peer_candidate_startup::Readiness, peer_relay_trust::candidate_registration::ForwardingVerifier,
};
use nostr::key::PublicKey;
use std::sync::atomic::AtomicBool;

use super::super::store::Error as CursorError;
pub(in crate::peer_invitation) use super::super::store::retry::DueCoordinate as SavedDeliveryCoordinate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::peer_invitation) enum SavedVisit {
    Waiting,
    Applied,
    Retired,
    Reobserve,
    Transient,
    Stop,
}

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
        self.observe_managed_delivery_guarded(carrier, || Ok(()))
    }

    pub(in crate::peer_invitation) fn observe_managed_delivery_guarded(
        &mut self,
        carrier: CheckedCarrier<'_>,
        check: impl FnOnce() -> consent_checkpoint::Result<()>,
    ) -> Result<Observation> {
        self.cursor.unchanged()?;
        let outcome =
            self.checkpoint
                .observe_installed_delivery_guarded(&self.source, carrier, check)?;
        self.cursor.unchanged()?;
        Ok(outcome)
    }

    pub(in crate::peer_invitation) fn probe_due_managed_delivery(
        &mut self,
        port: &mut PeerRecipientPort,
    ) -> Result<Option<SavedDeliveryCoordinate>> {
        self.cursor.unchanged()?;
        let coordinate = port.use_current_recipient(|account, origin, wall| {
            self.checkpoint
                .probe_due_installed_delivery(&self.source, account, origin, wall)
        })??;
        self.cursor.unchanged()?;
        Ok(coordinate)
    }

    pub(in crate::peer_invitation) fn service_saved_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        port: &mut PeerRecipientPort,
        coordinate: SavedDeliveryCoordinate,
        readiness: &Readiness,
        stop: &Arc<AtomicBool>,
        forwarding: &ForwardingVerifier,
    ) -> Result<SavedVisit> {
        let window = port.use_current_native(|route| {
            route
                .manual_window(readiness.clone(), Arc::clone(stop), u64::MAX, forwarding)
                .map_err(|_| Error::Clock)
        })??;
        match self.visit_saved_delivery(quota, port, coordinate, &window) {
            Err(
                Error::Cursor(CursorError::Stale)
                | Error::Checkpoint(consent_checkpoint::Error::Cursor(CursorError::Stale)),
            ) => Ok(SavedVisit::Reobserve),
            other => other,
        }
    }

    fn visit_saved_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        port: &mut PeerRecipientPort,
        coordinate: SavedDeliveryCoordinate,
        window: &NativeWindow,
    ) -> Result<SavedVisit> {
        let check = || {
            window
                .sample()
                .map_err(|_| consent_checkpoint::Error::Expired)
        };
        self.cursor.unchanged()?;
        let saved = port
            .use_current_recipient(|account, origin, wall| {
                check()?;
                let file = &self.cursor;
                self.checkpoint.acquire_installed_delivery(
                    &self.source,
                    (account, origin, wall),
                    Some(coordinate),
                    || {
                        require_cursor_identity(file)?;
                        if check()?.as_secs() < wall {
                            return Err(CursorError::ClockRegressed.into());
                        }
                        Ok(())
                    },
                )
            })??
            .ok_or(CursorError::Stale)?;
        self.cursor.unchanged()?;
        let observed = port.use_saved_delivery(&saved, |checked| {
            let (_, wall) = checked.observation();
            let file = &self.cursor;
            self.checkpoint
                .observe_managed_saved_guarded(&self.source, checked, |signed_end| {
                    require_cursor_identity(file)?;
                    let current = check()?.as_secs();
                    if current < wall {
                        return Err(CursorError::ClockRegressed.into());
                    }
                    if current >= signed_end {
                        return Err(CursorError::Stale.into());
                    }
                    Ok(())
                })
        })??;
        match observed {
            ManagedSaved::Waiting => Ok(SavedVisit::Waiting),
            ManagedSaved::Live(prepared) => {
                let challenge = prepared.challenge();
                let signed_end = challenge.deadline();
                let live = || {
                    if check()?.as_secs() >= signed_end {
                        return Err(CursorError::Stale.into());
                    }
                    Ok(())
                };
                let outcome = (|| -> Result<SavedVisit> {
                    live()?;
                    let scoped_window = window.clamp_signed(signed_end)?;
                    let measured = port.use_delivery_native(challenge, |route| {
                        self.measure_delivery_in_window(quota, route, scoped_window, &live)
                    })??;
                    port.use_saved_delivery(&saved, |checked| {
                        self.finish_measured_saved_delivery_guarded(
                            quota, checked, prepared, measured, &live,
                        )
                    })??;
                    Ok(SavedVisit::Applied)
                })();
                let can_have_crossed = matches!(
                    outcome,
                    Err(Error::Clock
                        | Error::Checkpoint(consent_checkpoint::Error::Expired)
                        | Error::Native(StageError::Unavailable))
                );
                if can_have_crossed && matches!(check(), Ok(wall) if wall.as_secs() >= signed_end) {
                    return Ok(SavedVisit::Reobserve);
                }
                outcome
            }
            ManagedSaved::Expired(expired) => {
                let retired = port.use_expired_delivery_native(expired, |checked| {
                    let (route, expired) = checked.into_parts();
                    if let Err(outcome) =
                        expired.check_route(route.account(), route.origin(), route.wall().as_secs())
                    {
                        return outcome;
                    }
                    self.retire_expired_readback(quota, expired, window, || check().map(|_| ()))
                })?;
                Ok(match retired {
                    Retirement::Retired => SavedVisit::Retired,
                    Retirement::Reobserve => SavedVisit::Reobserve,
                    Retirement::Transient => SavedVisit::Transient,
                    Retirement::Stop => SavedVisit::Stop,
                })
            }
        }
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
        self.retire_expired_guarded(quota, checked, readiness, stop, || Ok(()))
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) fn fixture_retire_expired_before_commit(
        &mut self,
        quota: &ReceiverQuota<'_>,
        checked: CheckedExpiredRoute<'_, '_>,
        readiness: crate::peer_candidate_startup::Readiness,
        stop: Arc<std::sync::atomic::AtomicBool>,
        before_commit: impl FnOnce(),
    ) -> Retirement {
        self.retire_expired_guarded(quota, checked, readiness, stop, || {
            before_commit();
            Ok(())
        })
    }

    fn retire_expired_guarded(
        &mut self,
        quota: &ReceiverQuota<'_>,
        checked: CheckedExpiredRoute<'_, '_>,
        readiness: crate::peer_candidate_startup::Readiness,
        stop: Arc<std::sync::atomic::AtomicBool>,
        before_commit: impl FnOnce() -> consent_checkpoint::Result<()>,
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
        self.retire_expired_readback(quota, expired, &window, before_commit)
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) fn fixture_retire_expired_at(
        &mut self,
        quota: &ReceiverQuota<'_>,
        checked: CheckedExpiredRoute<'_, '_>,
        readiness: crate::peer_candidate_startup::Readiness,
        stop: Arc<std::sync::atomic::AtomicBool>,
        fixture_wall: u64,
    ) -> Retirement {
        let (route, expired) = checked.into_parts();
        if let Err(outcome) = expired.check_route(route.account(), route.origin(), fixture_wall) {
            return outcome;
        }
        let wall = Duration::from_secs(fixture_wall);
        super::super::native_fence::fixture_retirement_wall(wall, || {
            let window = NativeWindow::start(
                readiness,
                stop,
                tokio_util::sync::CancellationToken::new(),
                wall,
                fixture_wall + 60,
            )
            .expect("hermetic bounded retirement window");
            self.retire_expired_readback(quota, expired, &window, || Ok(()))
        })
    }

    fn retire_expired_readback(
        &mut self,
        quota: &ReceiverQuota<'_>,
        expired: ExpiredSaved<'_>,
        window: &NativeWindow,
        before_commit: impl FnOnce() -> consent_checkpoint::Result<()>,
    ) -> Retirement {
        let retire = || -> Result<()> {
            self.cursor.unchanged()?;
            let image = self.read_retirement_image(quota, &expired, window)?;
            let proof = self.checkpoint.verify_retirement(expired, &image)?;
            self.recheck_retirement_image(quota, &image, window)?;
            self.cursor.unchanged()?;
            self.checkpoint.finish_retirement(
                &self.source,
                proof,
                window,
                &self.cursor,
                before_commit,
            )?;
            Ok(())
        };
        let outcome = retire();
        #[cfg(test)]
        assert_eq!(window.signs(), 0, "retirement must not sign");
        match outcome {
            Ok(()) => Retirement::Retired,
            Err(_) if window.sample().is_err() => Retirement::Transient,
            Err(error) => Retirement::failure(error),
        }
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) fn measure_delivery(
        &mut self,
        quota: &ReceiverQuota<'_>,
        route: CheckedDeliveryRoute<'_, '_>,
        readiness: crate::peer_candidate_startup::Readiness,
        stop: Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<MeasuredDelivery<'lease>> {
        self.measure_delivery_guarded(quota, route, readiness, stop, &|| Ok(()))
    }

    pub(in crate::peer_invitation) fn measure_delivery_guarded(
        &mut self,
        quota: &ReceiverQuota<'_>,
        route: CheckedDeliveryRoute<'_, '_>,
        readiness: crate::peer_candidate_startup::Readiness,
        stop: Arc<std::sync::atomic::AtomicBool>,
        check: &dyn Fn() -> consent_checkpoint::Result<()>,
    ) -> Result<MeasuredDelivery<'lease>> {
        check()?;
        let window = route
            .route()
            .window(readiness, stop, route.challenge_deadline())
            .map_err(|_| Error::Clock)?;
        self.measure_delivery_in_window(quota, route, window, check)
    }

    fn measure_delivery_in_window(
        &mut self,
        quota: &ReceiverQuota<'_>,
        route: CheckedDeliveryRoute<'_, '_>,
        window: NativeWindow,
        check: &dyn Fn() -> consent_checkpoint::Result<()>,
    ) -> Result<MeasuredDelivery<'lease>> {
        self.cursor.unchanged()?;
        check()?;
        let (route, challenge) = route.into_parts();
        let wall = route.wall().as_secs();
        let turn = challenge.seal();
        let queued = self.checkpoint.challenge_delivery_package(
            &self.source,
            route.account(),
            route.origin(),
            wall,
            challenge,
        )?;
        check()?;
        let package = self.attest_queued_package(quota, &queued, &window)?;
        check()?;
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
        self.finish_measured_delivery_guarded(quota, carrier, prepared, measured, &|| Ok(()))
    }

    pub(in crate::peer_invitation) fn finish_measured_delivery_guarded(
        &mut self,
        quota: &ReceiverQuota<'_>,
        carrier: CheckedCarrier<'_>,
        prepared: Prepared,
        measured: MeasuredDelivery<'lease>,
        check: &dyn Fn() -> consent_checkpoint::Result<()>,
    ) -> Result<Disposition> {
        self.cursor.unchanged()?;
        check()?;
        self.recheck_managed_package(quota, &measured.package, &measured.window)?;
        check()?;
        let outcome = self.checkpoint.finish_managed_delivery_guarded(
            &self.source,
            carrier,
            prepared,
            &measured,
            check,
        )?;
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
        let window = measured.window.clone();
        let outcome = self.finish_measured_saved_delivery_guarded(
            quota,
            checked,
            prepared,
            measured,
            &|| Ok(()),
        )?;
        window.sample()?;
        Ok(outcome)
    }

    fn finish_measured_saved_delivery_guarded(
        &mut self,
        quota: &ReceiverQuota<'_>,
        checked: CheckedSavedDelivery<'_, '_>,
        prepared: PreparedSaved<'_>,
        measured: MeasuredDelivery<'lease>,
        check: &dyn Fn() -> consent_checkpoint::Result<()>,
    ) -> Result<Option<Status>> {
        check()?;
        self.cursor.unchanged()?;
        self.recheck_managed_package(quota, &measured.package, &measured.window)?;
        let outcome = self.checkpoint.commit_managed_saved_delivery(
            &self.source,
            checked,
            prepared,
            &measured,
            &|| {
                require_cursor_identity(&self.cursor)?;
                check()
            },
        )?;
        self.cursor.unchanged()?;
        Ok(outcome)
    }
}

fn require_cursor_identity(cursor: &CheckedFile) -> consent_checkpoint::Result<()> {
    cursor
        .unchanged_identity()
        .map_err(|_| consent_checkpoint::Error::Source)
}
