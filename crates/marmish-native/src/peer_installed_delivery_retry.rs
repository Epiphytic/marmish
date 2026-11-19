//! Requalify one revisioned stored Delivery after its Consent settles.

use super::*;
use crate::peer_invitation::{
    socket::owner::recipient::{CheckedSavedDelivery, SavedPortWitness},
    store::{RetryFinding, retry::Snapshot},
};

pub(in crate::peer_invitation) struct SavedDelivery {
    snapshot: Snapshot,
    owner: Arc<()>,
    account: PublicKey,
    origin: String,
}

impl SavedDelivery {
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
    saved: &'saved SavedDelivery,
    queued: Box<Queued>,
    incoming: IncomingDelivery,
    expires: u64,
    sample: u64,
}

impl PreparedSaved<'_> {
    pub(in crate::peer_invitation) fn native_coordinates(&self) -> ((Binding, FileWitness), u64) {
        (self.queued.stage(), self.sample)
    }
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn due_installed_delivery(
        &mut self,
        source: &Source,
        account: PublicKey,
        origin: &str,
        wall: u64,
    ) -> Result<Option<SavedDelivery>> {
        self.check_installed_consent_scope(source, account, origin, wall)?;
        let cx = self.custody_context(source)?;
        let due = self.cursor.due_with_fence(cx.origin, wall, |tx| {
            cx.verify(tx, wall)?;
            rows::advance_wall(tx, wall)
        })?;
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
                let Ancestry::Queued(queued) = cx.installed_ancestry(tx, key)? else {
                    return Err(CursorError::Stale.into());
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
            self.cursor
                .retry_again_with_fence(&saved.snapshot, wall, |tx| {
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
                })?;
            return Ok(None);
        }

        // After expiry, historical authentication grants retirement only.
        let sample = wall.min(expires.checked_sub(1).ok_or(Error::Expired)?);
        Ok(Some(PreparedSaved {
            port: checked.witness(),
            saved,
            queued,
            incoming: *incoming,
            expires,
            sample,
        }))
    }

    pub(in crate::peer_invitation) fn commit_saved_delivery(
        &mut self,
        source: &Source,
        checked: CheckedSavedDelivery<'_, '_>,
        prepared: PreparedSaved<'_>,
        native: &NativeAttestation,
    ) -> Result<Option<Status>> {
        let (saved, wall) = checked.observation();
        if !checked.matches(&prepared.port) || !std::ptr::eq(saved, prepared.saved) {
            return Err(Error::Source);
        }
        self.check_local_owner(&saved.owner)?;
        let key = Selector::from_delivery(&prepared.incoming);
        let queued =
            self.read_installed_consent(source, saved.account, &saved.origin, wall, |tx, cx| {
                let existing = cx.current_delivery_consent(tx, key, &prepared.queued)?;
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
        let verified =
            self.verify_custody_delivery(&consent, native, prepared.incoming, verification_wall)?;
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
                        &delivery,
                        &package,
                        wall,
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
        self.cursor.finish_retry_with_fence(proof, wall, |tx| {
            cx.verify(tx, wall)?;
            cx.current_delivery_consent(tx, key, &prepared.queued)
                .map_err(schema::cursor_error)?;
            saved.snapshot.require_current_flight(
                tx,
                cx.origin,
                prepared.queued.row.transcript.carrier,
            )?;
            rows::advance_wall(tx, wall)
        })?;
        Ok(None)
    }
}
