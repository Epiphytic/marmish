//! Exact carrier disposition. Advancing native custody is a separate owner turn.

use super::*;
use crate::peer_invitation::store::{
    Auth, Metadata, RawRecord, consent_checkpoint::provisioning::prepared_consent::custody::Replay,
};

impl PreparedConsentOwner<'_> {
    pub(in crate::peer_invitation::prepared_consent) fn receive_custody_record(
        &mut self,
        raw: RawRecord<'_>,
    ) -> Result<Disposition> {
        let sample = self.available_sample()?;
        let wall = sample.as_secs();
        // The fixture's native package attestation is not quota scoped.
        if self.state.checkpoint.is_installed_custody() {
            return Err(Error::Binding);
        }
        let codec = self.state.checkpoint.custody_codec();
        let Ok(outer) = codec.verify_client_outer(raw.bytes) else {
            return self.commit_invalid_custody(raw, wall);
        };
        if !outer.matches_index(raw.id, raw.stamp) {
            return Err(consent_checkpoint::Error::Index.into());
        }

        match self
            .state
            .checkpoint
            .custody_replay(&self.state.source, raw, wall)?
        {
            Replay::AlreadyApplied(disposition) | Replay::Invalid(disposition) => {
                return Ok(disposition);
            }
            Replay::Absent => {}
        }
        let incoming = match codec.classify_verified_client(&self.state.signer, outer) {
            Ok(ClientCarrier::Offer) => {
                return Ok(self.state.checkpoint.admit(&self.state.source, raw, wall)?);
            }
            Ok(ClientCarrier::Delivery(incoming)) => incoming,
            Err(_) => return self.commit_invalid_custody(raw, wall),
        };

        let selector = Selector::from_delivery(&incoming);
        let recovered =
            match self
                .state
                .checkpoint
                .custody_consent(&self.state.source, selector, wall)
            {
                Ok(Some(recovered)) => recovered,
                Ok(None)
                | Err(consent_checkpoint::Error::Policy | consent_checkpoint::Error::Expired) => {
                    return self.commit_invalid_custody(raw, wall);
                }
                Err(error) => return Err(error.into()),
            };
        if let Some(saved) = recovered.evidence() {
            // Both a stable mismatch and a second authentic carrier are invalid.
            // Corruption of the saved local authority stops cursor progress.
            let _authenticated_conflict = codec
                .verify_custody_conflict(&self.state.signer, saved, *incoming, wall)
                .map_err(|_| consent_checkpoint::Error::Package)?;
            return self.commit_invalid_custody(raw, wall);
        }

        let native = self.attest(recovered.stage(), sample)?;
        let wall = self.available_sample()?.as_secs();
        let verified = self
            .state
            .checkpoint
            .verify_custody_delivery(&recovered, &native, *incoming, wall);
        let (delivery, package) = match verified {
            Ok(Some(verified)) => verified,
            Ok(None) | Err(consent_checkpoint::Error::Expired) => {
                return self.commit_invalid_custody(raw, wall);
            }
            Err(error) => return Err(error.into()),
        };
        self.verify_custody(&native)?;
        if !recovered.is_settled() {
            return Ok(Disposition {
                status: Status::Deferred,
                receipt: self.state.checkpoint.cursor.defer(
                    self.state.origin,
                    raw,
                    Metadata {
                        auth: Auth::FlightVerified,
                        expires: delivery.flight().expires_at,
                        prerequisite: Some(recovered.carrier()),
                    },
                    wall,
                )?,
            });
        }
        match self.state.checkpoint.commit_custody_held(
            &self.state.source,
            raw,
            &recovered,
            &delivery,
            &package,
            wall,
        ) {
            Ok(disposition) => Ok(disposition),
            Err(consent_checkpoint::Error::Expired) => self.commit_invalid_custody(raw, wall),
            Err(error) => Err(error.into()),
        }
    }

    fn commit_invalid_custody(&mut self, raw: RawRecord<'_>, wall: u64) -> Result<Disposition> {
        Ok(Disposition {
            status: Status::Invalid,
            receipt: self
                .state
                .checkpoint
                .cursor
                .commit_invalid(self.state.origin, raw, wall)?,
        })
    }

    pub(in crate::peer_invitation) fn retry_custody_due(&mut self) -> Result<Option<Status>> {
        if !self.custody {
            return Err(Error::Binding);
        }
        let wall = self.available_sample()?.as_secs();
        let Some(snapshot) = self.state.checkpoint.cursor.due(self.state.origin, wall)? else {
            return Ok(None);
        };
        let raw = snapshot.record();
        let disposition = self.receive_custody_record(raw)?;
        if disposition.status == Status::Deferred {
            let wall = self.available_sample()?.as_secs();
            let current = self
                .state
                .checkpoint
                .cursor
                .existing(self.state.origin, raw.id)?
                .ok_or(store::Error::Stale)?;
            let saved = current.record();
            if saved.stamp != raw.stamp || saved.bytes != raw.bytes {
                return Err(store::Error::Stale.into());
            }
            if current.retry_at <= wall {
                self.state.checkpoint.cursor.retry_again(&current, wall)?;
            }
        }
        Ok(Some(disposition.status))
    }
}
