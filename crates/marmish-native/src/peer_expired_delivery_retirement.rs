//! Expired saved Delivery authentication grants deletion of its exact SQL row.

use super::*;
use crate::peer_invitation::{Error as OwnerError, NativeWindow};
use crate::peer_invitation::{
    receiver_inventory::package::RetirementImage, store::retry::RetryProof,
};

pub(in crate::peer_invitation) enum ManagedSaved<'saved> {
    Live(PreparedSaved<'saved>),
    Waiting,
    Expired(ExpiredSaved<'saved>),
}

pub(in crate::peer_invitation) struct ExpiredSaved<'saved> {
    prior: PreparedSaved<'saved>,
    observed_at: u64,
}

pub(in crate::peer_invitation) struct HistoricalSample(u64);

impl HistoricalSample {
    pub(in crate::peer_invitation) fn stage_sample(&self) -> std::time::Duration {
        std::time::Duration::from_secs(self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum Retirement {
    Retired,
    Reobserve,
    Transient,
    Stop,
}

pub(in crate::peer_invitation) struct VerifiedRetirement<'saved> {
    saved: &'saved SavedDelivery,
    queued: Box<Queued>,
    key: Selector,
    proof: RetryProof,
    oldest_commit: u64,
}

impl ExpiredSaved<'_> {
    pub(in crate::peer_invitation) fn port(&self) -> &SavedPortWitness {
        &self.prior.port
    }

    pub(in crate::peer_invitation) fn stage(&self) -> ((Binding, FileWitness), Artifact) {
        (
            self.prior.queued.stage(),
            self.prior.queued.prepared.artifact,
        )
    }

    pub(in crate::peer_invitation) fn historical_sample(&self) -> HistoricalSample {
        HistoricalSample(self.prior.sample)
    }

    pub(in crate::peer_invitation) fn check_route(
        &self,
        account: PublicKey,
        origin: &str,
        wall: u64,
    ) -> std::result::Result<(), Retirement> {
        if !self.prior.saved.matches_route(account, origin) {
            return Err(Retirement::Stop);
        }
        if wall < self.observed_at {
            return Err(Retirement::Transient);
        }
        Ok(())
    }
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn observe_managed_saved<'saved>(
        &mut self,
        source: &Source,
        checked: CheckedSavedDelivery<'_, 'saved>,
    ) -> Result<ManagedSaved<'saved>> {
        let (_, observed_at) = checked.observation();
        Ok(match self.observe_saved(source, checked, true)? {
            Some(prior) if observed_at >= prior.expires => {
                self.require_retirement_policy(Selector::from_delivery(&prior.incoming))?;
                ManagedSaved::Expired(ExpiredSaved { prior, observed_at })
            }
            Some(prior) => ManagedSaved::Live(prior),
            None => ManagedSaved::Waiting,
        })
    }

    pub(in crate::peer_invitation) fn verify_retirement<'saved>(
        &self,
        expired: ExpiredSaved<'saved>,
        image: &RetirementImage,
    ) -> Result<VerifiedRetirement<'saved>> {
        let bytes = image.require(&expired).ok_or(Error::Package)?;
        let PreparedSaved {
            saved,
            queued,
            incoming,
            expires,
            sample,
            ..
        } = expired.prior;
        self.check_local_owner(&saved.owner)?;
        let key = Selector::from_delivery(&incoming);
        self.require_retirement_policy(key)?;
        let Some((delivery, package)) = queued
            .row
            .transcript
            .verify_delivery_with_package(
                &self.signer,
                &queued.prepared.offer,
                &queued.row.frame,
                bytes,
                sample,
                incoming,
            )
            .map_err(|_| Error::Package)?
        else {
            return Err(Error::Package);
        };
        if delivery_expiry(&queued, &delivery, &package) != Some(expires) {
            return Err(CursorError::Classification.into());
        }
        let proof = saved
            .snapshot
            .reverify(|_| {
                Some(RetryFinding::Expired {
                    verified_expiry: expires,
                })
            })
            .ok_or(CursorError::Classification)?;
        Ok(VerifiedRetirement {
            saved,
            queued,
            key,
            proof,
            oldest_commit: expired.observed_at,
        })
    }

    pub(in crate::peer_invitation) fn finish_retirement(
        &mut self,
        source: &Source,
        verified: VerifiedRetirement<'_>,
        window: &NativeWindow,
    ) -> Result<()> {
        let VerifiedRetirement {
            saved,
            queued,
            key,
            proof,
            oldest_commit,
        } = verified;
        self.check_local_owner(&saved.owner)?;
        self.require_retirement_policy(key)?;
        let wall = retirement_wall(window, oldest_commit)?;
        self.check_installed_consent_scope(source, saved.account, &saved.origin, wall)?;
        let cx = self.custody_context(source)?;
        self.cursor.finish_retry_guarded(
            proof,
            wall,
            |tx| {
                retirement_wall(window, wall)?;
                cx.verify(tx, wall)?;
                let Ancestry::Queued(current) = cx
                    .installed_ancestry(tx, key)
                    .map_err(schema::cursor_error)?
                else {
                    return Err(CursorError::Binding);
                };
                if !current.matches_exact(&queued) {
                    return Err(CursorError::Stale);
                }
                saved.snapshot.require_current_flight(
                    tx,
                    cx.origin,
                    current.row.transcript.carrier,
                )?;
                rows::advance_wall(tx, wall)
            },
            || retirement_wall(window, wall).map(|_| ()),
        )?;
        Ok(())
    }

    fn require_retirement_policy(&self, key: Selector) -> Result<()> {
        if !matches!(self.custody_mode(key.server)?, Mode::Manual) {
            return Err(Error::Policy);
        }
        Ok(())
    }
}

fn retirement_wall(window: &NativeWindow, floor: u64) -> std::result::Result<u64, CursorError> {
    window
        .sample()
        .map_err(|_| CursorError::ClockRegressed)
        .map(|current| current.as_secs())
        .and_then(|current| {
            if current >= floor {
                Ok(current)
            } else {
                Err(CursorError::ClockRegressed)
            }
        })
}

impl Retirement {
    pub(in crate::peer_invitation) fn failure(error: OwnerError) -> Self {
        match error {
            OwnerError::Cursor(cause) | OwnerError::Checkpoint(Error::Cursor(cause)) => match cause
            {
                CursorError::Stale => Self::Reobserve,
                CursorError::ClockRegressed | CursorError::RelayContract | CursorError::Busy => {
                    Self::Transient
                }
                _ => Self::Stop,
            },
            OwnerError::Clock => Self::Transient,
            _ => Self::Stop,
        }
    }
}
