//! Single direct-WSS Terminal admission on a verified Command binding.
use super::terminal_enrollment::Owner;
use super::terminal_enrollment_room::RoomWitness;
use super::terminal_enrollment_transport::{Authority, Lane};
use super::{AuthenticatedCommandContext, CommandDirection, TransportEndpoint};
use crate::terminal_bootstrap::state::{Advance, Exchange, Time};
use crate::terminal_bootstrap::{BoundSigning, Codec, Step};
use crate::terminal_fence::{AttachmentFence, FenceState};
use crate::terminal_frames;
use crate::transport::{RelayConnector, RelayPolicy};
use crate::{Error, Result};
use cgka_traits::TransportDeliveryPlane;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout_at};

#[path = "terminal_live_enrollment_clock.rs"]
mod clock;

use clock::{ConsumedProbes, Freshness, Sample};

const BINDING: Error = Error::Invalid("terminal_bootstrap_binding");
const ROUTE: Error = Error::Transport("terminal_transport_unavailable");
const GRANT: Error = Error::Invalid("terminal_enrollment_grant");
const CAPACITY: Error = Error::Invalid("terminal_enrollment_capacity");
const ORDER: Error = Error::Invalid("terminal_enrollment_order");
const BOOTSTRAP_OUTER: usize = 32 * 1024;
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(30);
const CHALLENGE_TIMEOUT: Duration = Duration::from_secs(5);
const ROOM_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

pub(super) struct Carrier {
    context: AuthenticatedCommandContext,
    fence: Arc<AttachmentFence>,
    scope: RelayPolicy,
}

pub(super) fn admit(
    context: AuthenticatedCommandContext,
    fence: Arc<AttachmentFence>,
    profile: &RelayPolicy,
    selected: &str,
) -> Result<Carrier> {
    if !fence.matches_digest(&context.authority_digest())
        || fence.state() != FenceState::Live
        || context.target_uid() != unsafe { libc::geteuid() }
    {
        return Err(BINDING);
    }
    if !context.relay_endpoints().iter().any(|url| url == selected) {
        return Err(ROUTE);
    }
    let scope = profile.direct_wss_scope(selected).ok_or(ROUTE)?;
    Ok(Carrier {
        context,
        fence,
        scope,
    })
}

#[derive(Default)]
pub(super) struct InboundState {
    clock: Freshness,
    consumed: ConsumedProbes,
}

pub(super) struct Bootstrap {
    carrier: Carrier,
    exchange: Exchange,
    signing: BoundSigning,
    lane: Lane,
}

pub(super) struct GrantedEnrollment {
    carrier: Carrier,
    reserved: ReservedRoom,
}

pub(super) struct RoomPermit(Option<OwnedSemaphorePermit>);

impl RoomPermit {
    pub(super) fn new(permit: OwnedSemaphorePermit) -> Self {
        Self(Some(permit))
    }

    fn release_after_reap(mut self) {
        drop(self.0.take());
    }
}

impl Drop for RoomPermit {
    fn drop(&mut self) {
        // An abandoned, failed or cancelled transition must not create
        // capacity while its source or room may still be running.
        if let Some(permit) = self.0.take() {
            permit.forget();
        }
    }
}

struct ReservedRoom {
    owner: Option<(Owner, RoomPermit)>,
}

impl ReservedRoom {
    fn start_reap(&mut self) -> Option<JoinHandle<Result<()>>> {
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let (owner, permit) = self.owner.take()?;
        owner.alias_adapter.authority.retire();
        Some(runtime.spawn(async move {
            let result = owner.close().await;
            if result.is_ok() {
                permit.release_after_reap();
            }
            result
        }))
    }

    async fn close(mut self) -> Result<()> {
        self.start_reap().ok_or(ORDER)?.await.map_err(|_| ORDER)?
    }

    fn owner_mut(&mut self) -> &mut Owner {
        &mut self.owner.as_mut().expect("owned native Terminal room").0
    }
}

impl Drop for ReservedRoom {
    fn drop(&mut self) {
        let _ = self.start_reap();
    }
}

