//! Thread ownership for proof-gated native Terminal attachments.
#[path = "terminal_live_supervisor_confirm.rs"]
mod confirm;
#[path = "terminal_live_supervisor_guard.rs"]
mod guard;
#[path = "terminal_live_supervisor_io.rs"]
mod io;
#[path = "terminal_selection.rs"]
pub(crate) mod selection;
#[cfg(test)]
#[path = "terminal_selection_wire.rs"]
pub(crate) mod selection_wire;
#[path = "terminal_live_slot_policy.rs"]
mod slot_policy;
#[path = "terminal_live_supervisor_slots.rs"]
mod slots;
#[path = "terminal_live_stream.rs"]
pub(crate) mod stream;
#[path = "terminal_live_takeover.rs"]
mod takeover;
#[cfg(test)]
#[path = "terminal_live_supervisor_tests.rs"]
mod tests;

#[cfg(test)]
use super::IdentityMaterial;
use super::{
    Account, CAPACITY, CLEANUP, CheckedLaunch, DENIED, Error, Instant, MISSING, Manager,
    ProfileLease, REQUEST, Registry, Result, Role, Shutdown, Slot, SlotId, State, UNAVAILABLE,
    backend, random, recovery, tmux,
};
use crate::command_config::CommandConfig;
use crate::terminal_offers::{Ledger, Scope};
use crate::terminal_selection_wire_v2::{Request, Ticket};
use confirm::{Confirmation, StartLimit, StartPermit};
use guard::Source;
use io::Event;
use io::{Installing, Native, Operation, Port, PushError, Wake};
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;
use tokio::sync::oneshot;

const MAX_LIVE: usize = 8;
const CONFIRM_BUDGET: Duration = Duration::from_secs(5);
const START_BUDGET: Duration = Duration::from_secs(10);
const RECONCILE_TURN: Duration = Duration::from_secs(2);
const HEALTH: Duration = Duration::from_millis(10);
const REAP_TURN: Duration = Duration::from_millis(2);
const STOPPED: Error = Error::Invalid("terminal_owner_unavailable");
const WRITER_MISSING: Error = Error::Invalid("terminal_writer_missing");
const STALE_OFFER: Error = Error::Invalid("terminal_offer_stale");
const REPLACEMENT_PENDING: Error = Error::Invalid("terminal_replacement_pending");
const DEADLINE: Error = Error::Invalid("terminal_attach_deadline");

use super::Claim;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Reason {
    OwnerLost = 1,
    Authority,
    Deadline,
    Clock,
    Idle,
    Budget,
    Detached,
    ShellExit,
    Io,
    Replaced,
}

