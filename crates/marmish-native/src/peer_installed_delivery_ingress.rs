//! Classification and transactions for checked installed recipient Delivery.

use super::super::super::super::installed;
use super::offer_ingress::{InstalledCarrier, OfferIngress};
use super::*;
use crate::peer_invitation::socket::owner::recipient::{CarrierWitness, CheckedCarrier, CheckedPage};
use crate::peer_invitation::store::{Auth, Metadata, Page, Receipt, ScanStart, pages::Cursor};

const OUTER_HORIZON: u64 = 90 * 3600;

pub(in crate::peer_invitation) enum Observation {
    Receipted(Disposition),
    Inspect(Box<Prepared>),
}

pub(in crate::peer_invitation) struct Prepared {
    witness: CarrierWitness,
    consent: CustodyConsent,
    incoming: IncomingDelivery,
    observed_at: u64,
}

impl Prepared {
    pub(in crate::peer_invitation) fn native_coordinates(&self) -> ((Binding, FileWitness), u64) {
        (self.consent.stage(), self.observed_at)
    }
}

enum Ancestry {
    Absent,
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
            return Err(Error::Custodied);
        }
        let queued = self.exact_consent(tx, key)?.ok_or(CursorError::Binding)?;
        if owns(tx, key, self.origin)? {
            Ok(Ancestry::Held(Box::new(
                self.read_evidence(tx, queued)?.ok_or(CursorError::Binding)?,
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
            return Err(Error::Index);
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
        let CheckedPage { account, origin, wall, bounds: [since, until], records, .. } = page;
        self.check_installed_consent_scope(source, account, origin, wall)?;
        let cx = self.custody_context(source)?;
        let records: Vec<_> = records.iter().map(crate::peer_invitation::record).collect();
        let page = Page { origin: cx.origin, since, until, records: &records };
        Ok(self.cursor.commit_page_with_fence(cursor, &page, receipts, wall, |tx| {
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
        self.read_installed_consent(source, carrier.account(), carrier.origin(), wall, |tx, cx| {
            cx.require_lexical_match(tx, raw)
        })?;
        let input = (&carrier).into();
        let incoming = match self.classify_installed_carrier(source, &input)? {
            InstalledCarrier::Offer(offer) => {
                return match self.record_installed_offer(source, input, offer)? {
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
            Ancestry::Held(held) => {
                return self.receipt_held_replay(source, carrier, *incoming, &held);
            }
            Ancestry::Queued(queued) => queued,
        };
        if stale(queued.latest_delivery(), incoming.expires_at(), wall) {
            return self.receipt_invalid_delivery(source, carrier);
        }
        Ok(Observation::Inspect(Box::new(Prepared {
            witness: carrier.witness(),
            consent: CustodyConsent {
                owner: self.custody_context(source)?.owner,
                authority: ConsentAuthority::Fresh(queued),
            },
            incoming: *incoming,
            observed_at: wall,
        })))
    }

    pub(in crate::peer_invitation) fn finish_installed_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        prepared: Prepared,
        native: &NativeAttestation,
    ) -> Result<Disposition> {
        if !carrier.matches(&prepared.witness) {
            return Err(Error::Source);
        }
        self.check_local_owner(&prepared.consent.owner)?;
        let wall = carrier.wall();
        let key = Selector::from_delivery(&prepared.incoming);
        let ConsentAuthority::Fresh(prior) = &prepared.consent.authority else {
            return Err(Error::Custodied);
        };
        let current = self.read_installed_consent(
            source,
            carrier.account(),
            carrier.origin(),
            wall,
            |tx, cx| cx.current_delivery_consent(tx, key, prior),
        )?;
        if stale(current.latest_delivery(), prepared.incoming.expires_at(), wall) {
            return Err(Error::Expired);
        }
        let Some((delivery, package)) = self.verify_custody_delivery(
            &prepared.consent,
            native,
            prepared.incoming,
            wall,
        )? else {
            return self.commit_invalid_queued_delivery(source, carrier, key, &current);
        };
        if current.row.ack_at.is_some() {
            let consent = CustodyConsent {
                owner: prepared.consent.owner,
                authority: ConsentAuthority::Fresh(current),
            };
            return self
                .commit_custody_held(source, carrier.record(), &consent, &delivery, &package, wall)
                ;
        }
        let expiry = delivery.flight().expires_at
            .min(delivery.flight().replay_safe_until)
            .min(current.latest_delivery())
            .min(package.bounds().1)
            .checked_sub(MARGIN)
            .filter(|&deadline| deadline > wall)
            .ok_or(Error::Expired)?;
        self.defer_installed_delivery(source, carrier, key, &current, expiry)
    }

    fn defer_installed_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        key: Selector,
        prior: &Queued,
        expiry: u64,
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
        let deferred = self.cursor.defer_with_fence(cx.origin, raw, metadata, wall, |tx| {
            cx.verify(tx, wall)?;
            cx.current_delivery_consent(tx, key, prior).map_err(schema::cursor_error)?;
            let competitor: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM peer_cursor_deferred
                  WHERE origin=?1 AND auth=?2 AND prerequisite=?3 AND id!=?4)",
                params![cx.origin, Auth::FlightVerified as i64, prerequisite, raw.id],
                |row| row.get(0),
            )?;
            if competitor {
                return Err(CursorError::Classification);
            }
            rows::advance_wall(tx, wall)
        });
        match deferred {
            Ok(receipt) => Ok(Disposition {
                status: Status::Deferred,
                receipt,
            }),
            Err(CursorError::Classification) => self.commit_invalid_queued_delivery(source, carrier, key, prior),
            Err(error) => Err(error.into()),
        }
    }

    fn receipt_invalid_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
    ) -> Result<Observation> {
        self.commit_invalid_delivery(source, carrier, |_, _| Ok(())).map(Observation::Receipted)
    }

    fn commit_invalid_queued_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        key: Selector,
        prior: &Queued,
    ) -> Result<Disposition> {
        self.commit_invalid_delivery(source, carrier, |cx, tx| {
            cx.current_delivery_consent(tx, key, prior).map(|_| ())
        })
    }

    fn commit_invalid_delivery(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
        require_ancestry: impl FnOnce(&Context, &Transaction<'_>) -> Result<()>,
    ) -> Result<Disposition> {
        let cx = self.custody_context(source)?;
        let wall = carrier.wall();
        let raw = carrier.record();
        let receipt = self.cursor.commit_invalid_with_fence(cx.origin, raw, wall, |tx| {
            cx.verify(tx, wall)?;
            match cx.require_lexical_match(tx, raw) {
                Err(Error::Index) => return Err(CursorError::Conflict),
                other => other.map_err(schema::cursor_error)?,
            }
            require_ancestry(&cx, tx).map_err(schema::cursor_error)?;
            rows::advance_wall(tx, wall)
        }).map_err(|error| match error {
            CursorError::Conflict => Error::Index,
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
            cx.codec.verify_custodied_delivery(&cx.signer, held, incoming, held.held_at())
        } else {
            cx.codec.reopen_custodied_delivery(&cx.signer, held, held.held_at())
        }.map_err(|_| Error::Package)?;
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
            return self.commit_invalid_delivery(source, carrier, require_current)
                .map(Observation::Receipted);
        }
        let (receipt, outcome) = self.cursor.commit_trusted_exact(cx.origin, raw, wall, |tx| {
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

fn stale(ancestry_upper: u64, delivery_upper: u64, wall: u64) -> bool {
    ancestry_upper.min(delivery_upper).saturating_sub(MARGIN) <= wall
}

#[path = "peer_installed_delivery_retry.rs"]
pub(in crate::peer_invitation) mod retry;
