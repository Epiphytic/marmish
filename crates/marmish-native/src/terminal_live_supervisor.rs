//! Thread ownership for the proof-gated native Terminal writer.
#[path = "terminal_live_supervisor_guard.rs"]
mod guard;
#[path = "terminal_live_supervisor_io.rs"]
mod io;
#[path = "terminal_selection.rs"]
pub(crate) mod selection;
#[path = "terminal_selection_wire.rs"]
mod selection_wire;
#[path = "terminal_live_supervisor_tests.rs"]
mod tests;

use super::{
    Account, CAPACITY, CLEANUP, CheckedLaunch, DENIED, Error, IdentityMaterial, Instant, MISSING,
    Manager, ProfileLease, REQUEST, Registry, Result, Role, Shutdown, Slot, SlotId, State,
    UNAVAILABLE, backend, random, recovery, tmux,
};
use crate::command_config::CommandConfig;
use guard::Source;
use io::{Event, Installing, Native, Operation, Port, PushError, Wake};
use std::collections::HashMap;
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
const START_BUDGET: Duration = Duration::from_secs(10);
const RECONCILE_TURN: Duration = Duration::from_secs(2);
const HEALTH: Duration = Duration::from_millis(10);
const REAP_TURN: Duration = Duration::from_millis(2);
const STOPPED: Error = Error::Invalid("terminal_owner_unavailable");
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
    Default,
    New,
}

struct VerifiedStart {
    account: Account,
    selection: StartChoice,
    rows: u16,
    cols: u16,
    source: Arc<Source>,
    deadline: Duration,
}

enum StartOutcome {
    Attached(AdmittedTerminal),
    Busy { slot: SlotId, writer: Claim },
    Refused(Error),
}

struct AdmittedTerminal {
    slot: SlotId,
    claim: Claim,
    port: Arc<Port>,
    closed_reported: bool,
}

impl AdmittedTerminal {
    fn submit(&self, operation: Operation) -> std::result::Result<(), PushError> {
        self.port.push(operation)
    }

    fn try_event(&mut self) -> Option<Event> {
        if self.closed_reported {
            return None;
        }
        let event = self.port.event()?;
        self.closed_reported = matches!(event, Event::Closed(_));
        Some(event)
    }

    async fn changed(&self) {
        self.port.changed().await;
    }

