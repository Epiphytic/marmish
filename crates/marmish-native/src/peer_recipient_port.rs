//! Checked physical observations from one authenticated recipient socket.
//!
//! The future installed worker supplies durable admission and row authority.
//! Legacy fixture connections cannot create a port or send permit.

use sha2::{Digest, Sha256};

use super::*;

/// An owner borrowing this observation may only consume it synchronously.
pub(in crate::peer_invitation) struct CheckedPage<'a> {
    pub(in crate::peer_invitation) account: PublicKey,
    pub(in crate::peer_invitation) origin: &'a str,
    pub(in crate::peer_invitation) wall: u64,
    pub(in crate::peer_invitation) request: &'a str,
    pub(in crate::peer_invitation) bounds: [u64; 2],
    pub(in crate::peer_invitation) records: &'a [Arrival],
}

pub(in crate::peer_invitation) struct CheckedLive<'a> {
    pub(in crate::peer_invitation) account: PublicKey,
    pub(in crate::peer_invitation) origin: &'a str,
    pub(in crate::peer_invitation) wall: u64,
    pub(in crate::peer_invitation) record: &'a Arrival,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::peer_invitation) struct Operation {
    pub(in crate::peer_invitation) id: [u8; 32],
    pub(in crate::peer_invitation) generation: u64,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
#[derive(PartialEq, Eq)]
enum CarrierLocation<'a> {
    Live(&'a str),
    History {
        request: &'a str,
        bounds: [u64; 2],
        index: usize,
    },
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(in crate::peer_invitation) struct CarrierWitness {
    scope: Arc<Scope>,
    location: CarrierCoordinates,
    id: [u8; 32],
    stamp: u64,
    digest: [u8; 32],
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
enum CarrierCoordinates {
    Live(String),
    History {
        request: String,
        bounds: [u64; 2],
        index: usize,
    },
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(in crate::peer_invitation) struct CheckedCarrier<'a> {
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    scope: &'a Arc<Scope>,
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    location: CarrierLocation<'a>,
    account: PublicKey,
    origin: &'a str,
    wall: u64,
    record: crate::peer_invitation::RawRecord<'a>,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl<'a> CheckedCarrier<'a> {
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(in crate::peer_invitation) fn witness(&self) -> CarrierWitness {
        let location = match self.location {
            CarrierLocation::Live(subscription) => CarrierCoordinates::Live(subscription.into()),
            CarrierLocation::History {
                request,
                bounds,
                index,
            } => CarrierCoordinates::History {
                request: request.into(),
                bounds,
                index,
            },
        };
        CarrierWitness {
            scope: Arc::clone(self.scope),
            location,
            id: self.record.id,
            stamp: self.record.stamp,
            digest: Sha256::digest(self.record.bytes).into(),
        }
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(in crate::peer_invitation) fn matches(&self, witness: &CarrierWitness) -> bool {
        let location = match &witness.location {
            CarrierCoordinates::Live(subscription) => CarrierLocation::Live(subscription),
            CarrierCoordinates::History {
                request,
                bounds,
                index,
            } => CarrierLocation::History {
                request,
                bounds: *bounds,
                index: *index,
            },
        };
        Arc::ptr_eq(self.scope, &witness.scope)
            && self.location == location
            && self.record.id == witness.id
            && self.record.stamp == witness.stamp
            && <[u8; 32]>::from(Sha256::digest(self.record.bytes)) == witness.digest
    }

    pub(in crate::peer_invitation) fn account(&self) -> PublicKey {
        self.account
    }

    pub(in crate::peer_invitation) fn origin(&self) -> &'a str {
        self.origin
    }

    pub(in crate::peer_invitation) fn wall(&self) -> u64 {
        self.wall
    }

    pub(in crate::peer_invitation) fn record(&self) -> crate::peer_invitation::RawRecord<'a> {
        self.record
    }
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "peer_recipient_delivery_port.rs"]
mod delivery;
#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(in crate::peer_invitation) use delivery::{
    CheckedDeliveryRoute, CheckedExpiredRoute, CheckedSavedDelivery, SavedPortWitness,
};

