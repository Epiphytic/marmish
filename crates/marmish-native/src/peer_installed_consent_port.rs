//! Test-only bridge from durable Prepared Consent to one authenticated port.

use super::*;
use crate::peer_invitation::{
    Error as Local, Selector,
    prepared_consent::installed_consent::Fixture,
    store::consent_checkpoint::{self, provisioning::prepared_consent::Attempt},
};

#[derive(Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) enum PublicationOutcome {
    Unavailable,
    NotSent(Local),
    Rejected,
    PossiblyExposed(Local),
    Settled,
}

fn operation(attempt: &Attempt) -> Operation {
    Operation {
        id: attempt.carrier(),
        generation: attempt.sequence(),
    }
}

impl CheckedAck<'_> {
    pub(in crate::peer_invitation) fn exact_attempt(
        &self,
        attempt: &Attempt,
        origin: &str,
    ) -> consent_checkpoint::Result<u64> {
        if self.account != attempt.account()
            || self.origin != origin
            || self.carrier != attempt.carrier()
            || self.operation != operation(attempt)
        {
            return Err(consent_checkpoint::Error::Source);
        }
        if self.wall >= attempt.deadline() {
            return Err(consent_checkpoint::Error::Expired);
        }
        Ok(self.wall)
    }
}

impl PeerRecipientPort {
    pub(in crate::peer_invitation) async fn publish_prepared_consent(
        &mut self,
        fixture: &mut Fixture<'_>,
        selector: Selector,
        deadline: Instant,
    ) -> PublicationOutcome {
        let (account, wall) = match check_deadline(deadline).and_then(|()| self.check_observation())
        {
            Ok(observation) => observation,
            Err(error) => return PublicationOutcome::NotSent(error.into()),
        };
        let reserved = match fixture.reserve(account, &self.0.scope.origin, selector, wall) {
            Ok(Some(reserved)) => reserved,
            Ok(None) => return PublicationOutcome::Unavailable,
            Err(error) => return PublicationOutcome::NotSent(error),
        };
        let attempt = &reserved.attempt;
        let operation = operation(attempt);
        // The exclusive fixture borrow fixes the local turn between fences.
        // Per-poll authorization must stay in memory.
        let mut current = |candidate, _| candidate == operation;
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
        let mut local_failure = None;
        let outcome = self
            .publish_control_with(permit, deadline, |wall| {
                fixture.before_flush(&reserved, wall).map_err(|error| {
                    local_failure = Some(error);
                    Failure::Binding
                })
            })
            .await;
        match outcome {
            ControlOutcome::NotSent(error) => {
                PublicationOutcome::NotSent(local_failure.unwrap_or_else(|| error.into()))
            }
            ControlOutcome::Rejected => PublicationOutcome::Rejected,
            ControlOutcome::PossiblyExposed(error) => {
                PublicationOutcome::PossiblyExposed(error.into())
            }
            ControlOutcome::Accepted(ack) => {
                match self.use_accepted(&ack, |view| fixture.settle(reserved, view)) {
                    Ok(Ok(settled)) => {
                        if let Err(error) = self
                            .use_accepted(&ack, |view| {
                                fixture.wake_delivery(settled, view.account, view.origin, view.wall)
                            })
                            .map_err(Local::from)
                            .and_then(|result| result)
                        {
                            fixture.record_wake_failure(error);
                        }
                        PublicationOutcome::Settled
                    }
                    Ok(Err(error)) => {
                        self.0.retire();
                        PublicationOutcome::PossiblyExposed(error)
                    }
                    Err(error) => {
                        self.0.retire();
                        PublicationOutcome::PossiblyExposed(error.into())
                    }
                }
            }
        }
    }
}
