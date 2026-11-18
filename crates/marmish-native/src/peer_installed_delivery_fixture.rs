//! Compose the checked physical and staged package turns under one owner borrow.

use super::*;
use crate::peer_invitation::{
    socket::owner::{
        LiveRecord, Page,
        recipient::{CheckedCarrier, PeerRecipientPort},
    },
    store::{
        ScanStart,
        consent_checkpoint::provisioning::prepared_consent::custody::delivery_ingress::Observation,
        pages::Cursor,
    },
};
use consent_checkpoint::Disposition;

#[path = "peer_installed_delivery_service.rs"]
mod service;

#[path = "peer_installed_delivery_wss_tests.rs"]
mod tests;

#[derive(Clone, Copy)]
enum Input<'a> {
    Live(&'a LiveRecord),
    History(&'a Page, usize),
}

impl Input<'_> {
    fn observe<T>(
        self,
        port: &mut PeerRecipientPort,
        use_checked: impl FnOnce(CheckedCarrier<'_>) -> T,
    ) -> std::result::Result<T, Failure> {
        match self {
            Self::Live(live) => port.use_live_carrier(live, use_checked),
            Self::History(page, index) => port.use_page_carrier(page, index, use_checked),
        }
    }
}

impl Fixture<'_> {
    fn scan_delivery(
        &mut self,
        port: &mut PeerRecipientPort,
        start: ScanStart,
    ) -> Result<Option<Cursor>> {
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        port.use_current_recipient(|account, origin, wall| {
            state
                .checkpoint
                .installed_delivery_scan(&state.source, account, origin, wall, start)
        })?
        .map_err(Into::into)
    }

    fn delivery_input(
        &mut self,
        port: &mut PeerRecipientPort,
        input: Input<'_>,
    ) -> Result<Disposition> {
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        let observation = input.observe(port, |carrier| {
            state
                .checkpoint
                .observe_installed_delivery(&state.source, carrier)
        })??;
        let prepared = match observation {
            Observation::Inspect(prepared) => prepared,
            Observation::Receipted(disposition) => return Ok(disposition),
        };

        let (stage, wall) = prepared.native_coordinates();
        let native = self.attest(stage, wall)?;
        input.observe(port, |carrier| {
            self.cursor.unchanged()?;
            self.owner.verify_custody(&native)?;
            let state = &mut self.owner.state;
            Ok(state.checkpoint.finish_installed_delivery(
                &state.source,
                carrier,
                *prepared,
                &native,
            )?)
        })?
    }

    fn retry_delivery(
        &mut self,
        port: &mut PeerRecipientPort,
    ) -> Result<Option<consent_checkpoint::Status>> {
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        let Some(saved) = port.use_current_recipient(|account, origin, wall| {
            state
                .checkpoint
                .due_installed_delivery(&state.source, account, origin, wall)
        })??
        else {
            return Ok(None);
        };
        let Some(prepared) = port.use_saved_delivery(&saved, |checked| {
            state
                .checkpoint
                .observe_saved_delivery(&state.source, checked)
        })??
        else {
            return Ok(None);
        };

        let (stage, wall) = prepared.native_coordinates();
        let native = self.attest(stage, wall)?;
        port.use_saved_delivery(&saved, |checked| {
            self.cursor.unchanged()?;
            self.owner.verify_custody(&native)?;
            let state = &mut self.owner.state;
            Ok(state
                .checkpoint
                .commit_saved_delivery(&state.source, checked, prepared, &native)?)
        })?
    }

    fn delivery_page(
        &mut self,
        port: &mut PeerRecipientPort,
        cursor: &Cursor,
        page: &Page,
    ) -> Result<(Vec<consent_checkpoint::Status>, Option<Cursor>)> {
        let count = port.use_page(page, |checked| {
            for (index, entry) in checked.records.iter().enumerate() {
                if checked.records[..index].iter().any(|earlier| {
                    earlier.id == entry.id
                        && (earlier.stamp != entry.stamp || earlier.body.get() != entry.body.get())
                }) {
                    return Err(consent_checkpoint::Error::ObservedPageConflict);
                }
            }
            Ok(checked.records.len())
        })??;
        let mut statuses = Vec::with_capacity(count);
        let mut receipts = Vec::with_capacity(count);
        for index in 0..count {
            let disposition = self.delivery_input(port, Input::History(page, index))?;
            statuses.push(disposition.status);
            receipts.push(disposition.receipt);
        }
        self.cursor.unchanged()?;
        let state = &mut self.owner.state;
        let next = port.use_page(page, |checked| {
            state.checkpoint.commit_installed_delivery_page(
                &state.source,
                cursor,
                checked,
                &receipts,
            )
        })??;
        Ok((statuses, next))
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "peer_installed_pending_wss_tests.rs"]
mod pending_bridge;
