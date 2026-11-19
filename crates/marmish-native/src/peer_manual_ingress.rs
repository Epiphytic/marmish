//! Exact-port, meter-backed observation and one durable recovery scan.

#![cfg_attr(
    not(marmish_peer_receiver_candidate),
    allow(
        dead_code,
        reason = "the installed Manual actor uses the candidate target"
    )
)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::{
    dispatcher_fatal::Fatal,
    peer_candidate_startup::Readiness,
    peer_invitation::{
        socket::{
            owner::{
                LiveRecord, Page,
                recipient::{CheckedCarrier, Incarnation, PeerRecipientPort},
            },
            wire::RECORD_LIMIT,
        },
        store::{
            Origin,
            consent_checkpoint::{
                Disposition, provisioning::prepared_consent::custody::delivery_ingress::Observation,
            },
            pages::Cursor,
        },
    },
    peer_relay_trust::{NativeRouteWindow, candidate_registration::ForwardingVerifier},
};

use super::*;

const PAGE_WAIT: Duration = Duration::from_secs(5);
const LIVE_WAIT: Duration = Duration::from_millis(500);

pub(in crate::peer_invitation) struct Ingress {
    owner: Arc<()>,
    guard: Guard,
    recovery: Recovery,
}

enum Recovery {
    Active(Cursor),
    Complete {
        _origin: Origin,
        _anchor: u64,
        _revision: u64,
    },
    Failed,
}

struct Guard {
    incarnation: Incarnation,
    readiness: Readiness,
    stop: Arc<AtomicBool>,
    forwarding: ForwardingVerifier,
}

impl Guard {
    fn check(&self) -> consent_checkpoint::Result<()> {
        // Keep checking the independent process-wide invariant on local loss.
        self.forwarding
            .recheck_default()
            .map_err(|_| consent_checkpoint::Error::Source)?;
        if !self.incarnation.is_live()
            || !self.readiness.is_current()
            || self.stop.load(Ordering::Acquire)
        {
            return Err(consent_checkpoint::Error::Source);
        }
        Ok(())
    }

    fn window(&self, port: &mut PeerRecipientPort) -> Result<NativeWindow> {
        self.check()?;
        if !port.matches_incarnation(&self.incarnation) {
            return Err(Error::Binding);
        }
        port.use_current_native(|route| {
            require_fatal(&route, &self.forwarding.fatal())?;
            route
                .window(self.readiness.clone(), self.stop.clone(), u64::MAX)
                .map_err(|_| Error::Clock)
        })?
    }

    fn admit(&self, window: &NativeWindow) -> consent_checkpoint::Result<()> {
        self.check()?;
        window
            .sample()
            .map_err(|_| consent_checkpoint::Error::Expired)?;
        Ok(())
    }

    fn before_io(&self, port: &mut PeerRecipientPort, deadline: Instant) -> Result<Instant> {
        self.check()?;
        if !port.matches_incarnation(&self.incarnation) {
            return Err(Error::Binding);
        }
        Ok(port.use_current_native(|route| deadline.min(route.horizon()))?)
    }
}

impl Ingress {
    pub(in crate::peer_invitation) fn recover(
        owner: &mut RecoveredOwner<'_>,
        port: &mut PeerRecipientPort,
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        forwarding: ForwardingVerifier,
    ) -> Result<Self> {
        forwarding
            .recheck_default()
            .map_err(|_| consent_checkpoint::Error::Source)?;
        let guard = Guard {
            incarnation: port.claim_recovery()?,
            readiness,
            stop,
            forwarding,
        };
        let window = guard.window(port)?;
        owner.cursor.unchanged()?;
        let cursor = port.use_current_recipient(|account, origin, wall| {
            owner.checkpoint.installed_delivery_scan_guarded(
                &owner.source,
                account,
                origin,
                wall,
                ScanStart::Recovery,
                || guard.admit(&window),
            )
        })??;
        guard.admit(&window)?;
        owner.cursor.unchanged()?;
        Ok(Self {
            owner: owner.ingress_owner.clone(),
            guard,
            recovery: Recovery::Active(cursor.ok_or(Error::Binding)?),
        })
    }

    fn check(&self, owner: &RecoveredOwner<'_>, port: &PeerRecipientPort) -> Result<()> {
        self.guard.check()?;
        if !Arc::ptr_eq(&self.owner, &owner.ingress_owner)
            || !port.matches_incarnation(&self.guard.incarnation)
        {
            return Err(Error::Binding);
        }
        owner.cursor.unchanged()?;
        Ok(())
    }

    pub(in crate::peer_invitation) async fn history_turn(
        &mut self,
        owner: &mut RecoveredOwner<'_>,
        port: &mut PeerRecipientPort,
        deadline: Option<Instant>,
    ) -> Result<bool> {
        let current = std::mem::replace(&mut self.recovery, Recovery::Failed);
        self.check(owner, port)?;
        let Recovery::Active(cursor) = current else {
            return Err(Error::Binding);
        };
        self.drain_buffered(owner, port)?;
        let page_limit = Instant::now() + PAGE_WAIT;
        let deadline = self.guard.before_io(
            port,
            deadline.map_or(page_limit, |deadline| deadline.min(page_limit)),
        )?;
        let result = port.page([cursor.lower, cursor.until], deadline).await;
        self.guard.check()?;
        let page = result?;
        let next = self.commit_page(owner, port, &cursor, &page)?;
        self.check(owner, port)?;
        let complete = next.is_none();
        self.recovery = match next {
            Some(next) => Recovery::Active(next),
            None => Recovery::Complete {
                _origin: cursor.origin,
                _anchor: cursor.anchor,
                _revision: cursor.revision.checked_add(1).ok_or(Error::Binding)?,
            },
        };
        Ok(complete)
    }