#[derive(Clone, PartialEq, Eq)]
struct SelectionBinding {
    transcript: [u8; 32],
    command_digest: [u8; 32],
    generation: u64,
    local: [u8; 32],
    peer: [u8; 32],
    direction: CommandDirection,
    uid: u32,
    native: RoomWitness,
}

pub(super) struct ReadyEnrollment {
    enrollment: GrantedEnrollment,
    binding: SelectionBinding,
}

/// Private stand-in for the future verified local selection interface.
/// This module does not accept a claim from transport or IPC.
pub(super) struct FakeWriterClaim {
    binding: SelectionBinding,
    attachment: [u8; 16],
    attachment_generation: NonZeroU64,
    deadline: Instant,
}

pub(super) struct AttachedWriter {
    reserved: ReservedRoom,
    pub(super) frames: terminal_frames::Binding,
}

impl GrantedEnrollment {
    pub(super) async fn close(self) -> Result<()> {
        self.reserved.close().await
    }

    pub(super) fn owner_mut(&mut self) -> &mut Owner {
        self.reserved.owner_mut()
    }

    async fn selection_binding(&mut self, granted: &impl Fn() -> bool) -> Result<SelectionBinding> {
        permitted(granted)?;
        let owner = self.reserved.owner_mut();
        owner.alias_adapter.authority.wait_live().await?;
        owner.alias_adapter.check().await?;
        let transcript = owner.ready().await?;
        let native = owner.room.witness()?;
        let context = &self.carrier.context;
        Ok(SelectionBinding {
            transcript,
            command_digest: context.authority_digest(),
            generation: self.carrier.fence.generation().get(),
            local: context.local_account(),
            peer: context.peer_account(),
            direction: context.direction(),
            uid: context.target_uid(),
            native,
        })
    }

    pub(super) async fn ready(mut self, granted: &impl Fn() -> bool) -> Result<ReadyEnrollment> {
        let binding = self.selection_binding(granted).await?;
        Ok(ReadyEnrollment {
            enrollment: self,
            binding,
        })
    }
}

impl ReadyEnrollment {
    pub(super) fn fake_writer_claim(
        &self,
        attachment: [u8; 16],
        attachment_generation: NonZeroU64,
        deadline: Instant,
    ) -> FakeWriterClaim {
        FakeWriterClaim {
            binding: self.binding.clone(),
            attachment,
            attachment_generation,
            deadline,
        }
    }

    pub(super) fn validate_claim(&self, claim: &FakeWriterClaim, now: Instant) -> Result<()> {
        if claim.binding != self.binding || claim.attachment == [0; 16] || now >= claim.deadline {
            return Err(BINDING);
        }
        Ok(())
    }

    pub(super) async fn select(
        mut self,
        claim: FakeWriterClaim,
        granted: &impl Fn() -> bool,
    ) -> Result<AttachedWriter> {
        self.validate_claim(&claim, Instant::now())?;
        if self.enrollment.selection_binding(granted).await? != self.binding {
            return Err(BINDING);
        }
        permitted(granted)?;
        let frames = terminal_frames::Binding {
            transcript: self.binding.transcript,
            attachment: claim.attachment,
            generation: claim.attachment_generation.get(),
            role: terminal_frames::Role::Writer,
        };
        self.enrollment
            .owner_mut()
            .alias_adapter
            .authority
            .begin_attachment(frames, claim.deadline.min(Instant::now() + ROOM_LIFETIME))?;
        self.enrollment.owner_mut().detach_alias().await?;
        Ok(AttachedWriter {
            reserved: self.enrollment.reserved,
            frames,
        })
    }
}

impl AttachedWriter {
    pub(super) fn owner_mut(&mut self) -> &mut Owner {
        self.reserved.owner_mut()
    }

    pub(super) async fn close(self) -> Result<()> {
        self.reserved.close().await
    }
}

impl Carrier {
    fn verify_scope(&self, connector: &RelayConnector) -> Result<TransportEndpoint> {
        let policy = connector.policy();
        if policy.urls() != self.scope.urls()
            || policy.mode() != self.scope.mode()
            || policy.needs_tor()
        {
            return Err(ROUTE);
        }
        Ok(TransportEndpoint(policy.urls()[0].to_string()))
    }

