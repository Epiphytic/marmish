//! Single direct-WSS Terminal admission on a verified Command binding.
use super::terminal_enrollment::Owner;
use super::terminal_enrollment_room::{App, RoomWitness};
use super::terminal_enrollment_transport::{Authority, Lane, Read};
pub(crate) use super::terminal_enrollment_transport::{EnrollmentCeiling, Reapers};
use super::{AuthenticatedCommandContext, CommandDirection, TransportEndpoint};
use crate::terminal_backend::supervisor::live::selection::AttachmentGuard;
use crate::terminal_bootstrap::state::{Advance, Exchange, Time};
use crate::terminal_bootstrap::{BoundSigning, Codec, Step};
use crate::terminal_fence::{AttachmentFence, FenceState};
use crate::terminal_frames;
use crate::transport::{RelayConnector, RelayPolicy};
use crate::{Error, Result};
#[cfg(test)]
use std::num::NonZeroU64;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::Instant;

#[path = "terminal_live_enrollment_clock.rs"]
mod clock;

use clock::Sample;
pub(crate) use clock::{AdmissionLedger, InboundState};

const BINDING: Error = Error::Invalid("terminal_bootstrap_binding");
const ROUTE: Error = Error::Transport("terminal_transport_unavailable");
const GRANT: Error = Error::Invalid("terminal_enrollment_grant");
const CAPACITY: Error = Error::Invalid("terminal_enrollment_capacity");
const ORDER: Error = Error::Invalid("terminal_enrollment_order");
const BOOTSTRAP_OUTER: usize = 32 * 1024;
const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(30);
const PROBE_RETRIES: [Duration; 4] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(18),
];
const NO_CHALLENGE: Error = Error::Transport("terminal_bootstrap_no_challenge");
pub(crate) const RELAY_AUTH_CHALLENGE_TIMEOUT: &str = "terminal_relay_auth_challenge_timeout";

pub(super) struct CarrierScope {
    pub(super) reapers: Arc<Reapers>,
    pub(super) grant: Arc<dyn Fn() -> bool + Send + Sync>,
    pub(super) local_closed: Option<Arc<AtomicBool>>,
}

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

pub(super) struct Bootstrap {
    carrier: Carrier,
    exchange: Exchange,
    signing: BoundSigning,
    lane: Lane,
    client_start: Option<EnrollmentCeiling>,
    probe_started: Option<Instant>,
}

pub(super) struct GrantedEnrollment {
    carrier: Carrier,
    reserved: ReservedRoom,
}

pub(super) struct RoomPermit(Arc<RoomLease>);

struct RoomLease {
    permit: Option<OwnedSemaphorePermit>,
    room_reaped: AtomicBool,
}

impl RoomPermit {
    pub(super) fn new(permit: OwnedSemaphorePermit) -> Self {
        Self(Arc::new(RoomLease {
            permit: Some(permit),
            room_reaped: AtomicBool::new(false),
        }))
    }

    fn release_after_reap(self) {
        self.0.room_reaped.store(true, Ordering::Release);
    }
}

impl Drop for RoomLease {
    fn drop(&mut self) {
        // An abandoned, failed or cancelled transition must not create
        // capacity while its source or room may still be running.
        if !self.room_reaped.load(Ordering::Acquire)
            && let Some(permit) = self.permit.take()
        {
            permit.forget();
        }
    }
}

pub(super) fn finish_failed_enrollment(
    original: Error,
    close: Result<()>,
    permit: Option<RoomPermit>,
    stage: &'static str,
) -> Error {
    if close.is_err() {
        crate::diagnostics::event(stage, "cleanup_failed");
    } else if let Some(permit) = permit {
        permit.release_after_reap();
    }
    original
}

struct ReservedRoom {
    owner: Option<(Owner, RoomPermit)>,
}

