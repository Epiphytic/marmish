//! Requalify one revisioned stored Delivery after its Consent settles.

use super::*;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "peer_expired_delivery_retirement.rs"]
pub(in crate::peer_invitation) mod retirement;

use crate::peer_invitation::{
    socket::owner::recipient::{CheckedSavedDelivery, SavedPortWitness},
    store::retry::{self, DueCoordinate, RetryFinding, Snapshot},
};

pub(in crate::peer_invitation) struct SavedDelivery {
    snapshot: Snapshot,
    owner: Arc<()>,
    account: PublicKey,
    origin: String,
}

impl SavedDelivery {
    #[cfg(test)]
    pub(in crate::peer_invitation) fn fixture_id(&self) -> [u8; 32] {
        self.snapshot.record().id
    }

    pub(in crate::peer_invitation) fn matches_route(
        &self,
        account: PublicKey,
        origin: &str,
    ) -> bool {
        self.account == account && self.origin == origin
    }
}

pub(in crate::peer_invitation) struct PreparedSaved<'saved> {
    port: SavedPortWitness,
    turn: Arc<()>,
    saved: &'saved SavedDelivery,
    queued: Box<Queued>,
    incoming: IncomingDelivery,
    expires: u64,
    sample: u64,
}

impl PreparedSaved<'_> {
    #[cfg(test)]
    pub(in crate::peer_invitation) fn native_coordinates(&self) -> ((Binding, FileWitness), u64) {
        (self.queued.stage(), self.sample)
    }

    pub(in crate::peer_invitation) fn challenge(&self) -> DeliveryChallenge<'_> {
        DeliveryChallenge {
            queued: &self.queued,
            turn: &self.turn,
            port: DeliveryPort::Saved(&self.port),
            deadline: self.expires,
            observation: self.sample,
        }
    }
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn probe_due_installed_delivery(
        &mut self,
        source: &Source,
        account: PublicKey,
        origin: &str,
        wall: u64,
    ) -> Result<Option<DueCoordinate>> {
        self.read_installed_consent(source, account, origin, wall, |tx, cx| {
            Ok(retry::probe_due(tx, cx.origin, wall)?)
        })
    }

    pub(in crate::peer_invitation) fn due_installed_delivery(
        &mut self,
        source: &Source,
        account: PublicKey,
        origin: &str,
        wall: u64,
    ) -> Result<Option<SavedDelivery>> {
        self.acquire_installed_delivery(source, (account, origin, wall), None, || Ok(()))
    }

    pub(in crate::peer_invitation) fn acquire_installed_delivery(
        &mut self,
        source: &Source,
        route: (PublicKey, &str, u64),
        expected: Option<DueCoordinate>,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<Option<SavedDelivery>> {
        let (account, origin, wall) = route;
        self.check_installed_consent_scope(source, account, origin, wall)?;
        let cx = self.custody_context(source)?;
        let due = self.cursor.acquire_due(
            cx.origin,
            wall,
            expected,
            |tx| {
                cx.verify(tx, wall)?;
                rows::advance_wall(tx, wall)
            },
            || before_commit().map_err(schema::cursor_error),
        )?;
        Ok(due.map(|snapshot| SavedDelivery {
            snapshot,
            owner: cx.owner,
            account,
            origin: origin.to_owned(),
        }))
    }

    pub(in crate::peer_invitation) fn observe_saved_delivery<'saved>(
        &mut self,
        source: &Source,
        checked: CheckedSavedDelivery<'_, 'saved>,
    ) -> Result<Option<PreparedSaved<'saved>>> {
        self.observe_saved(source, checked, false, |_| Ok(()))
    }

    fn observe_saved<'saved>(
        &mut self,
        source: &Source,
        checked: CheckedSavedDelivery<'_, 'saved>,
        retirement: bool,
        before_retry_commit: impl FnOnce(u64) -> Result<()>,
    ) -> Result<Option<PreparedSaved<'saved>>> {
        let (saved, wall) = checked.observation();
        self.check_local_owner(&saved.owner)?;
        let raw = saved.snapshot.record();
        let cx = self.custody_context(source)?;
        let incoming = match cx.codec.classify_client(&cx.signer, raw.bytes) {
            Ok(wire::ClientCarrier::Delivery(incoming))
                if incoming.matches_index(raw.id, raw.stamp) =>
            {
                incoming
            }
            _ => return Err(Error::Index),
        };
        let key = Selector::from_delivery(&incoming);
        let (queued, expires) =
            self.read_installed_consent(source, saved.account, &saved.origin, wall, |tx, cx| {
                let retiring = retirement && wall >= saved.snapshot.metadata.expires;
                if retiring {
                    let prerequisite = saved.snapshot.metadata.prerequisite.ok_or(Error::Index)?;
                    saved
                        .snapshot
                        .require_current_flight(tx, cx.origin, prerequisite)?;
                }
                let Ancestry::Queued(queued) = cx.installed_ancestry(tx, key)? else {
                    return Err(if retiring {
                        Error::Missing
                    } else {
                        CursorError::Stale.into()
                    });
                };
                saved.snapshot.require_current_flight(
                    tx,
                    cx.origin,
                    queued.row.transcript.carrier,
                )?;
                let signed_upper = queued
                    .latest_delivery()
                    .min(incoming.expires_at())
                    .checked_sub(MARGIN)
                    .ok_or(Error::Expired)?;
                let expires = saved.snapshot.metadata.expires;
                if expires > signed_upper {
                    return Err(CursorError::Classification.into());
                }
                Ok((queued, expires))
            })?;

        if wall < expires && queued.row.ack_at.is_none() {
            self.cursor.retry_again_guarded(
                &saved.snapshot,
                wall,
                |tx| {
                    cx.verify(tx, wall)?;
                    let current = cx
                        .current_delivery_consent(tx, key, &queued)
                        .map_err(schema::cursor_error)?;
                    saved.snapshot.require_current_flight(
                        tx,
                        cx.origin,
                        queued.row.transcript.carrier,
                    )?;
                    if current.row.ack_at.is_some() {
                        return Err(CursorError::Stale);
                    }
                    rows::advance_wall(tx, wall)
                },
                || before_retry_commit(expires).map_err(schema::cursor_error),
            )?;
            return Ok(None);
        }

        // After expiry, historical authentication grants retirement only.
        let sample = wall.min(expires.checked_sub(1).ok_or(Error::Expired)?);
        Ok(Some(PreparedSaved {
            port: checked.witness(),
            turn: Arc::new(()),
            saved,
            queued,
            incoming: *incoming,
            expires,
            sample,
        }))
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) fn commit_saved_delivery(
        &mut self,
        source: &Source,
        checked: CheckedSavedDelivery<'_, '_>,
        prepared: PreparedSaved<'_>,
        native: &NativeAttestation,
    ) -> Result<Option<Status>> {
        self.commit_saved(source, checked, prepared, Native::Fixture(native))
    }

    pub(in crate::peer_invitation) fn commit_managed_saved_delivery(
        &mut self,
        source: &Source,
        checked: CheckedSavedDelivery<'_, '_>,
        prepared: PreparedSaved<'_>,
        measured: &MeasuredDelivery<'_>,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Option<Status>> {
        if checked.observation().1 >= prepared.expires {
            return Err(Error::Expired);
        }
        measured.require_turn(&prepared.turn)?;
        self.commit_saved(source, checked, prepared, Native::Managed(measured, check))
    }

    fn commit_saved(
        &mut self,
        source: &Source,
        checked: CheckedSavedDelivery<'_, '_>,
        prepared: PreparedSaved<'_>,
        native: Native<'_, '_>,
    ) -> Result<Option<Status>> {
        let (saved, wall) = checked.observation();
        native.check_at(wall)?;
        if !checked.matches(&prepared.port) || !std::ptr::eq(saved, prepared.saved) {
            return Err(Error::Source);
        }
        self.check_local_owner(&saved.owner)?;
        let key = Selector::from_delivery(&prepared.incoming);
        let queued =
            self.read_installed_consent(source, saved.account, &saved.origin, wall, |tx, cx| {
                let existing = cx.current_delivery_consent(tx, key, &prepared.queued)?;
                native.require_same_turn(&existing, &prepared.queued)?;
                saved.snapshot.require_current_flight(
                    tx,
                    cx.origin,
                    existing.row.transcript.carrier,
                )?;
                Ok(existing)
            })?;
        let consent = CustodyConsent {
            owner: Arc::clone(&saved.owner),
            authority: ConsentAuthority::Fresh(queued),
        };
        let expired = wall >= prepared.expires;
        let verification_wall = if expired { prepared.sample } else { wall };
        let ConsentAuthority::Fresh(queued) = &consent.authority else {
            return Err(Error::Custodied);
        };
        let verified = self.verify_installed_delivery(
            &consent,
            queued,
            &native,
            prepared.incoming,
            verification_wall,
        )?;
        match verified {
            Some((delivery, package)) => {
                if delivery_expiry(&prepared.queued, &delivery, &package) != Some(prepared.expires)
                {
                    return Err(CursorError::Classification.into());
                }
                if !expired {
                    let disposition = self.commit_custody_held_input(
                        source,
                        HeldInput::Saved(&saved.snapshot),
                        &consent,
                        (&delivery, &package),
                        wall,
                        || native.check_at(wall),
                    )?;
                    return Ok(Some(disposition.status));
                }
            }
            None if expired => return Err(Error::Package),
            None => {}
        }

        let finding = if expired {
            RetryFinding::Expired {
                verified_expiry: prepared.expires,
            }
        } else {
            RetryFinding::Invalid
        };
        let proof = saved
            .snapshot
            .reverify(|_| Some(finding))
            .ok_or(CursorError::Classification)?;
        let cx = self.custody_context(source)?;
        self.cursor.finish_retry_guarded(
            proof,
            wall,
            |tx| {
                native.check_at(wall).map_err(schema::cursor_error)?;
                cx.verify(tx, wall)?;
                let existing = cx
                    .current_delivery_consent(tx, key, &prepared.queued)
                    .map_err(schema::cursor_error)?;
                native
                    .require_same_turn(&existing, &prepared.queued)
                    .map_err(schema::cursor_error)?;
                saved.snapshot.require_current_flight(
                    tx,
                    cx.origin,
                    prepared.queued.row.transcript.carrier,
                )?;
                rows::advance_wall(tx, wall)
            },
            || native.check_at(wall).map_err(schema::cursor_error),
        )?;
        Ok(None)
    }
}