    pub(in crate::peer_invitation) async fn live_turn(
        &mut self,
        owner: &mut RecoveredOwner<'_>,
        port: &mut PeerRecipientPort,
        deadline: Instant,
        before_record: impl FnOnce(),
    ) -> Result<Option<Status>> {
        let current = std::mem::replace(&mut self.recovery, Recovery::Failed);
        if matches!(current, Recovery::Failed) {
            return Err(Error::Binding);
        }
        self.check(owner, port)?;
        let live = match port.take_buffered_live()? {
            Some(buffered) => Some(buffered),
            None => {
                let deadline = self
                    .guard
                    .before_io(port, deadline.min(Instant::now() + LIVE_WAIT))?;
                port.wait_live(deadline).await?
            }
        };
        self.check(owner, port)?;
        let outcome = live
            .as_ref()
            .map(|live| {
                before_record();
                self.ingest(owner, port, Input::Live(live))
            })
            .transpose()?;
        self.check(owner, port)?;
        // Even an idle timeout must still be in the physical attestation window.
        self.guard.window(port)?;
        self.recovery = current;
        Ok(outcome.map(|effect| effect.status))
    }

    fn drain_buffered(
        &self,
        owner: &mut RecoveredOwner<'_>,
        port: &mut PeerRecipientPort,
    ) -> Result<()> {
        for _ in 0..RECORD_LIMIT {
            self.check(owner, port)?;
            let Some(live) = port.take_buffered_live()? else {
                break;
            };
            self.ingest(owner, port, Input::Live(&live))?;
        }
        Ok(())
    }

    fn commit_page(
        &self,
        owner: &mut RecoveredOwner<'_>,
        port: &mut PeerRecipientPort,
        cursor: &Cursor,
        page: &Page,
    ) -> Result<Option<Cursor>> {
        // The raw owner already rejected duplicate/conflicting wire carriers
        // and requires exact request attribution and terminal EOSE.
        let count = port.use_page(page, |checked| checked.records.len())?;
        let mut receipts = Vec::with_capacity(count);
        for index in 0..count {
            receipts.push(
                self.ingest(owner, port, Input::History(page, index))?
                    .receipt,
            );
        }
        let window = self.guard.window(port)?;
        self.check(owner, port)?;
        let next = port.use_page(page, |checked| {
            owner.checkpoint.commit_installed_delivery_page_guarded(
                &owner.source,
                cursor,
                checked,
                &receipts,
                || self.guard.admit(&window),
            )
        })??;
        self.guard.admit(&window)?;
        Ok(next)
    }

    fn ingest(
        &self,
        owner: &mut RecoveredOwner<'_>,
        port: &mut PeerRecipientPort,
        input: Input<'_>,
    ) -> Result<Disposition> {
        self.check(owner, port)?;
        let window = self.guard.window(port)?;
        let admit = || self.guard.admit(&window);
        let observed = input.observe(port, |checked| {
            owner.observe_managed_delivery_guarded(checked, admit)
        })??;
        admit()?;
        let prepared = match observed {
            Observation::Inspect(prepared) => prepared,
            Observation::Receipted(disposition) => return Ok(disposition),
        };
        let quota = owner.package_quota.ok_or(Error::Binding)?;
        let measured = port.use_delivery_native(prepared.challenge()?, |checked| {
            require_fatal(checked.route(), &self.guard.forwarding.fatal())?;
            admit()?;
            owner.measure_delivery_guarded(
                quota,
                checked,
                self.guard.readiness.clone(),
                self.guard.stop.clone(),
                &admit,
            )
        })??;
        admit()?;
        let effect = input.observe(port, |checked| {
            owner.finish_measured_delivery_guarded(quota, checked, *prepared, measured, &admit)
        })??;
        self.check(owner, port)?;
        admit()?;
        Ok(effect)
    }
}

pub(in crate::peer_invitation) fn require_fatal(
    route: &NativeRouteWindow<'_>,
    fatal: &Fatal,
) -> Result<()> {
    if !route.matches_fatal(fatal) {
        return Err(Error::Binding);
    }
    Ok(())
}

enum Input<'a> {
    Live(&'a LiveRecord),
    History(&'a Page, usize),
}

impl Input<'_> {
    fn observe<T>(
        &self,
        port: &mut PeerRecipientPort,
        use_checked: impl FnOnce(CheckedCarrier<'_>) -> T,
    ) -> std::result::Result<T, Failure> {
        match *self {
            Self::Live(live) => port.use_live_carrier(live, use_checked),
            Self::History(page, index) => port.use_page_carrier(page, index, use_checked),
        }
    }
}