pub(in crate::peer_invitation) struct CheckedAck<'a> {
    account: PublicKey,
    origin: &'a str,
    wall: u64,
    carrier: [u8; 32],
    operation: Operation,
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
use crate::peer_invitation::store::consent_checkpoint::{
    self, provisioning::prepared_consent::Attempt,
};

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
fn operation(attempt: &Attempt) -> Operation {
    Operation {
        id: attempt.carrier(),
        generation: attempt.sequence(),
    }
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl CheckedAck<'_> {
    pub(in crate::peer_invitation) fn observation(&self) -> (PublicKey, &str, u64) {
        (self.account, self.origin, self.wall)
    }

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

pub(in crate::peer_invitation) struct Acknowledgement {
    scope: Arc<Scope>,
    carrier: [u8; 32],
    operation: Operation,
    deadline: u64,
}

pub(in crate::peer_invitation) enum ControlOutcome {
    NotSent(Failure),
    Rejected,
    Accepted(Acknowledgement),
    PossiblyExposed(Failure),
}

/// Only a verified durable v2 send row may issue a production permit. That
/// producer is unmounted. Tests issue this from an exact prepared v2 frame.
pub(in crate::peer_invitation) struct ControlSendPermit<'a> {
    scope: Arc<Scope>,
    author: PublicKey,
    remote: PublicKey,
    carrier: [u8; 32],
    digest: [u8; 32],
    operation: Operation,
    deadline: u64,
    bytes: &'a [u8],
    current: &'a mut dyn FnMut(Operation, u64) -> bool,
}

impl ControlSendPermit<'_> {
    fn check(&mut self, wall: u64) -> Result<()> {
        if wall >= self.deadline || !(self.current)(self.operation, wall) {
            return Err(Failure::Retired);
        }
        Ok(())
    }
}

