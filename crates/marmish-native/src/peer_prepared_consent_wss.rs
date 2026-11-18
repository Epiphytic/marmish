//! Private fixture transport for the exact Prepared Consent owner.

use super::*;
use crate::peer_invitation::{
    record,
    socket::owner::LiveRecord,
    store::{
        Page, ScanStart,
        consent_checkpoint::{
            Disposition,
            wire::{ClientCarrier, Codec, Delivery, IncomingDelivery, MAX_OUTER_BYTES},
        },
    },
};
use tokio::time::Instant;

#[path = "peer_pending_join_ingress.rs"]
mod pending_join_ingress;
#[path = "peer_pending_join_owner.rs"]
mod pending_join_owner;

pub(in crate::peer_invitation) use pending_join_owner::{CustodyProgress, JoinCut};

pub(in crate::peer_invitation) enum Received {
    Admitted(Disposition),
    Delivery(Box<VerifiedDelivery>),
}

pub(in crate::peer_invitation) enum History {
    NoWork,
    Committed,
    Held(Box<VerifiedDelivery>),
}

pub(in crate::peer_invitation) struct VerifiedDelivery {
    frame: Box<serde_json::value::RawValue>,
    delivery: Delivery,
}

impl VerifiedDelivery {
    pub(in crate::peer_invitation) fn frame(&self) -> &[u8] {
        self.frame.get().as_bytes()
    }

    pub(in crate::peer_invitation) fn delivery(&self) -> &Delivery {
        &self.delivery
    }
}

impl PreparedConsentOwner<'_> {
    fn require_socket(&self, socket: &Owner) -> Result<()> {
        if !socket.owns_inbox(&self.state.origin_url, self.state.signer.public_key()) {
            return Err(Error::Binding);
        }
        Ok(())
    }

    pub(in crate::peer_invitation) fn available_sample(&mut self) -> Result<Duration> {
        let sample = self.sample()?;
        self.state
            .checkpoint
            .cursor
            .require_available(self.state.origin, sample.as_secs())?;
        Ok(sample)
    }

    pub(in crate::peer_invitation) async fn publish_consent_wss(
        &mut self,
        socket: &mut Owner,
        selector: Selector,
        deadline: Instant,
        after_settlement: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        self.require_socket(socket)?;
        socket.ready_to_publish(deadline)?;
        self.available_sample()?;
        let Some((attempt, native)) = self.reserve_attempt(selector)? else {
            return Ok(false);
        };
        let wall = self.available_sample()?.as_secs();
        self.verify_custody(&native)?;
        self.state
            .checkpoint
            .check_current_attempt(&self.state.source, &attempt, wall)?;
        let carrier = attempt.carrier();
        let receipt = socket
            .publish_bytes(attempt.bytes(), selector.server, carrier, deadline)
            .await?;
        self.require_socket(socket)?;
        self.verify_custody(&native)?;
        if !socket.accepted(&receipt, carrier) {
            return Ok(false);
        }
        let wall = self.available_sample()?.as_secs();
        self.state.checkpoint.settle_prepared_ack(
            &self.state.source,
            MatchingAck(attempt),
            wall,
        )?;
        self.verify_custody(&native)?;
        after_settlement()?;
        if self.custody {
            self.state
                .checkpoint
                .cursor
                .wake(self.state.origin, carrier, wall)?;
        }
        Ok(true)
    }

    pub(in crate::peer_invitation) fn receive_wss(
        &mut self,
        socket: &Owner,
        live: &LiveRecord,
    ) -> Result<Received> {
        self.require_socket(socket)?;
        if !socket.accepts_live(live) {
            return Err(Error::Binding);
        }
        self.receive_arrival(live.arrival())
    }

    pub(in crate::peer_invitation) async fn recover_wss(
        &mut self,
        socket: &mut Owner,
        deadline: Instant,
    ) -> Result<History> {
        self.scan_wss(socket, ScanStart::Recovery, deadline).await
    }

    pub(in crate::peer_invitation) async fn scan_wss(
        &mut self,
        socket: &mut Owner,
        start: ScanStart,
        deadline: Instant,
    ) -> Result<History> {
        self.require_socket(socket)?;
        let wall = self.available_sample()?.as_secs();
        let Some(cursor) = self
            .state
            .checkpoint
            .cursor
            .scan(self.state.origin, start, wall)?
        else {
            return Ok(History::NoWork);
        };
        let page = socket.page([cursor.lower, cursor.until], deadline).await?;
        if !socket.accepts_page(&page) {
            return Err(Error::Binding);
        }
        let mut receipts = Vec::with_capacity(page.records().len());
        for arrival in page.records() {
            match self.receive_arrival(arrival)? {
                Received::Admitted(disposition) => receipts.push(disposition.receipt),
                Received::Delivery(delivery) => return Ok(History::Held(delivery)),
            }
        }
        let wall = self.available_sample()?.as_secs();
        let (_, [since, until]) = page.request();
        let records: Vec<_> = page.records().iter().map(record).collect();
        self.state.checkpoint.cursor.commit_page(
            &cursor,
            &Page {
                origin: self.state.origin,
                since,
                until,
                records: &records,
            },
            &receipts,
            wall,
        )?;
        Ok(History::Committed)
    }

    fn receive_arrival(&mut self, arrival: &Arrival) -> Result<Received> {
        if self.custody {
            return self
                .receive_custody_record(record(arrival))
                .map(Received::Admitted);
        }
        let sample = self.available_sample()?;
        let wall = sample.as_secs();
        let codec = Codec::new(MAX_OUTER_BYTES).map_err(|_| Error::Binding)?;
        match codec.classify_client(&self.state.signer, arrival.body.get().as_bytes()) {
            Ok(ClientCarrier::Offer) => Ok(Received::Admitted(self.state.checkpoint.admit(
                &self.state.source,
                record(arrival),
                wall,
            )?)),
            Ok(ClientCarrier::Delivery(incoming)) => Ok(Received::Delivery(Box::new(
                self.verify_delivery(arrival, *incoming, sample)?,
            ))),
            Err(_) => Ok(Received::Admitted(
                self.state.checkpoint.admit_unclassified(
                    &self.state.source,
                    record(arrival),
                    wall,
                )?,
            )),
        }
    }

    fn verify_delivery(
        &mut self,
        arrival: &Arrival,
        incoming: IncomingDelivery,
        sample: Duration,
    ) -> Result<VerifiedDelivery> {
        if !incoming.matches_index(arrival.id, arrival.stamp) {
            return Err(consent_checkpoint::Error::Index.into());
        }
        let selector = Selector::from_delivery(&incoming);
        let queued = self
            .state
            .checkpoint
            .recover_queued_consent(&self.state.source, selector, sample.as_secs())?
            .ok_or(Error::Binding)?;
        let native = self.attest(queued.stage(), sample)?;
        let wall = self.available_sample()?.as_secs();
        let delivery = self
            .state
            .checkpoint
            .verify_queued_delivery(&queued, &native, incoming, wall)?;
        self.verify_custody(&native)?;
        Ok(VerifiedDelivery {
            frame: arrival.body.clone(),
            delivery,
        })
    }
}
