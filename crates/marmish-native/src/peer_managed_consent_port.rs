//! One managed Consent send on the registered owner's authenticated port.

use std::sync::atomic::AtomicBool;

use super::*;
use crate::{
    peer_candidate_startup::Readiness,
    peer_invitation::{Error as Local, NativeWindow, Selector, receiver_inventory::RecoveredOwner},
    peer_quota_vfs::dispatch::ReceiverQuota,
};

type LocalResult<T> = std::result::Result<T, Local>;

#[derive(Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum Publication {
    Unavailable,
    NotSent(Local),
    Rejected,
    PossiblyExposed(Local),
    Settled(Wake),
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum Wake {
    Completed(usize),
    Failed,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::peer_invitation) enum Cut {
    Staged,
    Accepted,
    WakeBeforeCommit,
}

impl PeerRecipientPort {
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "consumed by the separately mounted Manual actor")
    )]
    pub(in crate::peer_invitation) async fn publish_managed_consent(
        &mut self,
        owner: &mut RecoveredOwner<'_>,
        quota: &ReceiverQuota<'_>,
        selector: Selector,
        readiness: Readiness,
        stop: Arc<AtomicBool>,
    ) -> Publication {
        self.publish_managed_consent_inner(owner, quota, selector, readiness, stop, |_| Ok(()))
            .await
    }

    #[cfg(test)]
    pub(in crate::peer_invitation) async fn fixture_managed_consent_cut(
        &mut self,
        owner: &mut RecoveredOwner<'_>,
        quota: &ReceiverQuota<'_>,
        selector: Selector,
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        cut: impl FnMut(Cut) -> LocalResult<()>,
    ) -> Publication {
        self.publish_managed_consent_inner(owner, quota, selector, readiness, stop, cut)
            .await
    }

    async fn publish_managed_consent_inner(
        &mut self,
        owner: &mut RecoveredOwner<'_>,
        quota: &ReceiverQuota<'_>,
        selector: Selector,
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        mut cut: impl FnMut(Cut) -> LocalResult<()>,
    ) -> Publication {
        let prepared = self.use_current_native(|route| {
            let Some(queued) = owner.due_managed_consent(&route, selector)? else {
                return Ok(None);
            };
            let window = route
                .window(readiness, stop, queued.target().deadline())
                .map_err(|_| Local::Clock)?;
            Ok(Some((queued, window, route.horizon())))
        });
        let (queued, window, horizon) = match prepared.map_err(Local::from).and_then(|value| value)
        {
            Ok(Some(prepared)) => prepared,
            Ok(None) => return Publication::Unavailable,
            Err(error) => return Publication::NotSent(error),
        };
        let turn = match owner.reserve_managed_consent(quota, queued, &window) {
            Ok(Some(turn)) => turn,
            Ok(None) => return Publication::Unavailable,
            Err(error) => return Publication::NotSent(error),
        };
        let now = Instant::now();
        let deadline = match window
            .remaining()
            .and_then(|remaining| now.checked_add(remaining).ok_or(Local::Clock))
        {
            Ok(deadline) => horizon.min(deadline),
            Err(error) => return Publication::NotSent(error),
        };
        let attempt = turn.attempt();
        let operation = operation(attempt);
        let mut current = |candidate, _| candidate == operation && window.check().is_ok();
        let permit = ControlSendPermit {
            scope: self.0.scope.clone(),
            author: attempt.account(),
            remote: attempt.founder(),
            carrier: attempt.carrier(),
            digest: sealed_digest(attempt.bytes()),
            operation,
            deadline: attempt.deadline(),
            bytes: attempt.bytes(),
            current: &mut current,
        };
        let mut barrier_failure = None;
        let ack = match self
            .publish_control_with(permit, deadline, |_| {
                cut(Cut::Staged)
                    .and_then(|()| owner.managed_before_flush(quota, &turn))
                    .map_err(|error| {
                        barrier_failure = Some(error);
                        Failure::Binding
                    })
            })
            .await
        {
            ControlOutcome::NotSent(error) => {
                return Publication::NotSent(barrier_failure.unwrap_or_else(|| error.into()));
            }
            ControlOutcome::Rejected => return Publication::Rejected,
            ControlOutcome::PossiblyExposed(error) => {
                return Publication::PossiblyExposed(error.into());
            }
            ControlOutcome::Accepted(ack) => ack,
        };
        let settled = cut(Cut::Accepted).and_then(|()| {
            self.use_accepted(&ack, |checked| {
                owner.settle_managed_consent(quota, turn, checked)
            })
            .map_err(Local::from)?
        });
        let settled = match settled {
            Ok(settled) => settled,
            Err(error) => {
                self.0.retire();
                return Publication::PossiblyExposed(error);
            }
        };
        Publication::Settled(self.wake_managed_ack(&ack, operation, &window, |checked| {
            owner.wake_managed_delivery(settled, checked, &window, || cut(Cut::WakeBeforeCommit))
        }))
    }

    fn wake_managed_ack(
        &mut self,
        ack: &Acknowledgement,
        operation: Operation,
        window: &NativeWindow,
        wake: impl FnOnce(CheckedAck<'_>) -> LocalResult<usize>,
    ) -> Wake {
        let woken = self.use_accepted(ack, |checked| {
            (checked.operation == operation && checked.carrier == operation.id)
                .then(|| wake(checked))
        });
        match woken {
            Ok(Some(Ok(count))) => Wake::Completed(count),
            Err(_) | Ok(None) => {
                self.0.retire();
                Wake::Failed
            }
            Ok(Some(Err(_))) => {
                if window.check().is_err() {
                    self.0.retire();
                }
                Wake::Failed
            }
        }
    }
}