impl Reason {
    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::OwnerLost,
            2 => Self::Authority,
            3 => Self::Deadline,
            4 => Self::Clock,
            5 => Self::Idle,
            6 => Self::Budget,
            7 => Self::Detached,
            8 => Self::ShellExit,
            9 => Self::Io,
            10 => Self::Replaced,
            _ => return None,
        })
    }

    fn start_error(self) -> Error {
        match self {
            Self::Deadline => DEADLINE,
            Self::Authority => DENIED,
            _ => STOPPED,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GuardResult<T> {
    Applied(T),
    Withheld,
    Retired(Reason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SourceId {
    leaf: [u8; 32],
    epoch: u64,
    subscription: NonZeroU64,
}

#[derive(Clone, Copy)]
enum StartChoice {
    Checked {
        scope: Scope,
        request: Request,
    },
    #[cfg(test)]
    Default,
    #[cfg(test)]
    New,
}

struct VerifiedStart {
    account: Account,
    selection: StartChoice,
    rows: u16,
    cols: u16,
    source: Arc<Source>,
    deadline: Duration,
    room_hold: Option<Arc<dyn Send + Sync>>,
}

enum StartOutcome {
    Attached(AdmittedTerminal),
    Busy {
        slot: SlotId,
        writer: Claim,
        offer: Option<Ticket>,
    },
    Refused(Error),
}

struct AdmittedTerminal {
    slot: SlotId,
    claim: Claim,
    port: Arc<Port>,
    role: Role,
    confirmation: Option<Arc<Confirmation>>,
    confirmation_sent: bool,
    #[cfg(test)]
    closed_reported: bool,
}

impl AdmittedTerminal {
    fn submit(&self, operation: Operation) -> std::result::Result<(), PushError> {
        if !self.confirmed() {
            return Err(PushError::Closed(Reason::Authority));
        }
        self.port.push(operation)
    }

    fn event(&self, allow_output: bool) -> Option<Event> {
        self.confirmed()
            .then(|| self.port.event(allow_output))
            .flatten()
    }

    #[cfg(test)]
    fn try_event(&mut self) -> Option<Event> {
        if self.closed_reported {
            return None;
        }
        let event = self.event(true)?;
        self.closed_reported = matches!(event, Event::Closed(_));
        Some(event)
    }

    #[cfg(test)]
    async fn changed(&self) {
        self.port.changed().await;
    }

    fn role(&self) -> Role {
        self.role
    }

    fn confirmed(&self) -> bool {
        self.confirmation
            .as_ref()
            .is_none_or(|claim| claim.active())
    }

    fn submit_confirm(
        &mut self,
        control: &LiveControl,
        confirm_until: Duration,
    ) -> std::result::Result<oneshot::Receiver<Result<()>>, QueueError> {
        if self.confirmation.is_none() || self.confirmation_sent {
            return Err(QueueError::Stopped);
        }
        self.confirmation_sent = true;
        let (reply, received) = oneshot::channel();
        control.submit_confirm(confirm::Request {
            slot: self.slot,
            claim: self.claim,
            role: self.role,
            source: Arc::clone(&self.port.source),
            until: confirm_until,
            reply,
        })?;
        Ok(received)
    }

    fn renew(&self, source: SourceId, accepted: Duration) -> GuardResult<bool> {
        if !self.confirmed() {
            return GuardResult::Withheld;
        }
        self.port
            .source
            .renew(source, self.claim, accepted, guard::sample)
    }

    fn close(&self) {
        self.port.close(Reason::Detached);
    }
}

impl Drop for AdmittedTerminal {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Debug, PartialEq, Eq)]
enum QueueError {
    Full,
    Stopped,
}

type Start = (VerifiedStart, oneshot::Sender<StartOutcome>, StartPermit);
type Release = (SlotId, oneshot::Sender<Result<()>>);

#[derive(Clone)]
pub(crate) struct LiveControl {
    start: mpsc::SyncSender<Start>,
    confirm: mpsc::SyncSender<confirm::Request>,
    start_limit: Arc<StartLimit>,
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "explicit slot release is only submitted by native proof today"
        )
    )]
    release: mpsc::SyncSender<Release>,
    owner: Thread,
    stopping: Arc<AtomicBool>,
    submission: Arc<Mutex<()>>,
}

impl LiveControl {
    fn submit_start(
        &self,
        start: VerifiedStart,
        reply: oneshot::Sender<StartOutcome>,
    ) -> std::result::Result<(), QueueError> {
        let permit = self.start_limit.try_acquire().ok_or_else(|| {
            if self.stopping.load(Ordering::Acquire) {
                QueueError::Stopped
            } else {
                QueueError::Full
            }
        })?;
        self.submit(&self.start, (start, reply, permit))
    }

    fn submit_confirm(&self, request: confirm::Request) -> std::result::Result<(), QueueError> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(QueueError::Stopped);
        }
        match self.confirm.try_send(request) {
            Ok(()) => {
                self.owner.unpark();
                Ok(())
            }
            Err(mpsc::TrySendError::Full(request)) => {
                request.source.stop(Reason::OwnerLost);
                self.stopping.store(true, Ordering::Release);
                self.owner.unpark();
                Err(QueueError::Full)
            }
            Err(mpsc::TrySendError::Disconnected(_)) => Err(QueueError::Stopped),
        }
    }

    #[cfg(test)]
    fn release(
        &self,
        slot: SlotId,
        reply: oneshot::Sender<Result<()>>,
    ) -> std::result::Result<(), QueueError> {
        self.submit(&self.release, (slot, reply))
    }

    fn submit<T>(
        &self,
        channel: &mpsc::SyncSender<T>,
        request: T,
    ) -> std::result::Result<(), QueueError> {
        let _submission = self.submission.try_lock().map_err(|_| {
            if self.stopping.load(Ordering::Acquire) {
                QueueError::Stopped
            } else {
                QueueError::Full
            }
        })?;
        if self.stopping.load(Ordering::Acquire) {
            return Err(QueueError::Stopped);
        }
        channel.try_send(request).map_err(|error| match error {
            mpsc::TrySendError::Full(_) => QueueError::Full,
            mpsc::TrySendError::Disconnected(_) => QueueError::Stopped,
        })?;
        self.owner.unpark();
        Ok(())
    }
}