impl ReservedRoom {
    fn start_reap(&mut self) -> Option<JoinHandle<Result<()>>> {
        let runtime = tokio::runtime::Handle::try_current().ok()?;
        let (owner, permit) = self.owner.take()?;
        owner.alias_adapter.authority.retire();
        let cleanup = owner.alias_adapter.authority.reap();
        Some(runtime.spawn(async move {
            let result = owner.close().await;
            if let Some(cleanup) = cleanup {
                cleanup.completed(&result);
            }
            if result.is_ok() {
                permit.release_after_reap();
            }
            result
        }))
    }

    async fn close(mut self) -> Result<()> {
        self.start_reap().ok_or(ORDER)?.await.map_err(|_| ORDER)?
    }

    fn owner(&self) -> &Owner {
        &self.owner.as_ref().expect("owned native Terminal room").0
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
pub(crate) struct SelectionBinding {
    pub(crate) transcript: [u8; 32],
    pub(crate) command_digest: [u8; 32],
    pub(crate) generation: u64,
    pub(crate) local: [u8; 32],
    pub(crate) peer: [u8; 32],
    pub(crate) local_terminal: [u8; 32],
    pub(crate) peer_terminal: [u8; 32],
    pub(crate) direction: CommandDirection,
    pub(crate) uid: u32,
    pub(crate) native: RoomWitness,
}

pub(crate) struct ReadyEnrollment {
    enrollment: GrantedEnrollment,
    binding: SelectionBinding,
    ceiling: EnrollmentCeiling,
}

pub(crate) struct AttachedRoom(Box<AttachedRoomState>);

struct AttachedRoomState {
    reserved: ReservedRoom,
    binding: SelectionBinding,
    frames: terminal_frames::Binding,
    ceiling: EnrollmentCeiling,
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
        let (local_terminal, peer_terminal) = owner.room.terminal_accounts()?;
        permitted(granted)?;
        let context = &self.carrier.context;
        Ok(SelectionBinding {
            transcript,
            command_digest: context.authority_digest(),
            generation: self.carrier.fence.generation().get(),
            local: context.local_account(),
            peer: context.peer_account(),
            local_terminal,
            peer_terminal,
            direction: context.direction(),
            uid: context.target_uid(),
            native,
        })
    }

    pub(super) async fn ready(mut self, granted: &impl Fn() -> bool) -> Result<ReadyEnrollment> {
        let ceiling = self.reserved.owner().admission_ceiling()?;
        ceiling.check()?;
        let binding = self.selection_binding(granted).await?;
        ceiling.check()?;
        Ok(ReadyEnrollment {
            enrollment: self,
            binding,
            ceiling,
        })
    }
}

impl ReadyEnrollment {
    pub(crate) fn binding(&self) -> &SelectionBinding {
        &self.binding
    }

    pub(crate) fn fence(&self) -> Arc<AttachmentFence> {
        self.enrollment.carrier.fence.clone()
    }

    pub(crate) fn admission_ceiling(&self) -> EnrollmentCeiling {
        self.ceiling
    }

    pub(crate) fn authority(&self) -> Arc<Authority> {
        self.enrollment
            .reserved
            .owner()
            .alias_adapter
            .authority
            .clone()
    }

    pub(crate) async fn recheck(&self, granted: &impl Fn() -> bool) -> Result<()> {
        let result = self.recheck_inner(granted, true).await;
        if result.is_err() {
            self.authority().retire();
        }
        result
    }

    async fn recheck_inner(&self, granted: &impl Fn() -> bool, check_alias: bool) -> Result<()> {
        self.ceiling.check()?;
        permitted(granted)?;
        let owner = self.enrollment.reserved.owner();
        if check_alias {
            owner.alias_adapter.check().await?;
        }
        if owner.ready().await? != self.binding.transcript
            || owner.room.witness()? != self.binding.native
            || !self.fence().matches_digest(&self.binding.command_digest)
            || self.fence().generation().get() != self.binding.generation
        {
            return Err(BINDING);
        }
        owner.alias_adapter.authority.check()?;
        self.ceiling.check()?;
        permitted(granted)
    }

