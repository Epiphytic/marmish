//! Classification and transactions for checked installed recipient Delivery.

use super::super::super::super::installed;
use super::offer_ingress::{InstalledCarrier, OfferIngress};
use super::*;
use crate::peer_invitation::receiver_inventory::MeasuredDelivery;
use crate::peer_invitation::socket::owner::recipient::{
    CarrierWitness, CheckedCarrier, CheckedPage, SavedPortWitness,
};
use crate::peer_invitation::store::{Auth, Metadata, Page, Receipt, ScanStart, pages::Cursor};

enum Native<'a, 'lease> {
    #[cfg(test)]
    Fixture(&'a NativeAttestation),
    Managed(&'a MeasuredDelivery<'lease>),
}

impl Native<'_, '_> {
    fn check_at(&self, wall: u64) -> Result<()> {
        match self {
            #[cfg(test)]
            Self::Fixture(_) => Ok(()),
            Self::Managed(measured) => measured
                .window()
                .sample()
                .map_err(|_| Error::Expired)
                .and_then(|sample| {
                    if sample.as_secs() < wall {
                        return Err(CursorError::ClockRegressed.into());
                    }
                    Ok(())
                }),
        }
    }

    fn require_same_turn(&self, observed: &Queued, expected: &Queued) -> Result<()> {
        if matches!(self, Self::Managed(_)) && !observed.matches_exact(expected) {
            return Err(CursorError::Stale.into());
        }
        Ok(())
    }
}

const OUTER_HORIZON: u64 = 90 * 3600;

pub(in crate::peer_invitation) enum Observation {
    Receipted(Disposition),
    Inspect(Box<Prepared>),
}

pub(in crate::peer_invitation) struct Prepared {
    witness: CarrierWitness,
    turn: Arc<()>,
    consent: CustodyConsent,
    incoming: IncomingDelivery,
    observed_at: u64,
    deadline: u64,
}

impl Prepared {
    #[cfg(test)]
    pub(in crate::peer_invitation) fn native_coordinates(&self) -> ((Binding, FileWitness), u64) {
        (self.consent.stage(), self.observed_at)
    }

    pub(in crate::peer_invitation) fn challenge(&self) -> Result<DeliveryChallenge<'_>> {
        let ConsentAuthority::Fresh(queued) = &self.consent.authority else {
            return Err(Error::Custodied);
        };
        Ok(DeliveryChallenge {
            queued,
            turn: &self.turn,
            port: DeliveryPort::Observed(&self.witness),
            deadline: self.deadline,
            observation: self.observed_at,
        })
    }
}

/// The exact outbox generation and bytes authenticated by a physical read.
pub(in crate::peer_invitation) struct DeliveryChallenge<'a> {
    queued: &'a Queued,
    turn: &'a Arc<()>,
    port: DeliveryPort<'a>,
    deadline: u64,
    observation: u64,
}

pub(in crate::peer_invitation) enum DeliveryPort<'a> {
    Observed(&'a CarrierWitness),
    Saved(&'a SavedPortWitness),
}

impl DeliveryChallenge<'_> {
    pub(in crate::peer_invitation) fn deadline(&self) -> u64 {
        self.deadline
    }

    pub(in crate::peer_invitation) fn port(&self) -> &DeliveryPort<'_> {
        &self.port
    }

    pub(in crate::peer_invitation) fn seal(&self) -> Arc<()> {
        Arc::clone(self.turn)
    }
}

enum Ancestry {
    Absent,
    Terminal,
    Queued(Box<Queued>),
    Held(Box<HeldEvidence>),
}