pub(crate) struct LiveSupervisor {
    pub(crate) control: LiveControl,
    wake: Wake,
    control_worker: Option<JoinHandle<()>>,
    io_worker: Option<JoinHandle<Result<()>>>,
}

impl LiveSupervisor {
    #[cfg(test)]
    fn start(
        profile: Arc<ProfileLease>,
        material: &IdentityMaterial,
        daemon: &Path,
        tmux: &Path,
    ) -> Result<Self> {
        let config = CommandConfig::load(profile.root())?;
        let registry = Registry::open(&profile, material)?;
        Self::start_prepared(profile, config, registry, daemon, tmux, Arc::default())
    }

    pub(crate) fn start_prepared(
        profile: Arc<ProfileLease>,
        config: CommandConfig,
        registry: Registry,
        daemon: &Path,
        tmux: &Path,
        stopping: Arc<AtomicBool>,
    ) -> Result<Self> {
        backend::paths::checked_uid()?;
        backend::paths::daemon(daemon)?;
        if stopping.load(Ordering::Acquire) {
            return Err(STOPPED);
        }
        let mut manager = Manager::new(tmux, profile.root())?;
        manager.profile = Some(Arc::clone(&profile));
        let (wake, reader) = Wake::open()?;
        let submission = Arc::new(Mutex::new(()));
        let mut owner = ControlOwner {
            config,
            registry,
            manager,
            daemon: daemon.to_owned(),
            active: HashMap::with_capacity(MAX_LIVE),
            offers: Ledger::default(),
            parked: VecDeque::new(),
            departure: VecDeque::new(),
            quarantine: HashSet::new(),
            wake: wake.clone(),
            stopping: Arc::clone(&stopping),
            profile,
        };
        owner.reconcile()?;
        if stopping.load(Ordering::Acquire) {
            return Err(STOPPED);
        }
        let (install, installs) = mpsc::sync_channel(1);
        let (returned, reap) = mpsc::sync_channel(MAX_LIVE);
        let (start, starts) = mpsc::sync_channel(MAX_LIVE);
        let (confirm, confirms) = mpsc::sync_channel(MAX_LIVE);
        let (release, releases) = mpsc::sync_channel(2);
        let guard = StopGuard(Arc::clone(&stopping), wake.clone());
        let io_worker = thread::Builder::new()
            .name("terminal-native-io".to_owned())
            .spawn(move || io::run(reader, installs, returned, &guard.0))?;
        let guard = StopGuard(Arc::clone(&stopping), wake.clone());
        let close_submission = Arc::clone(&submission);
        let control_worker = thread::Builder::new()
            .name("terminal-native-control".to_owned())
            .spawn(move || {
                owner.run(starts, confirms, releases, install, reap, &close_submission);
                drop(guard);
            });
        let control_worker = match control_worker {
            Ok(worker) => worker,
            Err(error) => {
                stopping.store(true, Ordering::Release);
                wake.notify();
                let _ = io_worker.join();
                return Err(error.into());
            }
        };
        let control = LiveControl {
            start,
            confirm,
            start_limit: Arc::default(),
            release,
            owner: control_worker.thread().clone(),
            stopping,
            submission,
        };
        Ok(Self {
            control,
            wake,
            control_worker: Some(control_worker),
            io_worker: Some(io_worker),
        })
    }

    pub(crate) fn stop(&self) {
        self.control.stopping.store(true, Ordering::Release);
        self.wake.notify();
        self.control.owner.unpark();
    }

    pub(crate) fn join_if_finished(&mut self) -> Option<Result<()>> {
        if !self.control_worker.as_ref()?.is_finished() || !self.io_worker.as_ref()?.is_finished() {
            return None;
        }
        let control = self.control_worker.take()?.join();
        let io = self.io_worker.take()?.join();
        Some(if control.is_ok() && matches!(io, Ok(Ok(()))) {
            Ok(())
        } else {
            Err(STOPPED)
        })
    }
}

