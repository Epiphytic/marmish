//! One checked Offer admitted to the installed recipient custody successor.

use super::super::super::super::installed;
use super::*;
use crate::peer_invitation::socket::owner::recipient::CheckedCarrier;
use wire::ClientCarrier;

pub(in crate::peer_invitation) enum OfferIngress {
    Recorded(Disposition),
    Invalid,
    NotOffer,
    Backpressure,
}

pub(in crate::peer_invitation) enum InstalledCarrier {
    Offer(Box<wire::Offer>),
    Delivery(Box<IncomingDelivery>),
    InvalidOffer,
    InvalidFrame,
}

pub(in crate::peer_invitation) struct OfferInput<'a> {
    pub(super) account: PublicKey,
    pub(super) origin: &'a str,
    pub(super) wall: u64,
    pub(super) record: RawRecord<'a>,
}

#[cfg(test)]
impl<'a> OfferInput<'a> {
    pub(in crate::peer_invitation) fn fixture(
        account: PublicKey,
        origin: &'a str,
        wall: u64,
        record: RawRecord<'a>,
    ) -> Self {
        Self {
            account,
            origin,
            wall,
            record,
        }
    }
    pub(in crate::peer_invitation) fn corrupt_id_fixture(&mut self) {
        self.record.id[0] ^= 1;
    }
}

impl<'a> From<&CheckedCarrier<'a>> for OfferInput<'a> {
    fn from(carrier: &CheckedCarrier<'a>) -> Self {
        Self {
            account: carrier.account(),
            origin: carrier.origin(),
            wall: carrier.wall(),
            record: carrier.record(),
        }
    }
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn admit_installed_custody_offer(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
    ) -> Result<OfferIngress> {
        self.admit_checked_offer(source, (&carrier).into())
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) fn admit_offer_fixture(
        &mut self,
        source: &Source,
        input: OfferInput<'_>,
    ) -> Result<OfferIngress> {
        self.admit_checked_offer(source, input)
    }

    fn admit_checked_offer(
        &mut self,
        source: &Source,
        input: OfferInput<'_>,
    ) -> Result<OfferIngress> {
        match self.classify_installed_carrier(source, &input)? {
            InstalledCarrier::Offer(offer) => self.record_installed_offer(source, input, *offer),
            InstalledCarrier::InvalidOffer => Ok(OfferIngress::Invalid),
            InstalledCarrier::InvalidFrame | InstalledCarrier::Delivery(_) => {
                Ok(OfferIngress::NotOffer)
            }
        }
    }

    pub(super) fn classify_installed_carrier(
        &mut self,
        source: &Source,
        input: &OfferInput<'_>,
    ) -> Result<InstalledCarrier> {
        let wall = input.wall;
        let record = input.record;
        record.check()?;
        self.check_installed_consent_scope(source, input.account, input.origin, wall)?;

        let cx = self.custody_context(source)?;
        let Ok(outer) = cx.codec.verify_client_outer(record.bytes) else {
            return Ok(InstalledCarrier::InvalidFrame);
        };
        if !outer.matches_index(record.id, record.stamp) {
            return Err(Error::Index);
        }
        let offer = match cx.codec.classify_verified_client(&cx.signer, outer) {
            Ok(ClientCarrier::Delivery(incoming)) => {
                return Ok(InstalledCarrier::Delivery(incoming));
            }
            Ok(ClientCarrier::Offer(opened)) => {
                match opened.qualify(&cx.codec, &cx.route.urls, wall) {
                    Ok(offer) => offer,
                    Err(wire::Error::Route | wire::Error::Time | wire::Error::Binding) => {
                        return Ok(InstalledCarrier::InvalidOffer);
                    }
                    Err(_) => return Ok(InstalledCarrier::InvalidFrame),
                }
            }
            Err(_) => return Ok(InstalledCarrier::InvalidFrame),
        };
        let flight = offer.flight();
        if wall >= flight.expires_at
            || flight.author == flight.recipient
            || self.policy.blocked.contains(&flight.author)
        {
            return Ok(InstalledCarrier::InvalidOffer);
        }
        Ok(InstalledCarrier::Offer(Box::new(offer)))
    }

    pub(super) fn record_installed_offer(
        &mut self,
        source: &Source,
        input: OfferInput<'_>,
        offer: wire::Offer,
    ) -> Result<OfferIngress> {
        let cx = self.custody_context(source)?;
        let wall = input.wall;
        let record = input.record;
        let flight = offer.flight();
        if flight.carrier_id.to_bytes() != record.id {
            return Err(Error::Index);
        }

        let committed = self
            .cursor
            .commit_trusted_exact(cx.origin, record, wall, |tx| {
                cx.verify(tx, wall)?;
                if super::super::cooldown::under_cooldown(tx, flight.author.to_bytes(), wall)? {
                    return Ok(TrustedOutcome::SkippedCooldown);
                }
                installed::sweep(tx, wall)?;
                let outcome = rows::admit(tx, cx.origin, record, &offer, false, None)?;
                if outcome == TrustedOutcome::Applied {
                    installed::mark(tx, cx.origin, &offer)?;
                }
                cx.verify(tx, wall)?;
                installed::check_parent(
                    tx,
                    &cx.codec,
                    &cx.signer,
                    &cx.route,
                    Selector::from(&offer),
                )?;
                Ok(outcome)
            });
        match committed {
            Ok((receipt, outcome)) => Ok(OfferIngress::Recorded(Disposition {
                status: Status::Accepted(outcome),
                receipt,
            })),
            Err(CursorError::Backlog) => Ok(OfferIngress::Backpressure),
            Err(error) => Err(error.into()),
        }
    }
}

#[cfg(test)]
pub(in crate::peer_invitation) mod fault_fixture {
    use super::*;

    pub(in crate::peer_invitation) fn limit_to_current_pages(checkpoint: &Checkpoint) {
        let conn = &checkpoint.cursor.conn;
        let pages: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap();
        let cap: i64 = conn
            .query_row(&format!("PRAGMA max_page_count={pages}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(pages, cap);
    }

    pub(in crate::peer_invitation) fn apply_cooldown(
        checkpoint: &mut Checkpoint,
        founder: PublicKey,
        wall: u64,
    ) {
        let [origin] = checkpoint.cursor.origins.as_slice() else {
            panic!("exactly one installed route");
        };
        let tx = checkpoint.cursor.begin(*origin).unwrap();
        super::super::super::cooldown::decline(&tx, founder.to_bytes(), wall).unwrap();
        tx.commit().unwrap();
    }
}
