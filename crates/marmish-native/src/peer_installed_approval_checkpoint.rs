//! Same-row native Manual decision in the independently provisioned v3 schema.

use super::*;
use crate::peer_invitation::receiver_inventory::v2_pending::DecisionGuard;

pub(in crate::peer_invitation) use decision::{ApprovalFailure, ApprovalGuard, ApprovalScope};

#[path = "peer_installed_approval_decision.rs"]
mod decision;

#[allow(dead_code, reason = "candidate approval actor is mounted separately")]
impl Checkpoint {
    pub(in crate::peer_invitation) fn read_installed_approval(
        &mut self,
        scope: ApprovalScope<'_>,
        wall: u64,
    ) -> decision::Result<(HeldEvidence, Delivery)> {
        let cx = self.approval_context(scope)?;
        let tx = self.cursor.conn.transaction().map_err(Error::from)?;
        let held = approval_row(&tx, &cx, scope, wall)?;
        drop(tx);
        let delivery = self.reopen_installed_delivery(&held, wall, true)?;
        Ok((held, delivery))
    }

    fn approval_context(&self, scope: ApprovalScope<'_>) -> decision::Result<Context> {
        let cx = self.installed_consent_context(scope.source, scope.account, scope.origin)?;
        if !cx.epoch.is_approval_v3()
            || !matches!(self.custody_mode(scope.selector.server)?, Mode::Manual)
        {
            return Err(ApprovalFailure::Stale);
        }
        Ok(cx)
    }

    pub(in crate::peer_invitation) fn decide_installed_approval(
        &mut self,
        scope: ApprovalScope<'_>,
        prior: &HeldEvidence,
        joined: &InstalledJoinedReadback<'_>,
        quota: &ReceiverQuota<'_>,
        wall: u64,
        mut guard: DecisionGuard<'_>,
    ) -> decision::Result<()> {
        let delivery = self.reopen_installed_delivery(prior, wall, approval_pin(prior).is_ok())?;
        if !joined.matches_quota(quota) || !joined.matches(prior, &delivery) {
            return Err(ApprovalFailure::Stale);
        }
        let cx = self.approval_context(scope)?;
        let check = |tx: &Transaction<'_>, wall| {
            let current = approval_row(tx, &cx, scope, wall)?;
            let (saved, decided_at) = approval_pin(&current)?;
            if !current.same_turn(prior)
                || (
                    saved.native.group.as_slice(),
                    saved.native.epoch,
                    saved.native.epoch_identity,
                    saved.native.own_leaf,
                ) != joined.coordinates()
            {
                return Err(ApprovalFailure::Stale);
            }
            Ok((saved.qualified_at, saved.deadline, decided_at))
        };
        if let Progress::Activating { .. } = prior.saved.progress {
            // The capability must be consumed before the final read-only retry.
            let sampled = guard.check(ApprovalGuard::Consume)?;
            let tx = self.cursor.conn.transaction().map_err(Error::from)?;
            let (qualified, deadline, decided_at) = check(&tx, sampled)?;
            let decided_at = decided_at.ok_or(ApprovalFailure::Stale)?;
            let floors = decision::floors(&tx, cx.origin)?;
            if floors.0 < decided_at || floors.1 < decided_at {
                return Err(ApprovalFailure::Integrity);
            }
            decision::check_wall(
                sampled,
                wall.max(qualified).max(floors.0).max(floors.1),
                deadline,
            )?;
            let final_wall = guard.check(ApprovalGuard::Verify)?;
            decision::check_wall(final_wall, sampled, deadline)?;
            return Ok(());
        }
        let tx = self.cursor.begin(cx.origin)?;
        let (qualified, deadline, decided_at) = check(&tx, wall)?;
        if decided_at.is_some() {
            return Err(ApprovalFailure::Stale);
        }
        let floors = decision::floors(&tx, cx.origin)?;
        let at = guard.check(ApprovalGuard::Consume)?;
        decision::check_wall(
            at,
            wall.max(qualified).max(floors.0).max(floors.1),
            deadline,
        )?;
        let changed = decision::record(
            tx,
            cx.origin,
            prior.delivery_carrier(),
            scope.generation,
            at,
            |tx| {
                cx.verify(tx, at)?;
                if decision::floors(tx, cx.origin)? != (at, at) {
                    return Err(ApprovalFailure::Integrity);
                }
                let final_wall = guard.check(ApprovalGuard::Verify)?;
                decision::check_wall(final_wall, at, deadline)
            },
        )?;
        #[cfg(test)]
        record_custody_changes(changed);
        #[cfg(not(test))]
        let _ = changed;
        Ok(())
    }
}

fn approval_row(
    tx: &Transaction<'_>,
    cx: &Context,
    scope: ApprovalScope<'_>,
    wall: u64,
) -> decision::Result<HeldEvidence> {
    cx.verify(tx, wall)?;
    let (origin, global) = decision::floors(tx, cx.origin)?;
    if wall < origin.max(global) {
        return Err(ApprovalFailure::Clock);
    }
    let held = read_pending_custody_row(tx, cx, scope.selector, scope.generation)?
        .ok_or(ApprovalFailure::Stale)?;
    let (joined, _) = approval_pin(&held)?;
    decision::check_wall(wall, joined.qualified_at, joined.deadline)?;
    Ok(held)
}

fn approval_pin(held: &HeldEvidence) -> decision::Result<(&Qualified, Option<u64>)> {
    if held.is_active() || held.saved.automatic || held.stage().0.coordinates().0 {
        return Err(ApprovalFailure::Stale);
    }
    match &held.saved.progress {
        Progress::Pending { joined, .. } => Ok((joined, None)),
        Progress::Activating {
            joined,
            automatic: false,
            at,
            ..
        } => Ok((joined, Some(*at))),
        _ => Err(ApprovalFailure::Stale),
    }
}