impl Drop for LiveSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

struct StopGuard(Arc<AtomicBool>, Wake);

impl Drop for StopGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
        self.1.notify();
    }
}

fn valid_size(rows: u16, cols: u16) -> bool {
    (2..=250).contains(&rows) && (2..=500).contains(&cols)
}

struct Active {
    account: Account,
    inventory: tmux::Inventory,
    writer: Option<Claim>,
    claim: Claim,
    role: Role,
    confirmation: Option<Arc<Confirmation>>,
    pending_start: Option<StartPermit>,
    port: Arc<Port>,
    retiring: Option<Native>,
}

struct ControlOwner {
    config: CommandConfig,
    registry: Registry,
    manager: Manager,
    daemon: PathBuf,
    active: HashMap<SlotId, Vec<Active>>,
    offers: Ledger,
    parked: VecDeque<slots::Parked>,
    departure: VecDeque<(SlotId, tmux::Inventory)>,
    quarantine: HashSet<SlotId>,
    wake: Wake,
    stopping: Arc<AtomicBool>,
    profile: Arc<ProfileLease>,
}

impl ControlOwner {
    fn run(
        &mut self,
        starts: mpsc::Receiver<Start>,
        confirms: mpsc::Receiver<confirm::Request>,
        releases: mpsc::Receiver<Release>,
        install: mpsc::SyncSender<Installing>,
        reap: mpsc::Receiver<Native>,
        submission: &Mutex<()>,
    ) {
        let mut accepting = Some((install, confirms));
        loop {
            if let Some((_, confirms)) = &accepting {
                for request in confirms.try_iter().take(MAX_LIVE) {
                    self.confirm_one(request);
                }
            }
            let awaiting_confirm = self.expire_confirmations();
            let io_alive = self.maintain(&reap);
            if !io_alive {
                self.stopping.store(true, Ordering::Release);
            }
            if self.stopping.load(Ordering::Acquire) {
                if let Some((_, confirms)) = accepting.take() {
                    // Flush a concurrent Start or Release that passed the stop flag.
                    drop(submission.lock());
                    answer_stopped(&starts, &releases);
                    for confirmation in confirms.try_iter() {
                        let _ = confirmation.reply.send(Err(STOPPED));
                    }
                }
                if !io_alive && self.active.is_empty() && self.manager.drain_pending() {
                    return;
                }
            } else if let Some((install, _)) = &accepting
                && !awaiting_confirm
                && self.control_turn(&starts, &releases, install)
            {
                continue;
            }
            thread::park_timeout(HEALTH);
        }
    }

    fn control_turn(
        &mut self,
        starts: &mpsc::Receiver<Start>,
        releases: &mpsc::Receiver<Release>,
        install: &mpsc::SyncSender<Installing>,
    ) -> bool {
        let sampled = self.advance_parked(install);
        if self.expire_confirmations() {
            return true;
        }
        if !sampled {
            self.sample_idle_departure();
        }
        if self.stopping.load(Ordering::Acquire) {
            return true;
        }
        if let Ok((slot, reply)) = releases.try_recv() {
            if !reply.is_closed() {
                let _ = reply.send(self.release(slot));
            }
        } else if let Ok(request) = starts.try_recv() {
            self.start_one(request, install);
        } else {
            return false;
        }
        true
    }