    pub(crate) async fn send_selection(
        &mut self,
        bytes: Vec<u8>,
        granted: &impl Fn() -> bool,
    ) -> Result<()> {
        self.recheck(granted).await?;
        let result = self.enrollment.owner_mut().room.send_app(bytes).await;
        if result.is_err() {
            self.authority().retire();
        }
        result?;
        self.recheck(granted).await
    }

    pub(crate) async fn receive_selection(&mut self, granted: &impl Fn() -> bool) -> Result<App> {
        self.read_selection(granted, true).await?.ok_or(ORDER)
    }

    pub(crate) async fn try_receive_selection(
        &mut self,
        granted: &impl Fn() -> bool,
    ) -> Result<Option<App>> {
        self.read_selection(granted, false).await
    }

    async fn read_selection(
        &mut self,
        granted: &impl Fn() -> bool,
        wait: bool,
    ) -> Result<Option<App>> {
        self.recheck(granted).await?;
        let room = &mut self.enrollment.owner_mut().room;
        let result = if wait {
            room.receive_app().await.map(Some)
        } else {
            room.receive_queued_app().await
        };
        if result.is_err() {
            self.retire();
        }
        let app = result?;
        if app
            .as_ref()
            .is_some_and(|app| app.sender_leaf_index != self.binding.native.peer_leaf)
        {
            self.retire();
            return Err(BINDING);
        }
        self.recheck(granted).await?;
        Ok(app)
    }

    pub(crate) fn retire(&self) {
        self.authority().retire();
    }

    pub(crate) fn room_hold(&self) -> Arc<dyn Send + Sync> {
        let (_, permit) = self
            .enrollment
            .reserved
            .owner
            .as_ref()
            .expect("owned native Terminal room");
        permit.0.clone()
    }

    pub(crate) async fn close(self) -> Result<()> {
        self.enrollment.close().await
    }

    pub(crate) async fn handoff_client(
        self,
        binding: crate::terminal_selection_wire_v2::Binding,
        ceiling: EnrollmentCeiling,
        granted: &impl Fn() -> bool,
    ) -> Result<AttachedRoom> {
        if self.binding.direction != CommandDirection::Client {
            return Err(BINDING);
        }
        self.begin_attachment(binding, ceiling, granted, || true)
            .await
    }

    pub(crate) async fn handoff_server(
        self,
        binding: crate::terminal_selection_wire_v2::Binding,
        ceiling: EnrollmentCeiling,
        granted: &impl Fn() -> bool,
        guard: AttachmentGuard,
    ) -> Result<(AttachedRoom, AttachmentGuard)> {
        if self.binding.direction != CommandDirection::Server {
            return Err(BINDING);
        }
        let room = self
            .begin_attachment(binding, ceiling, granted, || {
                guard.matches_binding(binding.claim, binding.generation)
            })
            .await?;
        Ok((room, guard))
    }

    async fn begin_attachment(
        mut self,
        binding: crate::terminal_selection_wire_v2::Binding,
        ceiling: EnrollmentCeiling,
        granted: &impl Fn() -> bool,
        owns_attachment: impl Fn() -> bool,
    ) -> Result<AttachedRoom> {
        if binding.claim == [0; 16] || !owns_attachment() {
            return Err(BINDING);
        }
        ceiling.check()?;
        self.recheck(granted).await?;
        self.enrollment.owner_mut().detach_alias().await?;
        self.recheck_inner(granted, false).await?;
        if !owns_attachment() {
            return Err(BINDING);
        }
        let frames = terminal_frames::Binding {
            transcript: self.binding.transcript,
            attachment: binding.claim,
            generation: binding.generation.get(),
            role: match binding.role {
                crate::terminal_ipc_wire::Role::Writer => terminal_frames::Role::Writer,
                crate::terminal_ipc_wire::Role::Observer => terminal_frames::Role::Observer,
            },
        };
        self.authority().begin_attachment_at(frames, ceiling)?;
        Ok(AttachedRoom(Box::new(AttachedRoomState {
            reserved: self.enrollment.reserved,
            binding: self.binding,
            frames,
            ceiling,
        })))
    }