    pub(super) async fn open(
        self,
        signing: BoundSigning,
        codec: Codec,
        connector: RelayConnector,
        deadline: Instant,
    ) -> Result<Bootstrap> {
        signing.check_context(&self.context, &codec)?;
        let endpoint = self.verify_scope(&connector)?;
        let authority = Authority::from_fence(
            self.fence.clone(),
            deadline.min(Instant::now() + BOOTSTRAP_TIMEOUT),
        )?;
        let keys = Arc::new(codec.inbound_alias());
        let mut lane = Lane::open_verified(connector, endpoint, keys, authority, 64 * 1024).await?;
        if let Err(error) = lane.activate_inbox().await {
            let _ = lane.close().await;
            return Err(error);
        }
        if self.context.direction() == CommandDirection::Server
            && let Err(error) = lane.adapter.authority.begin_listener(deadline)
        {
            lane.close().await?;
            return Err(error);
        }
        Ok(Bootstrap {
            carrier: self,
            exchange: Exchange::new(codec),
            signing,
            lane,
        })
    }
}

fn current() -> Result<(Time, Sample)> {
    let sample = Sample::read()?;
    Ok((
        Time {
            unix: sample.wall.as_secs(),
            monotonic: std::time::Instant::now(),
        },
        sample,
    ))
}

fn permitted(granted: &(impl Fn() -> bool + ?Sized)) -> Result<()> {
    if !granted() {
        return Err(GRANT);
    }
    Ok(())
}

impl Bootstrap {
    pub(super) async fn start_client(&mut self) -> Result<()> {
        if self.carrier.context.direction() != CommandDirection::Client {
            return Err(BINDING);
        }
        self.lane.adapter.authority.wait_live().await?;
        self.exchange.start_bound(&self.signing, current()?.0)?;
        self.lane.begin_active_bootstrap();
        self.send(Step::Probe).await
    }

    pub(super) async fn retry_probe(&mut self) -> Result<()> {
        self.send(Step::Probe).await
    }

    pub(super) async fn finish_client(&mut self) -> Result<()> {
        let expected = timeout_at(
            Instant::now() + CHALLENGE_TIMEOUT,
            self.next(|_, _, _| Err(ORDER)),
        )
        .await
        .map_err(|_| Error::Timeout)??;
        if expected != Advance::Send(Step::Confirm) {
            return Err(ORDER);
        }
        self.send(Step::Confirm).await?;
        if self.next(|_, _, _| Err(ORDER)).await? != Advance::Confirmed {
            return Err(ORDER);
        }
        Ok(())
    }