    fn confirm_one(&mut self, request: confirm::Request) {
        let Some(link) = self.active.get_mut(&request.slot).and_then(|links| {
            links
                .iter_mut()
                .find(|link| request.matches(link, request.slot))
        }) else {
            let _ = request.reply.send(Err(STOPPED));
            return;
        };
        let Some(confirmation) = &link.confirmation else {
            let _ = request.reply.send(Err(STOPPED));
            return;
        };
        if confirmation.active() || link.retiring.is_some() {
            let _ = request.reply.send(Err(STOPPED));
            return;
        }
        let now = guard::sample().map(|sample| sample.continuous);
        let refusal = request.refusal(confirmation, now).or_else(|| {
            if self.stopping.load(Ordering::Acquire) {
                Some(Reason::OwnerLost)
            } else if !self.config.terminal_allowed(&link.account)
                || !matches!(link.port.source.check(), GuardResult::Applied(()))
                || !matches!(link.port.writer_live(), GuardResult::Applied(()))
                || matches!(
                    link.port.source.health(link.claim, guard::sample),
                    GuardResult::Retired(_)
                )
            {
                Some(Reason::Authority)
            } else {
                None
            }
        });
        if let Some(reason) = refusal {
            link.port.request_close(reason);
            let _ = request.reply.send(Err(reason.start_error()));
            return;
        }
        let acknowledge = || confirmation.acknowledge(request.reply);
        let replacement = self
            .parked
            .iter()
            .position(|work| work.slot == request.slot);
        let completed = if let Some(index) = replacement {
            self.parked[index].replacement.confirm(
                request.claim,
                now.expect("checked confirmation clock"),
                acknowledge,
            )
        } else {
            acknowledge()
        };
        if completed {
            link.pending_start.take();
            if let Some(index) = replacement {
                self.parked.remove(index);
            }
        } else {
            link.port.request_close(Reason::Detached);
        }
    }

    fn expire_confirmations(&self) -> bool {
        let mut waiting = false;
        let now = guard::sample().map(|at| at.continuous);
        for link in self.active.values().flatten() {
            let Some(confirmation) = &link.confirmation else {
                continue;
            };
            if confirmation.active()
                || link.retiring.is_some()
                || link.port.source.reason().is_some()
            {
                continue;
            }
            match (now, link.port.source.check()) {
                (_, GuardResult::Retired(reason)) => link.port.request_close(reason),
                (None, _) => link.port.request_close(Reason::Clock),
                (Some(now), _) if now >= confirmation.until => {
                    link.port.request_close(Reason::Deadline)
                }
                _ => waiting = true,
            }
        }
        waiting
    }

    fn maintain(&mut self, reap: &mpsc::Receiver<Native>) -> bool {
        let io_alive = loop {
            match reap.try_recv() {
                Ok(native) => {
                    if let Some(active) = self.exact_native(&native) {
                        active.retiring = Some(native);
                    } else {
                        self.stopping.store(true, Ordering::Release);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break true,
                Err(mpsc::TryRecvError::Disconnected) => break false,
            }
        };
        let mut reaped = Vec::new();
        self.active.retain(|slot, links| {
            links.retain_mut(|active| {
                let Some(native) = active.retiring.as_mut() else {
                    if !io_alive {
                        active.port.request_close(Reason::OwnerLost);
                    }
                    return io_alive;
                };
                if !matches!(
                    native.attachment.shutdown(Instant::now() + REAP_TURN),
                    Ok(Shutdown::Reaped)
                ) {
                    return true;
                }
                reaped.push((*slot, active.claim, active.role, active.inventory));
                false
            });
            !links.is_empty()
        });
        for (slot, claim, role, inventory) in reaped {
            if let Some(parked) = self.parked.iter_mut().find(|work| work.slot == slot) {
                if !parked.replacement.reaped(claim) {
                    self.stopping.store(true, Ordering::Release);
                }
            } else if role == Role::Writer {
                self.departure.push_back((slot, inventory));
            }
        }
        io_alive
    }

    fn recover_exit(&mut self, slot: SlotId) {
        let Ok(row) = self.row(slot) else { return };
        let until = Instant::now() + RECONCILE_TURN;
        if recovery::inventory(&mut self.registry, &mut self.manager, row, until).is_err() {
            let _ = recovery::retire_if_absent(&mut self.registry, &self.manager, row, until);
        }
    }

    fn reconcile(&mut self) -> Result<()> {
        let limit = guard::sample()
            .ok_or(DEADLINE)?
            .continuous
            .checked_add(Duration::from_secs(20))
            .ok_or(DEADLINE)?;
        for slot in self.registry.slots()? {
            let until = self.reconcile_deadline(limit)?;
            let cleanup = !self.config.terminal_allowed(&slot.account)
                || matches!(slot.state, State::Retiring(_));
            match recovery::inventory(&mut self.registry, &mut self.manager, slot, until) {
                Ok(inventory) if cleanup => {
                    let mut row = self.row(slot.id)?;
                    if !matches!(row.state, State::Retiring(_)) {
                        self.registry.retiring(row)?;
                        row = self.row(slot.id)?;
                    }
                    let until = self.reconcile_deadline(limit)?;
                    if self.manager.kill_session(row.id, inventory, until).is_ok() {
                        let _ = recovery::retire_if_absent(
                            &mut self.registry,
                            &self.manager,
                            row,
                            until,
                        );
                    }
                }
                Ok(_) => (),
                Err(_) => {
                    if cleanup && !matches!(slot.state, State::Retiring(_)) && slot.proof.is_none()
                    {
                        self.registry.retiring(slot)?;
                    }
                    let _ =
                        recovery::retire_if_absent(&mut self.registry, &self.manager, slot, until);
                }
            }
        }
        self.reconcile_deadline(limit)?;
        Ok(())
    }

    fn reconcile_deadline(&self, limit: Duration) -> Result<Instant> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(STOPPED);
        }
        let now = guard::sample().ok_or(DEADLINE)?.continuous;
        let remaining = limit
            .checked_sub(now)
            .filter(|time| !time.is_zero())
            .ok_or(DEADLINE)?;
        Instant::now()
            .checked_add(remaining.min(RECONCILE_TURN))
            .ok_or(DEADLINE)
    }