    fn renew(&self, source: SourceId, accepted: Duration) -> GuardResult<bool> {
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

type Start = (VerifiedStart, oneshot::Sender<StartOutcome>);
type Release = (SlotId, oneshot::Sender<Result<()>>);

#[derive(Clone)]
struct LiveControl {
    start: mpsc::SyncSender<Start>,
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
        self.submit(&self.start, (start, reply))
    }

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

struct LiveSupervisor {
    control: LiveControl,
    wake: Wake,
    control_worker: Option<JoinHandle<()>>,
    io_worker: Option<JoinHandle<Result<()>>>,
}

impl LiveSupervisor {
    fn start(
        profile: Arc<ProfileLease>,
        material: &IdentityMaterial,
        daemon: &Path,
        tmux: &Path,
    ) -> Result<Self> {
        backend::paths::daemon(daemon)?;
        let mut manager = Manager::new(tmux, profile.root())?;
        manager.profile = Some(Arc::clone(&profile));
        let (wake, reader) = Wake::open()?;
        let stopping = Arc::new(AtomicBool::new(false));
        let submission = Arc::new(Mutex::new(()));
        let mut owner = ControlOwner {
            config: CommandConfig::load(profile.root())?,
            registry: Registry::open(&profile, material)?,
            manager,
            daemon: daemon.to_owned(),
            active: HashMap::with_capacity(MAX_LIVE),
            wake: wake.clone(),
            stopping: Arc::clone(&stopping),
            profile,
        };
        owner.reconcile()?;
        let (install, installs) = mpsc::sync_channel(1);
        let (returned, reap) = mpsc::sync_channel(MAX_LIVE);
        let (start, starts) = mpsc::sync_channel(MAX_LIVE);
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
                owner.run(starts, releases, install, reap, &close_submission);
                drop(guard);
            })?;
        let control = LiveControl {
            start,
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

    fn stop(&self) {
        self.control.stopping.store(true, Ordering::Release);
        self.wake.notify();
        self.control.owner.unpark();
    }

    fn join_if_finished(&mut self) -> Option<Result<()>> {
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
    claim: Claim,
    port: Arc<Port>,
    retiring: Option<Native>,
}

struct ControlOwner {
    config: CommandConfig,
    registry: Registry,
    manager: Manager,
    daemon: PathBuf,
    active: HashMap<SlotId, Active>,
    wake: Wake,
    stopping: Arc<AtomicBool>,
    profile: Arc<ProfileLease>,
}

impl ControlOwner {
    fn run(
        &mut self,
        starts: mpsc::Receiver<Start>,
        releases: mpsc::Receiver<Release>,
        install: mpsc::SyncSender<Installing>,
        reap: mpsc::Receiver<Native>,
        submission: &Mutex<()>,
    ) {
        let mut install = Some(install);
        loop {
            let io_alive = self.maintain(&reap);
            if !io_alive {
                self.stopping.store(true, Ordering::Release);
            }
            if self.stopping.load(Ordering::Acquire) {
                if install.take().is_some() {
                    // Flush a concurrent submit which passed the stop flag.
                    drop(submission.lock());
                    answer_stopped(&starts, &releases);
                }
                // No I/O owner can transfer further native handles once its
                // sender is closed. A panic keeps their leases in the reaper.
                if !io_alive && self.active.is_empty() && self.manager.drain_pending() {
                    return;
                }
            } else if let Ok((slot, reply)) = releases.try_recv() {
                if !reply.is_closed() {
                    let _ = reply.send(self.release(slot));
                }
                continue;
            } else if let (Ok(request), Some(install)) = (starts.try_recv(), install.as_ref()) {
                self.start_one(request, install);
                continue;
            }
            thread::park_timeout(HEALTH);
        }
    }

    fn maintain(&mut self, reap: &mpsc::Receiver<Native>) -> bool {
        let io_alive = loop {
            match reap.try_recv() {
                Ok(native) => {
                    if let Some(active) = self.active.get_mut(&native.slot)
                        && active.claim == native.claim
                        && active.retiring.is_none()
                    {
                        active.retiring = Some(native);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break true,
                Err(mpsc::TryRecvError::Disconnected) => break false,
            }
        };
        let mut shell_exits = Vec::new();
        self.active.retain(|slot, active| {
            let Some(native) = active.retiring.as_mut() else {
                if !io_alive {
                    active.port.close(Reason::OwnerLost);
                }
                return io_alive;
            };
            match native.attachment.shutdown(Instant::now() + REAP_TURN) {
                Ok(Shutdown::Reaped) => {
                    if active.port.source.reason() == Some(Reason::ShellExit) {
                        shell_exits.push(*slot);
                    }
                    false
                }
                _ => true,
            }
        });
        if !self.stopping.load(Ordering::Acquire) {
            for slot in shell_exits {
                self.recover_exit(slot);
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
        for slot in self.registry.slots()? {
            let until = Instant::now() + RECONCILE_TURN;
            let cleanup = !self.config.terminal_allowed(&slot.account)
                || matches!(slot.state, State::Retiring(_));
            match recovery::inventory(&mut self.registry, &mut self.manager, slot, until) {
                Ok(inventory) if cleanup => {
                    let mut row = self.row(slot.id)?;
                    if !matches!(row.state, State::Retiring(_)) {
                        self.registry.retiring(row)?;
                        row = self.row(slot.id)?;
                    }
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
        Ok(())
    }

    fn row(&self, id: SlotId) -> Result<Slot> {
        self.registry
            .slots()?
            .into_iter()
            .find(|slot| slot.id == id)
            .ok_or(MISSING)
    }

    fn release(&mut self, slot: SlotId) -> Result<()> {
        if self.active.contains_key(&slot) {
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

    fn start_one(&mut self, (start, reply): Start, install: &mpsc::SyncSender<Installing>) {
        let stopping = Arc::clone(&self.stopping);
        let budget = match AdmissionBudget::open(&start, &reply, &stopping) {
            Ok(budget) => budget,
            Err(error) => return refuse(reply, error),
        };
        let native = match self.start_writer(&start, &budget) {
            Err(error) => return refuse(reply, error),
            Ok(Writer::Busy { slot, writer }) => {
                let _ = reply.send(StartOutcome::Busy { slot, writer });
                return;
            }
            Ok(Writer::Native(native)) => native,
        };
        if let Err(error) = budget.check(false) {
            self.retire(native);
            return refuse(reply, error);
        }
        let deadline = budget.due;
        let request = Installing {
            native,
            deadline,
            reply,
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

    fn start_writer(
        &mut self,
        start: &VerifiedStart,
        budget: &AdmissionBudget<'_>,
    ) -> Result<Writer> {
        if !self.config.terminal_allowed(&start.account) {
            return Err(DENIED);
        }
        if !valid_size(start.rows, start.cols) {
            return Err(REQUEST);
        }
        let rows = self.registry.slots()?;
        if matches!(start.selection, StartChoice::Default)
            && let Some(slot) = rows
                .iter()
                .find(|row| row.account == start.account && row.default)
            && let Some(active) = self.active.get(&slot.id)
        {
            budget.check(false)?;
            return if active.port.source.reason().is_none() {
                Ok(Writer::Busy {
                    slot: slot.id,
                    writer: active.claim,
                })
            } else {
                Err(CLEANUP)
            };
        }
        if self.active.len() >= MAX_LIVE || !self.manager.drain_pending() {
            return Err(CAPACITY);
        }
        let (slot, inventory) = self.select(start, budget, &rows)?;
        budget.check(true)?;
        self.manager.wait_clients_until(
            slot,
            inventory,
            0,
            tmux::ClientChange::Departing,
            budget.until,
        )?;
        let checked = CheckedLaunch::for_role(
            &self.daemon,
            &self.manager.tmux,
            &self.manager.socket(slot),
            tmux::SESSION,
            Role::Writer,
        )?;
        budget.check(true)?;
        let claim = Claim {
            random: random()?,
            generation: self.registry.allocate()?,
        };
        let port = Port::new(Arc::clone(&start.source), self.wake.clone());
        budget.check(true)?;
        self.active.insert(
            slot,
            Active {
                claim,
                port: Arc::clone(&port),
                retiring: None,
            },
        );
        let attachment = match checked.launch_until(
            claim.generation,
            start.rows,
            start.cols,
            budget.until,
            Some(Arc::clone(&self.profile)),
            || budget.check(false),
        ) {
            Ok(attachment) => attachment,
            Err(failure) => {
                if let Some(attachment) = failure.pending {
                    self.retire(Native {
                        slot,
                        claim,
                        port,
                        attachment,
                    });
                } else {
                    self.active.remove(&slot);
                }
                return Err(failure.error);
            }
        };
        let native = Native {
            slot,
            claim,
            port,
            attachment,
        };
        if let Err(error) = self
            .manager
            .wait_clients_until(
                slot,
                inventory,
                1,
                tmux::ClientChange::Arriving,
                budget.until,
            )
            .and_then(|_| budget.check(false))
        {
            self.retire(native);
            return Err(error);
        }
        Ok(Writer::Native(native))
    }

    fn select(
        &mut self,
        start: &VerifiedStart,
        budget: &AdmissionBudget<'_>,
        rows: &[Slot],
    ) -> Result<(SlotId, tmux::Inventory)> {
        let mut count = rows.len();
        if matches!(start.selection, StartChoice::Default)
            && let Some(slot) = rows
                .iter()
                .copied()
                .find(|slot| slot.account == start.account && slot.default)
        {
            budget.check(true)?;
            if let Ok(inventory) =
                recovery::inventory(&mut self.registry, &mut self.manager, slot, budget.until)
            {
                if !matches!(slot.state, State::Retiring(_)) {
                    return Ok((slot.id, inventory));
                }
                return Err(CLEANUP);
            }
            budget.check(true)?;
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
            default: matches!(start.selection, StartChoice::Default),
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
        if let Some(active) = self.active.get_mut(&native.slot)
            && active.claim == native.claim
            && active.retiring.is_none()
        {
            active.retiring = Some(native);
        }
    }
}

enum Writer {
    Busy { slot: SlotId, writer: Claim },
    Native(Native),
}

fn refuse(reply: oneshot::Sender<StartOutcome>, error: Error) {
    let _ = reply.send(StartOutcome::Refused(error));
}

fn answer_stopped(starts: &mpsc::Receiver<Start>, releases: &mpsc::Receiver<Release>) {
    for (_, reply) in starts.try_iter() {
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
        let until = Instant::now() + due.saturating_sub(at.continuous);
        let budget = Self {
            start,
            reply,
            stopping,
            until,
            due,
        };
        budget.check(true)?;
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
