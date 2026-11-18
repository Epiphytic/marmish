//! One checked Offer admitted to the installed recipient custody successor.

use super::super::super::super::installed;
use super::*;
use crate::peer_invitation::socket::owner::recipient::CheckedCarrier;

pub(in crate::peer_invitation) enum OfferIngress {
    Recorded(Disposition),
    Invalid,
    NotOffer,
    Backpressure,
}

impl Checkpoint {
    pub(in crate::peer_invitation) fn admit_installed_custody_offer(
        &mut self,
        source: &Source,
        carrier: CheckedCarrier<'_>,
    ) -> Result<OfferIngress> {
        if self.layout != Layout::InstalledCustodyV2 {
            return Err(Error::Layout);
        }
        if !matches!(self.installed_receive, Some(ReceivePolicy::Manual)) {
            return Err(Error::Policy);
        }
        let cx = self.custody_context(source)?;
        if carrier.account().to_bytes() != cx.account || carrier.origin() != cx.route.urls[0] {
            return Err(Error::Source);
        }
        let wall = carrier.wall();
        let record = carrier.record();
        record.check()?;
        {
            let tx = self.cursor.begin(cx.origin)?;
            cx.verify(&tx, wall)?;
            let floor = tx.query_row(
                "SELECT wall FROM peer_cursor_origin WHERE origin=?1",
                [cx.origin],
                |row| read_seconds(row, 0),
            )?;
            if wall < floor {
                return Err(CursorError::ClockRegressed.into());
            }
        }

        let Ok(outer) = cx.codec.verify_client_outer(record.bytes) else {
            return Ok(OfferIngress::NotOffer);
        };
        if !outer.matches_index(record.id, record.stamp) {
            return Err(Error::Index);
        }
        if !matches!(
            cx.codec.classify_verified_client(&cx.signer, outer),
            Ok(ClientCarrier::Offer)
        ) {
            return Ok(OfferIngress::NotOffer);
        }
        let offer = match cx
            .codec
            .open_offer(&cx.signer, record.bytes, None, &cx.route.urls, wall)
        {
            Ok(offer) => offer,
            Err(wire::Error::Route | wire::Error::Time | wire::Error::Binding) => {
                return Ok(OfferIngress::Invalid);
            }
            Err(_) => return Ok(OfferIngress::NotOffer),
        };
        let flight = offer.flight();
        if flight.carrier_id.to_bytes() != record.id {
            return Err(Error::Index);
        }
        if wall >= flight.expires_at
            || flight.author == flight.recipient
            || self.policy.blocked.contains(&flight.author)
        {
            return Ok(OfferIngress::Invalid);
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