    fn row(&self, id: SlotId) -> Result<Slot> {
        self.registry
            .slots()?
            .into_iter()
            .find(|slot| slot.id == id)
            .ok_or(MISSING)
    }

    fn release(&mut self, slot: SlotId) -> Result<()> {
        if self.active.contains_key(&slot)
            || self.pending_departure(slot)
            || self.quarantine.contains(&slot)
            || self.parked.iter().any(|work| work.slot == slot)
        {
            return Err(CLEANUP);
        }
        let row = self.row(slot)?;
        recovery::retire_if_absent(
            &mut self.registry,
            &self.manager,
            row,
            Instant::now() + RECONCILE_TURN,
        )
    }

    fn start_one(&mut self, (start, reply, permit): Start, install: &mpsc::SyncSender<Installing>) {
        let stopping = Arc::clone(&self.stopping);
        let budget = match AdmissionBudget::open(&start, &reply, &stopping) {
            Ok(budget) => budget,
            Err(error) => return refuse(reply, error),
        };
        let plan = match self.plan(&start, &budget) {
            Ok(plan) => plan,
            Err(error) => return refuse(reply, error),
        };
        match plan {
            slots::Plan::Busy {
                slot,
                writer,
                offer,
            } => {
                let _ = reply.send(StartOutcome::Busy {
                    slot,
                    writer,
                    offer,
                });
            }
            slots::Plan::Replace { slot, old } => {
                let deadline = budget.due;
                if let Err((reply, error)) =
                    self.reserve_replacement(slot, old, start, reply, permit, deadline)
                {
                    refuse(reply, error);
                }
            }
            slots::Plan::Launch(launch) => {
                let deadline = budget.due;
                match self.prepare_start(launch, &budget, permit) {
                    Ok((native, confirmation)) => {
                        self.install_native(native, deadline, confirmation, reply, install)
                    }
                    Err(error) => refuse(reply, error),
                }
            }
        }
    }

    fn prepare_start(
        &mut self,
        launch: slots::Launch,
        budget: &AdmissionBudget<'_>,
        permit: StartPermit,
    ) -> Result<(Native, Option<Arc<Confirmation>>)> {
        let claim = self.allocate_claim()?;
        let attempt = self.launch(budget, claim, launch);
        let checked = matches!(budget.start.selection, StartChoice::Checked { .. });
        if (checked || attempt.is_err())
            && let Some(link) = self
                .active
                .get_mut(&launch.slot)
                .and_then(|links| links.iter_mut().find(|link| link.claim == claim))
        {
            link.pending_start = Some(permit);
        }
        let native = attempt?;
        if !checked {
            return Ok((native, None));
        }
        match self.prepare_confirmation(&native, budget.due) {
            Ok(confirmation) => Ok((native, Some(confirmation))),
            Err(error) => {
                self.retire(native);
                Err(error)
            }
        }
    }