    #[cfg(test)]
    pub(super) async fn attach_enrollment_proof(
        self,
        expected: &SelectionBinding,
        attachment: [u8; 16],
        generation: NonZeroU64,
        ceiling: EnrollmentCeiling,
        granted: &impl Fn() -> bool,
    ) -> Result<AttachedRoom> {
        self.check_proof_binding(expected, attachment, ceiling)?;
        self.begin_attachment(
            crate::terminal_selection_wire_v2::Binding {
                claim: attachment,
                generation,
                role: crate::terminal_ipc_wire::Role::Writer,
            },
            ceiling,
            granted,
            || true,
        )
        .await
    }

    #[cfg(test)]
    pub(super) fn check_proof_binding(
        &self,
        expected: &SelectionBinding,
        attachment: [u8; 16],
        ceiling: EnrollmentCeiling,
    ) -> Result<()> {
        if expected != &self.binding || attachment == [0; 16] {
            return Err(BINDING);
        }
        ceiling.check()
    }
}

impl AttachedRoom {
    pub(crate) fn binding(&self) -> &SelectionBinding {
        &self.0.binding
    }

    pub(crate) fn frame_binding(&self) -> terminal_frames::Binding {
        self.0.frames
    }

    pub(crate) async fn check_live(&self) -> Result<()> {
        let owner = self.0.reserved.owner();
        let result = async {
            self.0.ceiling.check()?;
            owner
                .alias_adapter
                .authority
                .permits_frames(self.0.frames)?;
            if owner.ready().await? != self.0.binding.transcript
                || owner.room.witness()? != self.0.binding.native
            {
                return Err(BINDING);
            }
            self.0.ceiling.check()
        }
        .await;
        if result.is_err() {
            owner.alias_adapter.authority.retire();
        }
        result
    }

    pub(crate) async fn send_frame(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.check_live().await?;
        let result = self
            .0
            .reserved
            .owner_mut()
            .send_frame(self.0.frames, bytes)
            .await;
        if result.is_err() {
            self.0.reserved.owner().alias_adapter.authority.retire();
        }
        result?;
        self.check_live().await
    }

    #[cfg(test)]
    pub(crate) async fn receive_frame(&mut self) -> Result<App> {
        self.check_live().await?;
        let result = self
            .0
            .reserved
            .owner_mut()
            .receive_frame(self.0.frames)
            .await;
        if result.is_err() {
            self.0.reserved.owner().alias_adapter.authority.retire();
        }
        let app = result?;
        if app.sender_leaf_index != self.0.binding.native.peer_leaf {
            self.0.reserved.owner().alias_adapter.authority.retire();
            return Err(BINDING);
        }
        self.check_live().await?;
        Ok(app)
    }

    pub(crate) async fn try_receive_frame(
        &mut self,
    ) -> Result<Option<(terminal_frames::Admitted, Duration)>> {
        self.check_live().await?;
        let result = async {
            let Some(app) = self
                .0
                .reserved
                .owner_mut()
                .room
                .receive_queued_app()
                .await?
            else {
                return Ok(None);
            };
            let clock = Sample::read()?;
            if app.sender_leaf_index != self.0.binding.native.peer_leaf {
                return Err(BINDING);
            }
            let direction = match self.0.binding.direction {
                CommandDirection::Client => terminal_frames::Direction::FromServer,
                CommandDirection::Server => terminal_frames::Direction::ToServer,
            };
            let arrival = terminal_frames::Arrival {
                retention: Some(app.retention),
                wall: Some(clock.wall.as_secs()),
                monotonic: Some(clock.tick),
            };
            let received = terminal_frames::decode_app(
                &app.bytes,
                &self.0.binding.peer_terminal,
                self.0.frames,
                direction,
                arrival,
            )
            .ok_or(ORDER)?;
            self.check_live().await?;
            Ok(Some((received, clock.tick)))
        }
        .await;
        if result.is_err() {
            self.0.reserved.owner().alias_adapter.authority.retire();
        }
        result
    }