impl Context {
    fn installed_ancestry(&self, tx: &Transaction<'_>, key: Selector) -> Result<Ancestry> {
        let installed_offer: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM peer_installed_offer_v2
               WHERE origin=?1 AND server=?2 AND offer_id=?3 AND revision=?4)",
            params![self.origin, key.server.to_bytes(), key.id, key.revision],
            |row| row.get(0),
        )?;
        if !installed_offer {
            return Ok(Ancestry::Absent);
        }
        installed::check_parent(tx, &self.codec, &self.signer, &self.route, key)?;
        if terminalized(tx, key, self.origin)? {
            return Ok(Ancestry::Terminal);
        }
        let queued = self.exact_consent(tx, key)?.ok_or(CursorError::Binding)?;
        if owns(tx, key, self.origin)? {
            Ok(Ancestry::Held(Box::new(
                self.read_evidence(tx, queued)?
                    .ok_or(CursorError::Binding)?,
            )))
        } else {
            Ok(Ancestry::Queued(Box::new(queued)))
        }
    }

    fn require_lexical_match(&self, tx: &Transaction<'_>, raw: RawRecord<'_>) -> Result<()> {
        let conflict: bool = tx.query_row(
            "SELECT EXISTS(
                   SELECT 1 FROM peer_pending_custody WHERE delivery_carrier=?1
                       AND (origin!=?2 OR outer_stamp!=?3 OR frame!=?4)
                   UNION ALL
                   SELECT 1 FROM peer_pending_terminal WHERE delivery_carrier=?1 AND step=3
                       AND (origin!=?2 OR outer_stamp!=?3 OR delivery_digest!=?5)
                   UNION ALL
                   SELECT 1 FROM peer_cursor_deferred WHERE id=?1
                       AND (origin!=?2 OR stamp!=?3 OR frame!=?4))",
            params![
                raw.id,
                self.origin,
                seconds(raw.stamp)?,
                raw.bytes,
                <[u8; 32]>::from(Sha256::digest(raw.bytes))
            ],
            |row| row.get(0),
        )?;
        if conflict {
            return Err(Error::ObservedLexicalConflict);
        }
        Ok(())
    }
}

impl Queued {
    fn same_artifact_and_consent(&self, current: &Self) -> bool {
        self.stage() == current.stage()
            && self.prepared.artifact == current.prepared.artifact
            && self.row.transcript == current.row.transcript
            && self.row.digest == current.row.digest
            && self.row.frame == current.row.frame
    }

    fn latest_delivery(&self) -> u64 {
        self.row
            .transcript
            .expires
            .min(self.prepared.artifact.expires)
            .min(self.prepared.offer.flight().expires_at)
    }
}

impl Checkpoint {
    #[cfg(test)]
    pub(in crate::peer_invitation) fn fixture_delivery_state(
        &self,
        source: &Source,
    ) -> Result<[i64; 6]> {
        let cx = self.custody_context(source)?;
        Ok(self.cursor.conn.query_row(
            "SELECT revision, retry_revision, wall,
                (SELECT wall FROM peer_consent_version),
                (SELECT count(*) FROM peer_cursor_deferred),
                (SELECT count(*) FROM peer_pending_custody)
             FROM peer_cursor_origin WHERE origin=?1",
            [cx.origin],
            |row| {
                Ok([
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ])
            },
        )?)
    }

    pub(in crate::peer_invitation) fn installed_delivery_scan(
        &mut self,
        source: &Source,
        account: PublicKey,
        origin: &str,
        wall: u64,
        start: ScanStart,
    ) -> Result<Option<Cursor>> {
        self.check_installed_consent_scope(source, account, origin, wall)?;
        Ok(self.cursor.scan(source.origin, start, wall)?)
    }

    pub(in crate::peer_invitation) fn commit_installed_delivery_page(
        &mut self,
        source: &Source,
        cursor: &Cursor,
        page: CheckedPage<'_>,
        receipts: &[Receipt],
    ) -> Result<Option<Cursor>> {
        let CheckedPage {
            account,
            origin,
            wall,
            bounds: [since, until],
            records,
            ..
        } = page;
        self.check_installed_consent_scope(source, account, origin, wall)?;
        let cx = self.custody_context(source)?;
        let records: Vec<_> = records.iter().map(crate::peer_invitation::record).collect();
        let page = Page {
            origin: cx.origin,
            since,
            until,
            records: &records,
        };
        Ok(self
            .cursor
            .commit_page_with_fence(cursor, &page, receipts, wall, |tx| {
                cx.verify(tx, wall)?;
                rows::advance_wall(tx, wall)
            })?)
    }