    fn prepare_confirmation(
        &mut self,
        native: &Native,
        deadline: Duration,
    ) -> Result<Arc<Confirmation>> {
        let at = guard::sample().ok_or(STOPPED)?;
        let until = at.continuous.checked_add(CONFIRM_BUDGET).ok_or(DEADLINE)?;
        let confirmation = Confirmation::new(deadline.min(until));
        let link = self.exact_native(native).ok_or(CLEANUP)?;
        link.confirmation = Some(Arc::clone(&confirmation));
        Ok(confirmation)
    }

    fn install_native(
        &mut self,
        native: Native,
        deadline: Duration,
        confirmation: Option<Arc<Confirmation>>,
        reply: oneshot::Sender<StartOutcome>,
        install: &mpsc::SyncSender<Installing>,
    ) {
        let request = Installing {
            native,
            deadline,
            reply,
            confirmation,
        };
        if let Err(error) = install.try_send(request) {
            let request = match error {
                mpsc::TrySendError::Full(request) | mpsc::TrySendError::Disconnected(request) => {
                    request
                }
            };
            self.retire(request.native);
            refuse(request.reply, CLEANUP);
        } else {
            self.wake.notify();
        }
    }

    fn allocate_claim(&mut self) -> Result<Claim> {
        Ok(Claim {
            random: random()?,
            generation: self.registry.allocate()?,
        })
    }

    fn launch(
        &mut self,
        budget: &AdmissionBudget<'_>,
        claim: Claim,
        launch: slots::Launch,
    ) -> Result<Native> {
        let slots::Launch {
            slot,
            inventory,
            writer,
            rows,
            cols,
        } = launch;
        let (role, port) = if let Some(writer) = writer {
            let linked = self
                .writer(slot)
                .filter(|link| link.claim == writer)
                .ok_or(WRITER_MISSING)?;
            (
                Role::Observer,
                Port::observer(
                    Arc::clone(&budget.start.source),
                    self.wake.clone(),
                    &linked.port,
                    writer,
                ),
            )
        } else {
            (
                Role::Writer,
                Port::new(Arc::clone(&budget.start.source), self.wake.clone()),
            )
        };
        let checked = CheckedLaunch::for_role(
            &self.daemon,
            &self.manager.tmux,
            &self.manager.socket(slot),
            tmux::SESSION,
            role,
        )?;
        budget.check(false)?;
        self.active.entry(slot).or_default().push(Active {
            account: budget.start.account,
            inventory,
            writer,
            claim,
            role,
            confirmation: None,
            pending_start: None,
            port: Arc::clone(&port),
            retiring: None,
        });
        let mut attachment = match checked.launch_until(
            claim.generation,
            rows,
            cols,
            budget.until,
            Some(Arc::clone(&self.profile)),
            || budget.check(false),
        ) {
            Ok(attachment) => attachment,
            Err(failure) => {
                if let Some(mut attachment) = failure.pending {
                    attachment.hold_terminal_room(budget.start.room_hold.clone());
                    self.retire(Native {
                        slot,
                        claim,
                        port,
                        attachment: *attachment,
                    });
                } else {
                    self.remove_unstarted(slot, claim);
                }
                return Err(failure.error);
            }
        };
        attachment.hold_terminal_room(budget.start.room_hold.clone());
        let native = Native {
            slot,
            claim,
            port,
            attachment,
        };
        let ready = if role == Role::Observer {
            self.manager.wait_observer_until(
                slot,
                inventory,
                native.attachment.worker_pid(),
                budget.until,
            )
        } else {
            self.manager.wait_clients_until(
                slot,
                inventory,
                1,
                tmux::ClientChange::Arriving,
                budget.until,
            )
        };
        if let Err(error) = ready.and_then(|_| budget.check(false)) {
            self.retire(native);
            return Err(error);
        }
        Ok(native)
    }