pub(in crate::peer_invitation) struct PeerRecipientPort(Owner);

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
pub(in crate::peer_invitation) struct Incarnation(Arc<Scope>);

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl Incarnation {
    pub(in crate::peer_invitation) fn is_live(&self) -> bool {
        self.0.live.load(std::sync::atomic::Ordering::Acquire)
    }
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
impl PeerRecipientPort {
    pub(in crate::peer_invitation) fn claim_recovery(&mut self) -> Result<Incarnation> {
        self.check_observation()?;
        if std::mem::replace(&mut self.0.recovery_claimed, true) {
            return Err(Failure::Binding);
        }
        Ok(Incarnation(self.0.scope.clone()))
    }

    pub(in crate::peer_invitation) fn matches_incarnation(
        &self,
        incarnation: &Incarnation,
    ) -> bool {
        self.0.current(&incarnation.0)
    }

    pub(in crate::peer_invitation) fn take_buffered_live(&mut self) -> Result<Option<LiveRecord>> {
        self.check_observation()?;
        Ok(self.0.pop_live())
    }
}

impl PeerRecipientPort {
    pub(in crate::peer_invitation) async fn open(
        admission: ReceiverRouteAdmission,
        deadline: Instant,
    ) -> Result<Self> {
        Owner::open_checked(admission, deadline).await.map(Self)
    }

    pub(in crate::peer_invitation) async fn page(
        &mut self,
        bounds: [u64; 2],
        deadline: Instant,
    ) -> Result<Page> {
        self.check_observation()?;
        self.0.page(bounds, deadline).await
    }

    pub(in crate::peer_invitation) async fn wait_live(
        &mut self,
        deadline: Instant,
    ) -> Result<Option<LiveRecord>> {
        self.check_observation()?;
        self.0.wait_live(deadline).await
    }

    pub(in crate::peer_invitation) fn use_page<T>(
        &mut self,
        page: &Page,
        use_checked: impl FnOnce(CheckedPage<'_>) -> T,
    ) -> Result<T> {
        if !self.0.accepts_page(page) {
            return Err(Failure::Binding);
        }
        let (account, wall) = self.check_observation()?;
        let (request, bounds) = page.request();
        Ok(use_checked(CheckedPage {
            account,
            origin: &self.0.scope.origin,
            wall,
            request,
            bounds,
            records: page.records(),
        }))
    }

    pub(in crate::peer_invitation) fn use_live<T>(
        &mut self,
        record: &LiveRecord,
        use_checked: impl FnOnce(CheckedLive<'_>) -> T,
    ) -> Result<T> {
        if !self.0.accepts_live(record) {
            return Err(Failure::Binding);
        }
        let (account, wall) = self.check_observation()?;
        Ok(use_checked(CheckedLive {
            account,
            origin: &self.0.scope.origin,
            wall,
            record: record.arrival(),
        }))
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(in crate::peer_invitation) fn use_page_carrier<T>(
        &mut self,
        page: &Page,
        index: usize,
        use_checked: impl FnOnce(CheckedCarrier<'_>) -> T,
    ) -> Result<T> {
        self.use_page(page, |checked| {
            let [lower, upper] = checked.bounds;
            let arrival = checked.records.get(index).ok_or(Failure::Binding)?;
            if !(lower..=upper).contains(&arrival.stamp) {
                return Err(Failure::Binding);
            }
            Ok(use_checked(CheckedCarrier {
                #[cfg(all(
                    any(test, marmish_peer_receiver_candidate),
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                scope: &page.scope,
                #[cfg(all(
                    any(test, marmish_peer_receiver_candidate),
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                location: CarrierLocation::History {
                    request: checked.request,
                    bounds: checked.bounds,
                    index,
                },
                account: checked.account,
                origin: checked.origin,
                wall: checked.wall,
                record: crate::peer_invitation::record(arrival),
            }))
        })?
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(in crate::peer_invitation) fn use_live_carrier<T>(
        &mut self,
        live: &LiveRecord,
        use_checked: impl FnOnce(CheckedCarrier<'_>) -> T,
    ) -> Result<T> {
        self.use_live(live, |checked| {
            use_checked(CheckedCarrier {
                #[cfg(all(
                    any(test, marmish_peer_receiver_candidate),
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                scope: &live.scope,
                #[cfg(all(
                    any(test, marmish_peer_receiver_candidate),
                    target_os = "linux",
                    target_arch = "x86_64"
                ))]
                location: CarrierLocation::Live(&live.subscription),
                account: checked.account,
                origin: checked.origin,
                wall: checked.wall,
                record: crate::peer_invitation::record(checked.record),
            })
        })
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    #[allow(dead_code, reason = "candidate actor mounting is reviewed separately")]
    pub(in crate::peer_invitation) fn use_current_recipient<T>(
        &mut self,
        use_checked: impl FnOnce(PublicKey, &str, u64) -> T,
    ) -> Result<T> {
        let (account, wall) = self.check_observation()?;
        Ok(use_checked(account, &self.0.scope.origin, wall))
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    #[allow(dead_code, reason = "consumed by the separately mounted Manual actor")]
    pub(in crate::peer_invitation) fn use_current_native<T>(
        &mut self,
        use_checked: impl FnOnce(crate::peer_relay_trust::NativeRouteWindow<'_>) -> T,
    ) -> Result<T> {
        let result = self.0.socket().and_then(|socket| {
            let physical = socket.physical.as_mut().ok_or(Failure::Binding)?;
            let scope = physical.native_window().map_err(|_| Failure::Retired)?;
            Ok(use_checked(scope))
        });
        result.or_else(|err| self.0.fail(err))
    }

    pub(in crate::peer_invitation) fn use_accepted<T>(
        &mut self,
        ack: &Acknowledgement,
        use_checked: impl FnOnce(CheckedAck<'_>) -> T,
    ) -> Result<T> {
        if !self.0.current(&ack.scope) {
            return Err(Failure::Binding);
        }
        let (account, wall) = self.check_observation()?;
        if wall >= ack.deadline {
            return Err(Failure::Deadline);
        }
        Ok(use_checked(CheckedAck {
            account,
            origin: &self.0.scope.origin,
            wall,
            carrier: ack.carrier,
            operation: ack.operation,
        }))
    }

    fn check_observation(&mut self) -> Result<(PublicKey, u64)> {
        let result = self.0.socket().and_then(|socket| {
            let physical = socket.physical.as_mut().ok_or(Failure::Binding)?;
            let wall = physical.observe().map_err(|_| Failure::Retired)?;
            Ok((physical.account(), wall))
        });
        result.or_else(|err| self.0.fail(err))
    }

    pub(in crate::peer_invitation) async fn stop(&mut self, deadline: Instant) -> Result<()> {
        self.check_observation()?;
        self.0.stop(deadline).await
    }

    pub(in crate::peer_invitation) async fn publish_control(
        &mut self,
        permit: ControlSendPermit<'_>,
        deadline: Instant,
    ) -> ControlOutcome {
        self.publish_control_with(permit, deadline, |_| Ok(()))
            .await
    }

    async fn publish_control_with(
        &mut self,
        mut permit: ControlSendPermit<'_>,
        deadline: Instant,
        before_flush: impl FnOnce(u64) -> Result<()>,
    ) -> ControlOutcome {
        if !self.0.current(&permit.scope) {
            return ControlOutcome::NotSent(Failure::Binding);
        }
        let mut exposed = false;
        let mut budget = Budget::default();
        let result = self
            .0
            .active(async |owner| {
                let mut end = deadline;
                let socket = owner.socket()?;
                socket.refresh(&mut end)?;
                let route = socket.physical.as_ref().ok_or(Failure::Binding)?;
                if permit.author != route.account() {
                    return Err(Failure::Binding);
                }
                owner.predrain(&mut budget, end).await?;
                if permit.digest != sealed_digest(permit.bytes) {
                    return Err(Failure::Binding);
                }
                let frame = exact_publication(permit.bytes, permit.remote, permit.carrier)?;
                owner
                    .socket()?
                    .fenced_write(
                        frame,
                        end,
                        Some(permit.deadline),
                        |wall| permit.check(wall),
                        before_flush,
                        &mut exposed,
                    )
                    .await?;
                owner
                    .socket()?
                    .physical
                    .as_ref()
                    .ok_or(Failure::Binding)?
                    .clamp_wall(&mut end, permit.deadline)
                    .map_err(|_| Failure::Retired)?;
                let positive = owner.read_ack(permit.carrier, &mut budget, end).await?;
                let wall = owner.socket()?.refresh(&mut end)?.ok_or(Failure::Binding)?;
                permit.check(wall)?;
                Ok(positive)
            })
            .await;
        match result {
            Ok(true) => ControlOutcome::Accepted(Acknowledgement {
                scope: permit.scope.clone(),
                carrier: permit.carrier,
                operation: permit.operation,
                deadline: permit.deadline,
            }),
            Ok(false) => ControlOutcome::Rejected,
            Err(err) => {
                self.0.retire();
                if exposed {
                    ControlOutcome::PossiblyExposed(err)
                } else {
                    ControlOutcome::NotSent(err)
                }
            }
        }
    }
}

fn sealed_digest(frame: &[u8]) -> [u8; 32] {
    Sha256::digest(frame).into()
}

#[cfg(all(
    any(test, marmish_peer_receiver_candidate),
    target_os = "linux",
    target_arch = "x86_64"
))]
#[path = "peer_managed_consent_port.rs"]
pub(in crate::peer_invitation) mod managed_consent;

#[cfg(test)]
#[path = "peer_authenticated_route_tests.rs"]
mod tests;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "peer_founder_port.rs"]
pub(in crate::peer_invitation) mod founder;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "peer_installed_consent_port.rs"]
mod installed_consent;
#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
pub(in crate::peer_invitation) use installed_consent::PublicationOutcome;