    pub(in crate::peer_invitation) fn observe_installed_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
    ) -> Result<Observation> {
        let wall = carrier.wall();
        let raw = carrier.record();
        self.read_installed_consent(
            source,
            carrier.account(),
            carrier.origin(),
            wall,
            |tx, cx| cx.require_lexical_match(tx, raw),
        )?;
        let input = (&carrier).into();
        let classified = self
            .classify_installed_carrier(source, &input)
            .map_err(|error| match error {
                Error::Index => Error::ObservedLexicalConflict,
                other => other,
            })?;
        let incoming = match classified {
            InstalledCarrier::Offer(offer) => {
                return match self.record_installed_offer(source, input, *offer)? {
                    OfferIngress::Recorded(disposition) => Ok(Observation::Receipted(disposition)),
                    OfferIngress::Backpressure => Err(CursorError::Backlog.into()),
                    _ => Err(CursorError::Binding.into()),
                };
            }
            InstalledCarrier::Delivery(incoming) => incoming,
            InstalledCarrier::InvalidOffer | InstalledCarrier::InvalidFrame => {
                return self.receipt_invalid_delivery(source, carrier);
            }
        };
        let selector = Selector::from_delivery(&incoming);
        if matches!(self.custody_mode(selector.server), Err(Error::Policy)) {
            return self.receipt_invalid_delivery(source, carrier);
        }
        let ancestry = self.read_installed_consent(
            source,
            carrier.account(),
            carrier.origin(),
            wall,
            |tx, cx| cx.installed_ancestry(tx, selector),
        )?;
        let queued = match ancestry {
            Ancestry::Absent => return self.receipt_invalid_delivery(source, carrier),
            Ancestry::Terminal => {
                return self
                    .commit_invalid_delivery(source, carrier, |cx, tx| {
                        match cx.installed_ancestry(tx, selector)? {
                            Ancestry::Terminal => Ok(()),
                            _ => Err(CursorError::Stale.into()),
                        }
                    })
                    .map(Observation::Receipted);
            }
            Ancestry::Held(held) => {
                return self.receipt_held_replay(source, carrier, *incoming, &held);
            }
            Ancestry::Queued(queued) => queued,
        };
        let deadline = queued
            .latest_delivery()
            .min(incoming.expires_at())
            .saturating_sub(MARGIN);
        if deadline <= wall {
            return self.receipt_invalid_delivery(source, carrier);
        }
        Ok(Observation::Inspect(Box::new(Prepared {
            witness: carrier.witness(),
            turn: Arc::new(()),
            consent: CustodyConsent {
                owner: self.custody_context(source)?.owner,
                authority: ConsentAuthority::Fresh(queued),
            },
            incoming: *incoming,
            observed_at: wall,
            deadline,
        })))
    }

    pub(in crate::peer_invitation) fn challenge_delivery_package(
        &mut self,
        source: &Source,
        account: PublicKey,
        origin: &str,
        wall: u64,
        challenge: DeliveryChallenge<'_>,
    ) -> Result<ManagedQueued> {
        if wall < challenge.observation || wall >= challenge.deadline {
            return Err(Error::Expired);
        }
        let selector = challenge.queued.prepared.binding.key;
        self.read_managed_queued(source, account, origin, selector, wall)?
            .filter(|current| current.matches_saved(challenge.queued))
            .ok_or(CursorError::Stale.into())
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) fn finish_installed_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        prepared: Prepared,
        native: &NativeAttestation,
    ) -> Result<Disposition> {
        self.finish_delivery(source, carrier, prepared, Native::Fixture(native))
    }

    pub(in crate::peer_invitation) fn finish_managed_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        prepared: Prepared,
        measured: &MeasuredDelivery<'_>,
    ) -> Result<Disposition> {
        measured.require_turn(&prepared.turn)?;
        self.finish_delivery(source, carrier, prepared, Native::Managed(measured))
    }

    fn verify_installed_delivery(
        &self,
        consent: &CustodyConsent,
        queued: &Queued,
        native: &Native<'_, '_>,
        incoming: IncomingDelivery,
        wall: u64,
    ) -> Result<Option<(Delivery, VerifiedDeliveryPackage)>> {
        match native {
            #[cfg(test)]
            Native::Fixture(proof) => self.verify_custody_delivery(consent, proof, incoming, wall),
            Native::Managed(measured) => {
                self.check_local_owner(&consent.owner)?;
                let target = PackageTarget {
                    owner: Arc::clone(&consent.owner),
                    binding: queued.prepared.binding,
                    file: queued.prepared.witness,
                    artifact: queued.prepared.artifact,
                    deadline: queued.latest_delivery(),
                };
                self.verify_exact_delivery(queued, incoming, wall, |_, _| {
                    measured
                        .package()
                        .require(self, &target, wall)
                        .map_err(|_| Error::Package)
                })
            }
        }
    }

    fn finish_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        prepared: Prepared,
        native: Native<'_, '_>,
    ) -> Result<Disposition> {
        let wall = carrier.wall();
        native.check_at(wall)?;
        if !carrier.matches(&prepared.witness) {
            return Err(Error::Source);
        }
        self.check_local_owner(&prepared.consent.owner)?;
        if wall < prepared.observed_at || wall >= prepared.deadline {
            return Err(Error::Expired);
        }
        let key = Selector::from_delivery(&prepared.incoming);
        let ConsentAuthority::Fresh(prior) = &prepared.consent.authority else {
            return Err(Error::Custodied);
        };
        let current = self.read_installed_consent(
            source,
            carrier.account(),
            carrier.origin(),
            wall,
            |tx, cx| {
                let current = cx.current_delivery_consent(tx, key, prior)?;
                native.require_same_turn(&current, prior)?;
                Ok(current)
            },
        )?;
        let Some((delivery, package)) = self.verify_installed_delivery(
            &prepared.consent,
            &current,
            &native,
            prepared.incoming,
            wall,
        )?
        else {
            return self.commit_invalid_queued_delivery(source, carrier, key, &current, &native);
        };
        if current.row.ack_at.is_some() {
            let consent = CustodyConsent {
                owner: prepared.consent.owner,
                authority: ConsentAuthority::Fresh(current),
            };
            let result = self.commit_custody_held_input(
                source,
                HeldInput::InstalledObserved(carrier.record()),
                &consent,
                (&delivery, &package),
                wall,
                || native.check_at(wall),
            );
            let ConsentAuthority::Fresh(current) = &consent.authority else {
                return Err(Error::Custodied);
            };
            return match result {
                Err(Error::Cursor(CursorError::Classification)) => {
                    self.commit_invalid_queued_delivery(source, carrier, key, current, &native)
                }
                Err(Error::Cursor(CursorError::Conflict)) => Err(Error::ObservedLexicalConflict),
                outcome => outcome,
            };
        }
        let expiry = delivery_expiry(&current, &delivery, &package)
            .filter(|&deadline| deadline > wall)
            .ok_or(Error::Expired)?;
        self.defer_installed_delivery(source, carrier, key, &current, expiry, &native)
    }

    fn defer_installed_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        key: Selector,
        prior: &Queued,
        expiry: u64,
        native: &Native<'_, '_>,
    ) -> Result<Disposition> {
        let cx = self.custody_context(source)?;
        let wall = carrier.wall();
        let raw = carrier.record();
        let prerequisite = prior.row.transcript.carrier;
        let metadata = Metadata {
            auth: Auth::FlightVerified,
            expires: expiry.min(wall.checked_add(OUTER_HORIZON).ok_or(CursorError::Range)?),
            prerequisite: Some(prerequisite),
        };
        let deferred = self.cursor.defer_guarded(
            cx.origin,
            raw,
            metadata,
            wall,
            |tx| {
                native.check_at(wall).map_err(schema::cursor_error)?;
                cx.verify(tx, wall)?;
                let current = cx
                    .current_delivery_consent(tx, key, prior)
                    .map_err(schema::cursor_error)?;
                native
                    .require_same_turn(&current, prior)
                    .map_err(schema::cursor_error)?;
                cx.require_no_delivery_competitor(tx, raw, prerequisite)?;
                rows::advance_wall(tx, wall)
            },
            || native.check_at(wall).map_err(schema::cursor_error),
        );
        match deferred {
            Ok(receipt) => Ok(Disposition {
                status: Status::Deferred,
                receipt,
            }),
            Err(CursorError::Classification) => {
                self.commit_invalid_queued_delivery(source, carrier, key, prior, native)
            }
            Err(CursorError::Conflict) => Err(Error::ObservedLexicalConflict),
            Err(error) => Err(error.into()),
        }
    }

    fn receipt_invalid_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
    ) -> Result<Observation> {
        self.commit_invalid_delivery(source, carrier, |_, _| Ok(()))
            .map(Observation::Receipted)
    }

    fn commit_invalid_queued_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        key: Selector,
        prior: &Queued,
        native: &Native<'_, '_>,
    ) -> Result<Disposition> {
        let wall = carrier.wall();
        self.commit_invalid_delivery_guarded(
            source,
            carrier,
            |cx, tx| {
                native.check_at(wall)?;
                let current = cx.current_delivery_consent(tx, key, prior)?;
                native.require_same_turn(&current, prior)
            },
            || native.check_at(wall),
        )
    }

    fn commit_invalid_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        require_ancestry: impl FnOnce(&Context, &Transaction<'_>) -> Result<()>,
    ) -> Result<Disposition> {
        self.commit_invalid_delivery_guarded(source, carrier, require_ancestry, || Ok(()))
    }

    fn commit_invalid_delivery_guarded(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        require_ancestry: impl FnOnce(&Context, &Transaction<'_>) -> Result<()>,
        before_commit: impl FnOnce() -> Result<()>,
    ) -> Result<Disposition> {
        let cx = self.custody_context(source)?;
        let wall = carrier.wall();
        let raw = carrier.record();
        let receipt = self
            .cursor
            .commit_invalid_guarded(
                cx.origin,
                raw,
                wall,
                |tx| {
                    cx.verify(tx, wall)?;
                    match cx.require_lexical_match(tx, raw) {
                        Err(Error::ObservedLexicalConflict) => return Err(CursorError::Conflict),
                        other => other.map_err(schema::cursor_error)?,
                    }
                    require_ancestry(&cx, tx).map_err(schema::cursor_error)?;
                    rows::advance_wall(tx, wall)
                },
                || before_commit().map_err(schema::cursor_error),
            )
            .map_err(|error| match error {
                CursorError::Conflict => Error::ObservedLexicalConflict,
                other => other.into(),
            })?;
        Ok(Disposition {
            status: Status::Invalid,
            receipt,
        })
    }

    fn receipt_held_replay(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        incoming: IncomingDelivery,
        held: &HeldEvidence,
    ) -> Result<Observation> {
        let cx = self.custody_context(source)?;
        let wall = carrier.wall();
        let raw = carrier.record();
        let key = Selector::from(held.offer());
        let exact = raw.id == held.delivery_carrier()
            && raw.stamp == held.outer_stamp()
            && raw.bytes == held.signed_delivery();
        // Check the original Held ancestry even for a competing carrier or
        // an expired replay. Reuse the current classification for exact bytes.
        if exact {
            cx.codec
                .verify_custodied_delivery(&cx.signer, held, incoming, held.held_at())
        } else {
            cx.codec
                .reopen_custodied_delivery(&cx.signer, held, held.held_at())
        }
        .map_err(|_| Error::Package)?;
        let require_current = |cx: &Context, tx: &Transaction<'_>| {
            let Ancestry::Held(current) = cx.installed_ancestry(tx, key)? else {
                return Err(CursorError::Stale.into());
            };
            if current.saved != held.saved || current.row != held.row {
                return Err(CursorError::Stale.into());
            }
            Ok(())
        };
        if !exact || wall >= held.replay_until() {
            return self
                .commit_invalid_delivery(source, carrier, require_current)
                .map(Observation::Receipted);
        }
        let (receipt, outcome) = self
            .cursor
            .commit_trusted_exact(cx.origin, raw, wall, |tx| {
                cx.verify(tx, wall)?;
                require_current(&cx, tx).map_err(schema::cursor_error)?;
                rows::advance_wall(tx, wall)?;
                Ok(TrustedOutcome::AlreadyApplied)
            })?;
        Ok(Observation::Receipted(Disposition {
            status: Status::Accepted(outcome),
            receipt,
        }))
    }
}

impl Context {
    fn current_delivery_consent(
        &self,
        tx: &Transaction<'_>,
        key: Selector,
        prior: &Queued,
    ) -> Result<Box<Queued>> {
        let Ancestry::Queued(current) = self.installed_ancestry(tx, key)? else {
            return Err(CursorError::Stale.into());
        };
        if !prior.same_artifact_and_consent(&current) {
            return Err(CursorError::Stale.into());
        }
        Ok(current)
    }
}

fn delivery_expiry(
    queued: &Queued,
    delivery: &Delivery,
    package: &VerifiedDeliveryPackage,
) -> Option<u64> {
    delivery
        .flight()
        .expires_at
        .min(delivery.flight().replay_safe_until)
        .min(queued.latest_delivery())
        .min(package.bounds().1)
        .checked_sub(MARGIN)
}

#[path = "peer_installed_delivery_retry.rs"]
pub(in crate::peer_invitation) mod retry;