    fn select(
        &mut self,
        budget: &AdmissionBudget<'_>,
        mut count: usize,
        existing: Option<Slot>,
        default: bool,
    ) -> Result<(SlotId, tmux::Inventory)> {
        let start = budget.start;
        if let Some(slot) = existing {
            budget.check(true)?;
            if let Ok(inventory) =
                recovery::inventory(&mut self.registry, &mut self.manager, slot, budget.until)
            {
                if !matches!(slot.state, State::Retiring(_)) {
                    return Ok((slot.id, inventory));
                }
                return Err(CLEANUP);
            }
            if !default {
                return Err(UNAVAILABLE);
            }
            budget.check(false)?;
            recovery::retire_if_absent(&mut self.registry, &self.manager, slot, budget.until)
                .map_err(|_| UNAVAILABLE)?;
            count -= 1;
        }
        if count >= super::MAX_SLOTS {
            return Err(CAPACITY);
        }
        recovery::check_capacity(&self.manager)?;
        let slot = Slot {
            id: SlotId(random()?),
            account: start.account,
            default,
            marker: random()?,
            state: State::Prepared,
            proof: None,
        };
        budget.check(true)?;
        self.registry.prepare(slot)?;
        budget.check(true)?;
        let inventory =
            self.manager
                .create_until(slot, start.rows, start.cols, budget.until, || {
                    budget.check(false)
                })?;
        recovery::pin_ready(&mut self.registry, slot, inventory)?;
        Ok((slot.id, inventory))
    }

    fn retire(&mut self, mut native: Native) {
        native.port.close(Reason::OwnerLost);
        drop(native.attachment.master.take());
        if let Some(active) = self.exact_native(&native) {
            active.retiring = Some(native);
        } else {
            self.stopping.store(true, Ordering::Release);
        }
    }
}

fn refuse(reply: oneshot::Sender<StartOutcome>, error: Error) {
    let _ = reply.send(StartOutcome::Refused(error));
}

fn answer_stopped(starts: &mpsc::Receiver<Start>, releases: &mpsc::Receiver<Release>) {
    for (_, reply, _) in starts.try_iter() {
        refuse(reply, STOPPED);
    }
    for (_, reply) in releases.try_iter() {
        let _ = reply.send(Err(STOPPED));
    }
}

struct AdmissionBudget<'a> {
    start: &'a VerifiedStart,
    reply: &'a oneshot::Sender<StartOutcome>,
    stopping: &'a AtomicBool,
    until: Instant,
    due: Duration,
}

impl<'a> AdmissionBudget<'a> {
    fn open(
        start: &'a VerifiedStart,
        reply: &'a oneshot::Sender<StartOutcome>,
        stopping: &'a AtomicBool,
    ) -> Result<Self> {
        let at = guard::sample().ok_or(STOPPED)?;
        let due = at
            .continuous
            .checked_add(START_BUDGET)
            .ok_or(STOPPED)?
            .min(start.deadline);
        Self::fixed_at(start, reply, stopping, due, at.continuous, true)
    }

    fn fixed(
        start: &'a VerifiedStart,
        reply: &'a oneshot::Sender<StartOutcome>,
        stopping: &'a AtomicBool,
        due: Duration,
    ) -> Result<Self> {
        let at = guard::sample().ok_or(STOPPED)?.continuous;
        Self::fixed_at(start, reply, stopping, due, at, false)
    }

    fn fixed_at(
        start: &'a VerifiedStart,
        reply: &'a oneshot::Sender<StartOutcome>,
        stopping: &'a AtomicBool,
        due: Duration,
        at: Duration,
        wait_suspect: bool,
    ) -> Result<Self> {
        let until = Instant::now() + due.saturating_sub(at);
        let budget = Self {
            start,
            reply,
            stopping,
            until,
            due,
        };
        budget.check(wait_suspect)?;
        Ok(budget)
    }

    fn check(&self, wait_suspect: bool) -> Result<()> {
        loop {
            if self.stopping.load(Ordering::Acquire) || self.reply.is_closed() {
                return Err(STOPPED);
            }
            let at = guard::sample().ok_or(STOPPED)?;
            let remaining = self
                .until
                .saturating_duration_since(Instant::now())
                .min(self.due.saturating_sub(at.continuous));
            if remaining.is_zero() {
                return Err(DEADLINE);
            }
            match self.start.source.check() {
                GuardResult::Applied(()) => return Ok(()),
                GuardResult::Retired(reason) => return Err(reason.start_error()),
                GuardResult::Withheld if !wait_suspect => return Err(DENIED),
                GuardResult::Withheld => thread::sleep(HEALTH.min(remaining)),
            }
        }
    }
}
