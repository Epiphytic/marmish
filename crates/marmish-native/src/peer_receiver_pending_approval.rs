//! Unmounted decision leaf for the exact local exchange and charged native stage.

use super::*;
use crate::ipc::{
    peer_approve::{Failure as ReplyFailure, Selection},
    peer_approve_listener::{Claimed, Consumed},
};
use consent_checkpoint::provisioning::prepared_consent::custody::{
    ApprovalFailure, ApprovalGuard, ApprovalScope,
};
use socket::wire::Failure as PortFailure;

type Decision = std::result::Result<(), ApprovalFailure>;
type PortDecision = std::result::Result<Decision, PortFailure>;

#[must_use]
pub(in crate::peer_invitation) struct ApprovalSettlement {
    result: PortDecision,
    exchange: Exchange,
}

#[allow(
    dead_code,
    reason = "the candidate approval actor is mounted separately"
)]
impl ApprovalSettlement {
    pub(in crate::peer_invitation) fn result(&self) -> PortDecision {
        self.result
    }

    // Caller independently verifies the owning service and current port before
    // releasing the reply. The socket token checks its own lifetime again.
    pub(in crate::peer_invitation) fn respond(self) {
        let reply = match self.result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(ApprovalFailure::Stale)) => Err(ReplyFailure::Stale),
            Ok(Err(ApprovalFailure::Expired)) => Err(ReplyFailure::Expired),
            Ok(Err(ApprovalFailure::Ambiguous)) => Err(ReplyFailure::Uncertain),
            _ => Err(ReplyFailure::Unavailable),
        };
        match self.exchange {
            Exchange::Claimed(request) => {
                request.refuse(reply.err().unwrap_or(ReplyFailure::Uncertain))
            }
            Exchange::Consumed(request) => request.complete(reply),
            Exchange::Revoked => {}
        }
    }
}

pub(in crate::peer_invitation) struct DecisionGuard<'a>(
    &'a mut dyn FnMut(ApprovalGuard) -> std::result::Result<u64, ApprovalFailure>,
);

impl DecisionGuard<'_> {
    pub(in crate::peer_invitation) fn check(
        &mut self,
        guard: ApprovalGuard,
    ) -> std::result::Result<u64, ApprovalFailure> {
        (self.0)(guard)
    }
}

enum Exchange {
    Claimed(Claimed),
    Consumed(Consumed),
    Revoked,
}

impl Exchange {
    fn check(
        &mut self,
        lease: &Arc<ProfileLease>,
        selection: Selection,
        guard: ApprovalGuard,
    ) -> Decision {
        match guard {
            ApprovalGuard::Consume => {
                let Self::Claimed(request) = std::mem::replace(self, Self::Revoked) else {
                    return Err(ApprovalFailure::Unavailable);
                };
                let Some(request) = request.consume(lease, selection) else {
                    return Err(ApprovalFailure::Unavailable);
                };
                *self = Self::Consumed(request);
                Ok(())
            }
            ApprovalGuard::Verify => match self {
                Self::Consumed(request)
                    if request.matches_lease(lease)
                        && request.selection() == selection
                        && request.check_live() =>
                {
                    Ok(())
                }
                _ => Err(ApprovalFailure::Unavailable),
            },
        }
    }
}

#[allow(
    dead_code,
    reason = "the candidate approval actor is mounted separately"
)]
impl PendingTurn<'_, '_> {
    pub(in crate::peer_invitation) fn approve(
        &mut self,
        lease: &Arc<ProfileLease>,
        port: &mut PeerRecipientPort,
        request: Claimed,
        clock: PendingClock<'_>,
    ) -> ApprovalSettlement {
        let selection = request.selection();
        let cutoff = request.deadline();
        let mut exchange = Exchange::Claimed(request);
        let result = port.use_current_native(|scope| {
            let Exchange::Claimed(request) = &exchange else {
                return Err(ApprovalFailure::Unavailable);
            };
            if !request.matches_lease(lease)
                || !request.check_live()
                || !ptr::eq(lease.as_ref(), self.native.lease)
                || scope.account() != self.native.signer.public_key()
                || scope.origin() != self.native.origin
            {
                return Err(ApprovalFailure::Unavailable);
            }
            let PendingClock::Candidate {
                readiness,
                stop,
                forwarding: Some(forwarding),
            } = clock
            else {
                return Err(ApprovalFailure::Unavailable);
            };
            let window = scope
                .manual_window_before(
                    readiness.clone(),
                    stop.clone(),
                    u64::MAX,
                    forwarding,
                    cutoff,
                )
                .map_err(|_| ApprovalFailure::Unavailable)?;
            let route_wall = scope.wall().as_secs();
            let wall = window
                .sample()
                .map_err(|_| ApprovalFailure::Unavailable)?
                .as_secs();
            if wall < route_wall {
                return Err(ApprovalFailure::Clock);
            }
            let account = scope.account();
            let origin = scope.origin();
            let selector = Selector {
                server: selection.server,
                id: selection.offer_id,
                revision: selection.revision,
            };
            let generation = selection.generation;
            self.owner
                .cursor
                .unchanged()
                .map_err(|_| ApprovalFailure::Integrity)?;
            let (held, delivery) = self.owner.checkpoint.read_installed_approval(
                ApprovalScope {
                    source: &self.owner.source,
                    account,
                    origin,
                    selector,
                    generation,
                },
                wall,
            )?;
            let deadline = held.pending_deadline().ok_or(ApprovalFailure::Integrity)?;
            let bounded = window
                .clamp_signed(deadline)
                .map_err(|_| ApprovalFailure::Unavailable)?;
            if !request.check_live() {
                return Err(ApprovalFailure::Unavailable);
            }
            let joined = self
                .native
                .read_joined_for_approval(
                    self.owner,
                    &held,
                    &delivery,
                    &WelcomeFence::Candidate(&bounded),
                )
                .map_err(|_| ApprovalFailure::Stale)?;
            let (binding, file) = held.stage();
            let owner = &mut *self.owner;
            owner
                .cursor
                .unchanged()
                .map_err(|_| ApprovalFailure::Integrity)?;
            let cursor = &owner.cursor;
            let paths = &owner.paths;
            owner.checkpoint.decide_installed_approval(
                ApprovalScope {
                    source: &owner.source,
                    account,
                    origin,
                    selector,
                    generation,
                },
                &held,
                &joined,
                self.native.quota,
                wall,
                DecisionGuard(&mut |guard| {
                    cursor
                        .unchanged_identity()
                        .map_err(|_| ApprovalFailure::Integrity)?;
                    paths
                        .reopen_provisional(binding, file)
                        .map_err(|_| ApprovalFailure::Stale)?;
                    exchange.check(lease, selection, guard)?;
                    Ok(window
                        .sample()
                        .map_err(|_| ApprovalFailure::Unavailable)?
                        .as_secs())
                }),
            )
        });
        ApprovalSettlement { result, exchange }
    }
}