    pub(super) async fn serve(
        mut self,
        state: &mut InboundState,
        rooms: &Arc<Semaphore>,
        grant: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<(Self, RoomPermit)> {
        let mut permit = None;
        match self.serve_inner(state, rooms, &grant, &mut permit).await {
            Ok(()) => Ok((self, permit.ok_or(ORDER)?)),
            Err(error) => {
                self.close().await?;
                if let Some(permit) = permit {
                    permit.release_after_reap();
                }
                Err(error)
            }
        }
    }

    async fn serve_inner(
        &mut self,
        state: &mut InboundState,
        rooms: &Arc<Semaphore>,
        grant: &Arc<dyn Fn() -> bool + Send + Sync>,
        permit: &mut Option<RoomPermit>,
    ) -> Result<()> {
        if self.carrier.context.direction() != CommandDirection::Server {
            return Err(BINDING);
        }
        let first = self
            .next(|step, id, sample| {
                if step != Step::Probe {
                    return Err(ORDER);
                }
                state.clock.check(sample)?;
                state.consumed.charge(id, sample.tick)
            })
            .await?;
        if first != Advance::Send(Step::Challenge) {
            return Err(ORDER);
        }
        self.lane
            .adapter
            .authority
            .begin_flight(Instant::now() + BOOTSTRAP_TIMEOUT)?;
        self.lane.begin_active_bootstrap();
        self.send(Step::Challenge).await?;
        let authority = self.lane.adapter.authority.clone();
        let advance = self
            .next(|step, _, sample| {
                if step == Step::Probe {
                    return Ok(false);
                }
                state.clock.check(sample)?;
                permitted(grant.as_ref())?;
                authority.install_grant(grant.clone())?;
                match rooms.clone().try_acquire_owned() {
                    Ok(acquired) => {
                        *permit = Some(RoomPermit::new(acquired));
                        Ok(true)
                    }
                    Err(_) => Err(CAPACITY),
                }
            })
            .await;
        match advance {
            Ok(Advance::Send(Step::Ready)) if permit.is_some() => {
                permitted(grant.as_ref())?;
                self.send(Step::Ready).await
            }
            Err(error) => Err(error),
            _ => Err(ORDER),
        }
    }

    async fn send(&mut self, step: Step) -> Result<()> {
        let frame = &self.exchange.retry(step, current()?.0, true)?.frame;
        self.lane.publish_alias(frame).await
    }

    async fn next(
        &mut self,
        mut gate: impl FnMut(Step, [u8; 32], Sample) -> Result<bool>,
    ) -> Result<Advance> {
        loop {
            let delivery = self
                .lane
                .receive(TransportDeliveryPlane::AccountInbox, None, BOOTSTRAP_OUTER)
                .await?;
            self.lane.acknowledge(&delivery.message.id).await;
            self.lane.adapter.authority.wait_live().await?;
            let (now, sample) = current()?;
            let mut denied = None;
            let advanced = self.exchange.receive_bound(
                &self.signing,
                &delivery.message.payload,
                now,
                |step, id| match gate(step, id, sample) {
                    Ok(admitted) => Ok(admitted),
                    Err(error) => {
                        denied = Some(error);
                        Ok(false)
                    }
                },
            );
            if let Some(error) = denied {
                if matches!(
                    error,
                    Error::Invalid("terminal_enrollment_grant" | "terminal_enrollment_capacity")
                ) {
                    self.lane.adapter.authority.retire();
                    return Err(error);
                }
                continue;
            }
            if let Ok(advance) = advanced
                && advance != Advance::Duplicate
            {
                return Ok(advance);
            }
        }
    }

    pub(super) async fn enroll(
        self,
        connector: RelayConnector,
        permit: RoomPermit,
        client_grant: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    ) -> Result<GrantedEnrollment> {
        let checked = async {
            self.carrier.verify_scope(&connector)?;
            match (self.carrier.context.direction(), client_grant) {
                (CommandDirection::Client, Some(grant)) => {
                    self.lane.adapter.authority.install_grant(grant)?;
                }
                (CommandDirection::Server, None) => {}
                _ => return Err(BINDING),
            }
            self.lane.adapter.authority.wait_live().await
        };
        if let Err(error) = checked.await {
            self.close().await?;
            permit.release_after_reap();
            return Err(error);
        }
        let opened = Owner::open_bound(self.exchange, self.signing, self.lane, connector).await;
        let owner = match opened {
            Ok(owner) => owner,
            Err(failure) => {
                if failure.reaped {
                    permit.release_after_reap();
                }
                return Err(failure.error);
            }
        };
        Ok(GrantedEnrollment {
            carrier: self.carrier,
            reserved: ReservedRoom {
                owner: Some((owner, permit)),
            },
        })
    }

    pub(super) async fn close(self) -> Result<()> {
        self.lane.close().await
    }
}

#[test]
fn room_capacity_returns_only_after_a_confirmed_reap() {
    let rooms = Arc::new(Semaphore::new(1));
    let permit = RoomPermit::new(rooms.clone().try_acquire_owned().unwrap());
    assert_eq!(rooms.available_permits(), 0);
    permit.release_after_reap();
    assert_eq!(rooms.available_permits(), 1);
    drop(RoomPermit::new(rooms.clone().try_acquire_owned().unwrap()));
    assert_eq!(rooms.available_permits(), 0);
}