    pub(crate) fn ceiling(&self) -> EnrollmentCeiling {
        self.0.ceiling
    }

    pub(crate) async fn close(self) -> Result<()> {
        self.0.reserved.close().await
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

    #[cfg(test)]
    pub(super) async fn open(
        self,
        signing: BoundSigning,
        codec: Codec,
        connector: RelayConnector,
        deadline: Instant,
    ) -> Result<Bootstrap> {
        self.open_inner(signing, codec, connector, deadline, None)
            .await
    }

    pub(super) async fn open_inner(
        self,
        signing: BoundSigning,
        codec: Codec,
        connector: RelayConnector,
        deadline: Instant,
        scope: Option<CarrierScope>,
    ) -> Result<Bootstrap> {
        signing.check_context(&self.context, &codec)?;
        let endpoint = self.verify_scope(&connector)?;
        let authority = Authority::from_fence(
            self.fence.clone(),
            deadline.min(Instant::now() + BOOTSTRAP_TIMEOUT),
            scope.as_ref().map(|scope| scope.reapers.clone()),
            scope.as_ref().and_then(|scope| scope.local_closed.clone()),
        )?;
        if let Some(scope) = scope {
            authority.install_grant(scope.grant)?;
        }
        let keys = Arc::new(codec.inbound_alias());
        let mut lane = Lane::open_verified(connector, endpoint, keys, authority, 64 * 1024).await?;
        if let Err(error) = lane.activate_inbox().await {
            return Err(finish_failed_enrollment(
                error,
                lane.close().await,
                None,
                "terminal_inbox_close",
            ));
        }
        if self.context.direction() == CommandDirection::Server
            && let Err(error) = lane.adapter.authority.begin_listener(deadline)
        {
            return Err(finish_failed_enrollment(
                error,
                lane.close().await,
                None,
                "terminal_listener_close",
            ));
        }
        Ok(Bootstrap {
            carrier: self,
            exchange: Exchange::new(codec),
            signing,
            lane,
            client_start: None,
            probe_started: None,
        })
    }
}

fn current() -> Result<(Time, Sample)> {
    let sample = Sample::read()?;
    Ok((
        Time {
            unix: sample.wall.as_secs(),
            monotonic: std::time::Instant::now(),
            continuous: Some(sample.tick),
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
    pub(crate) fn constrain_client_start(&mut self, ceiling: EnrollmentCeiling) -> Result<()> {
        if self.carrier.context.direction() != CommandDirection::Client {
            return Err(BINDING);
        }
        let ceiling = self
            .client_start
            .map_or(ceiling, |previous| previous.clamp(ceiling));
        self.lane.adapter.authority.constrain_bootstrap(ceiling)?;
        self.client_start = Some(ceiling);
        Ok(())
    }

    pub(super) async fn start_client(&mut self) -> Result<()> {
        if self.carrier.context.direction() != CommandDirection::Client {
            return Err(BINDING);
        }
        self.lane.adapter.authority.wait_live().await?;
        self.exchange.start_bound(&self.signing, current()?.0)?;
        self.lane.begin_active_bootstrap();
        self.probe_started = Some(Instant::now());
        self.send(Step::Probe).await
    }

    pub(super) async fn retry_probe(&mut self) -> Result<()> {
        self.lane.adapter.authority.wait_live().await?;
        let frame = self
            .exchange
            .rewrap_probe_bound(&self.signing, current()?.0)?;
        self.lane.publish_alias(&frame).await
    }

    pub(super) async fn finish_client(&mut self) -> Result<()> {
        let began = self.probe_started.take().ok_or(ORDER)?;
        let mut retries = PROBE_RETRIES.into_iter().peekable();
        let expected = loop {
            let retry = retries.peek().map(|delay| began + *delay);
            match self.next_until(retry, |_, _, _| Err(ORDER)).await {
                Ok(Some(advance)) => break advance,
                Ok(None) if retries.next().is_some() => {
                    // A native placement or publication can pass more than
                    // one cadence point. Never replay a burst of missed sends.
                    self.retry_probe().await?;
                    while retries
                        .peek()
                        .is_some_and(|delay| began + *delay <= Instant::now())
                    {
                        retries.next();
                    }
                }
                Ok(None) => return Err(ORDER),
                Err(Error::Timeout) => return Err(NO_CHALLENGE),
                Err(error) => return Err(error),
            }
        };
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
            Err(error) => Err(finish_failed_enrollment(
                error,
                self.close().await,
                permit,
                "terminal_inbound_close",
            )),
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
        self.lane.discard_stored_alias_inbox()?;
        let first = self
            .next(|step, id, sample| {
                if step != Step::Probe {
                    return Err(ORDER);
                }
                permitted(grant.as_ref())?;
                state.charge(id, sample)
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
                state.check(sample)?;
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
        gate: impl FnMut(Step, [u8; 32], Sample) -> Result<bool>,
    ) -> Result<Advance> {
        self.next_until(None, gate).await?.ok_or(ORDER)
    }

    async fn next_until(
        &mut self,
        retry: Option<Instant>,
        mut gate: impl FnMut(Step, [u8; 32], Sample) -> Result<bool>,
    ) -> Result<Option<Advance>> {
        loop {
            let delivery = match self
                .lane
                .receive_inbox_until(BOOTSTRAP_OUTER, retry)
                .await?
            {
                Read::Delivery(delivery) => delivery,
                Read::Tick => return Ok(None),
                Read::Empty => return Err(ORDER),
            };
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
                return Ok(Some(advance));
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
            return Err(finish_failed_enrollment(
                error,
                self.close().await,
                Some(permit),
                "terminal_enroll_close",
            ));
        }
        let opened = Owner::open_bound(
            self.exchange,
            self.signing,
            self.lane,
            connector,
            self.client_start,
        )
        .await;
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

#[test]
fn terminal_failed_enrollment_retains_unsafe_room_capacity() {
    let rooms = Arc::new(Semaphore::new(1));
    let permit = RoomPermit::new(rooms.clone().try_acquire_owned().unwrap());
    assert!(matches!(
        finish_failed_enrollment(Error::Timeout, Ok(()), Some(permit), "fixture_close"),
        Error::Timeout
    ));
    assert_eq!(rooms.available_permits(), 1);

    let permit = RoomPermit::new(rooms.clone().try_acquire_owned().unwrap());
    assert!(matches!(
        finish_failed_enrollment(
            ROUTE,
            Err(Error::Invalid("fixture_close")),
            Some(permit),
            "fixture_close",
        ),
        Error::Transport("terminal_transport_unavailable")
    ));
    assert_eq!(rooms.available_permits(), 0);
}

#[test]
fn terminal_runtime_core_cleanup_failure_keeps_client_refusal() {
    let error = finish_failed_enrollment(
        Error::Invalid("terminal_enrollment_grant"),
        Err(Error::Transport("fixture_close")),
        None,
        "fixture_close",
    );
    assert_eq!(
        crate::terminal_owner::category(&error),
        "terminal_grant_denied"
    );
}

#[test]
fn terminal_room_capacity_waits_for_native_replacement_hold() {
    let rooms = Arc::new(Semaphore::new(1));
    let permit = RoomPermit::new(rooms.clone().try_acquire_owned().unwrap());
    let hold = Arc::clone(&permit.0);
    permit.release_after_reap();
    assert_eq!(rooms.available_permits(), 0);
    drop(hold);
    assert_eq!(rooms.available_permits(), 1);

    let permit = RoomPermit::new(rooms.clone().try_acquire_owned().unwrap());
    let hold = Arc::clone(&permit.0);
    drop(permit);
    drop(hold);
    assert_eq!(rooms.available_permits(), 0);
}
