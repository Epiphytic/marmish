//! Dedicated runtime for volatile Terminal transport. Command retains its actor.
use crate::command_config::{CommandConfig, Role};
use crate::command_wire::{Account, Id};
use crate::identity::{IdentityMaterial, ProfileLease};
use crate::ipc::terminal_handshake::{Attach, Intake, LocalStart, PendingStart};
use crate::mdk_adapter::{
    Binding, MdkOwner,
    terminal_enrollment_driver::Route,
    terminal_live_enrollment::{AdmissionLedger, InboundState},
};
use crate::terminal_backend::supervisor::{
    Registry,
    live::{LiveControl, LiveSupervisor, selection, selection_wire::Choice, stream},
};
use crate::terminal_fence::FenceState;
use crate::terminal_frames::Size;
use crate::transport::{ArtiDirs, RelayPolicy};
use crate::{Error, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout_at};

const BINDING: Error = Error::Invalid("terminal_command_binding");
const BINDING_DEADLINE: Error = Error::Invalid("terminal_binding_deadline");
const TRANSPORT: Error = Error::Transport("terminal_transport_unavailable");
const STOPPED: Error = Error::Identity("owned_stop_incomplete");
const SNAPSHOT_VALID: Duration = Duration::from_secs(2);
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(250);
const POLL: Duration = Duration::from_millis(25);
const LISTENERS: usize = 32;
const ROOMS: usize = 16;

fn listener_delay(failures: u8) -> Duration {
    if failures == 0 {
        POLL
    } else {
        Duration::from_millis(250) * (1 << (failures - 1).min(3))
    }
}

async fn await_listener_registration(
    current: impl Fn() -> bool,
    eligible: impl Fn() -> bool,
    mut register: impl FnMut() -> Result<InboundState>,
    mut report: impl FnMut(),
) -> Option<InboundState> {
    let mut next_attempt = Instant::now();
    let mut next_report = next_attempt;
    let mut failures = 0u8;
    while current() {
        if Instant::now() < next_attempt {
            tokio::time::sleep_until(next_attempt).await;
            continue;
        }
        if !eligible() {
            tokio::time::sleep(POLL).await;
            continue;
        }
        if let Ok(inbound) = register() {
            return Some(inbound);
        }
        let now = Instant::now();
        if now >= next_report {
            report();
            next_report = now + Duration::from_secs(30);
        }
        failures = failures.saturating_add(1);
        next_attempt = now + listener_delay(failures);
    }
    None
}

pub(crate) struct Prepared {
    route: Route,
    server: Option<(CommandConfig, Registry, PathBuf)>,
}

impl Prepared {
    #[cfg(test)]
    pub(crate) fn for_loopback_fixture(
        profile: &ProfileLease,
        material: &IdentityMaterial,
        commands: Option<CommandConfig>,
        policy: RelayPolicy,
        daemon: PathBuf,
        roots: rustls::RootCertStore,
    ) -> Result<Self> {
        let mut prepared = Self::open(profile, material, commands, policy)?;
        if let Some((_, _, executable)) = &mut prepared.server {
            *executable = daemon;
        }
        prepared.route.set_fixture_roots(roots)?;
        Ok(prepared)
    }

    pub(crate) fn open(
        profile: &ProfileLease,
        material: &IdentityMaterial,
        commands: Option<CommandConfig>,
        policy: RelayPolicy,
    ) -> Result<Self> {
        let directories = ArtiDirs {
            state: profile.arti_state().to_owned(),
            cache: profile.arti_cache().to_owned(),
        };
        let server = commands
            .filter(|commands| {
                !matches!(commands.role(), Role::Client)
                    && commands
                        .execution_clients()
                        .iter()
                        .any(|peer| commands.terminal_allowed(peer))
                    && crate::terminal_backend::paths::checked_uid().is_ok()
            })
            .and_then(|commands| {
                match Registry::open(profile, material)
                    .and_then(|registry| Ok((commands, registry, std::env::current_exe()?)))
                {
                    Ok(server) => Some(server),
                    Err(_) => {
                        crate::diagnostics::event("terminal_owner", "server_registry_unavailable");
                        None
                    }
                }
            });
        Ok(Self {
            route: Route::new(policy, directories, material.signer()),
            server,
        })
    }
}

pub(crate) struct BindingRequest {
    peer: Account,
    relays: Vec<String>,
    session: Id,
    deadline: Instant,
    reply: oneshot::Sender<Result<Option<Binding>>>,
}

impl BindingRequest {
    pub(crate) fn complete(self, owner: &mut MdkOwner) {
        if self.reply.is_closed() || Instant::now() >= self.deadline {
            return;
        }
        let outcome = owner.terminal_client_binding(self.peer, &self.relays, self.session);
        if Instant::now() < self.deadline {
            let _ = self.reply.send(outcome);
        }
    }
}

#[derive(Default)]
struct Snapshot {
    at: Option<Instant>,
    routes: Vec<Binding>,
}

impl Snapshot {
    fn fresh(&self) -> bool {
        self.fresh_at(Instant::now())
    }

    fn fresh_at(&self, now: Instant) -> bool {
        self.at
            .is_some_and(|at| now.saturating_duration_since(at) < SNAPSHOT_VALID)
    }
}

pub(crate) struct Control {
    pub(crate) intake: Intake,
    pub(crate) requests: mpsc::Receiver<BindingRequest>,
    snapshots: watch::Sender<Snapshot>,
    refreshed: Option<Instant>,
}

impl Control {
    pub(crate) fn refresh(&mut self, owner: &mut MdkOwner) {
        let now = Instant::now();
        if self
            .refreshed
            .is_some_and(|at| now.duration_since(at) < SNAPSHOT_INTERVAL)
        {
            return;
        }
        self.refreshed = Some(now);
        if let Ok(routes) = owner.terminal_server_bindings() {
            self.snapshots.send_replace(Snapshot {
                at: Some(now),
                routes,
            });
        }
    }
}

pub(crate) struct Runtime {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<()>>,
}

impl Runtime {
    pub(crate) fn start(prepared: Prepared, profile: Arc<ProfileLease>) -> Result<(Self, Control)> {
        let (intake, local) = Intake::new();
        let (requests, bindings) = mpsc::channel(4);
        let (snapshots, latest) = watch::channel(Snapshot::default());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::Builder::new()
            .name("marmish-terminal-transport".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .max_blocking_threads(2)
                    .build()?;
                let runtime_tasks = tokio::task::LocalSet::new();
                runtime_tasks.block_on(
                    &runtime,
                    run(prepared, profile, local, requests, latest, stopping),
                )
            })?;
        Ok((
            Self { stop, thread },
            Control {
                intake,
                requests: bindings,
                snapshots,
                refreshed: None,
            },
        ))
    }

    pub(crate) async fn join(self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        let monotonic = Instant::now();
        let began = crate::command_limits::continuous_time().ok();
        let mut incomplete = false;
        while !self.thread.is_finished() {
            let elapsed = began
                .zip(crate::command_limits::continuous_time().ok())
                .and_then(|(began, now)| now.checked_sub(began));
            if !incomplete
                && elapsed.unwrap_or_else(|| monotonic.elapsed()) >= Duration::from_secs(15)
            {
                crate::diagnostics::event("terminal_owner", "owned_stop_incomplete");
                incomplete = true;
            }
            tokio::time::sleep(POLL).await;
        }
        let result = self.thread.join().map_err(|_| STOPPED)?;
        if incomplete { Err(STOPPED) } else { result }
    }
}

struct Native {
    init: Option<JoinHandle<Result<LiveSupervisor>>>,
    ready: Option<LiveSupervisor>,
    stopping: Arc<AtomicBool>,
}

impl Native {
    fn start(
        prepared: Option<(CommandConfig, Registry, PathBuf)>,
        profile: Arc<ProfileLease>,
    ) -> Self {
        let stopping = Arc::new(AtomicBool::new(false));
        let init = prepared.and_then(|(config, registry, daemon)| {
            let stop = stopping.clone();
            match thread::Builder::new()
                .name("marmish-terminal-initialize".into())
                .spawn(move || {
                    let tmux = tmux_path()?;
                    LiveSupervisor::start_prepared(profile, config, registry, &daemon, &tmux, stop)
                }) {
                Ok(thread) => Some(thread),
                Err(_) => {
                    crate::diagnostics::event("terminal_owner", "server_unavailable");
                    None
                }
            }
        });
        Self {
            init,
            ready: None,
            stopping,
        }
    }

    fn poll(&mut self) {
        if self.init.as_ref().is_some_and(JoinHandle::is_finished) {
            match self.init.take().expect("finished initializer").join() {
                Ok(Ok(supervisor)) => self.ready = Some(supervisor),
                _ => crate::diagnostics::event("terminal_owner", "server_unavailable"),
            }
        }
    }

    async fn finish(mut self) -> Result<()> {
        self.stopping.store(true, Ordering::Release);
        while self.init.is_some() {
            self.poll();
            tokio::time::sleep(POLL).await;
        }
        if let Some(mut supervisor) = self.ready.take() {
            supervisor.stop();
            loop {
                if let Some(result) = supervisor.join_if_finished() {
                    return result;
                }
                tokio::time::sleep(POLL).await;
            }
        }
        Ok(())
    }
}

pub(crate) fn tmux_path() -> Result<PathBuf> {
    [
        "/usr/bin/tmux",
        "/opt/homebrew/bin/tmux",
        "/usr/local/bin/tmux",
        "/opt/local/bin/tmux",
    ]
    .into_iter()
    .find_map(|path| Path::new(path).canonicalize().ok())
    .ok_or(Error::Invalid("terminal_tmux_path"))
}

struct Listener {
    task: tokio::task::AbortHandle,
    stop: Arc<AtomicBool>,
}

type ListenerKey = (Account, Vec<u8>, String, u64);

struct Server {
    route: Route,
    latest: watch::Receiver<Snapshot>,
    stop: Arc<AtomicBool>,
    rooms: Arc<Semaphore>,
    completed: mpsc::Sender<selection::ServerAttachment>,
    ledger: Arc<AdmissionLedger>,
}

impl Server {
    async fn listen(
        self: Arc<Self>,
        binding: Binding,
        endpoint: String,
        stop: Arc<AtomicBool>,
        control: LiveControl,
    ) {
        let current = || {
            !self.stop.load(Ordering::Acquire)
                && !stop.load(Ordering::Acquire)
                && binding.fence.state() != FenceState::Retired
        };
        let eligible = || {
            self.latest.borrow().fresh()
                && binding.fence.state() == FenceState::Live
                && self.route.server_available()
        };
        let Some(mut inbound) = await_listener_registration(
            current,
            eligible,
            || {
                InboundState::register(
                    self.ledger.clone(),
                    binding.context.peer_account(),
                    endpoint.clone(),
                )
            },
            || crate::diagnostics::event("terminal_listener", "admission_unavailable"),
        )
        .await
        else {
            return;
        };
        let mut tokens = selection::SourceTokens::default();
        let mut failures = 0u8;
        while current() {
            if !eligible() {
                tokio::time::sleep(POLL).await;
                continue;
            }
            let server = self.clone();
            let stopping = stop.clone();
            let admitted = Arc::new(AtomicBool::new(false));
            let activated = admitted.clone();
            let grant: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
                !server.stop.load(Ordering::Acquire)
                    && (activated.load(Ordering::Acquire)
                        || (!stopping.load(Ordering::Acquire)
                            && server.route.server_available()
                            && server.latest.borrow().fresh()))
            });
            let ready = self
                .route
                .inbound(
                    &binding,
                    &endpoint,
                    Instant::now() + Duration::from_secs(30),
                    &mut inbound,
                    &self.rooms,
                    grant.clone(),
                )
                .await;
            if let Ok(room) = ready {
                failures = 0;
                if let Ok(selection::ServerOutcome::Attached(attachment)) =
                    selection::accept(room, &control, &mut tokens, &|| grant()).await
                {
                    admitted.store(true, Ordering::Release);
                    if let Err(error) = self.completed.try_send(attachment) {
                        let _ = error.into_inner().close().await;
                    }
                }
            } else {
                failures = failures.saturating_add(1);
            }
            tokio::time::sleep(listener_delay(failures)).await;
        }
    }
}

fn manage_listeners(
    server: &Arc<Server>,
    control: &LiveControl,
    listeners: &mut BTreeMap<ListenerKey, Listener>,
    tasks: &mut JoinSet<()>,
) {
    listeners.retain(|_, listener| !listener.task.is_finished());
    let snapshot = server.latest.borrow();
    if !snapshot.fresh() {
        return;
    }
    let mut eligible = BTreeMap::new();
    for binding in &snapshot.routes {
        if let Some(endpoint) = server.route.endpoint(binding) {
            let key = (
                binding.context.peer_account(),
                binding.context.mls_group().to_vec(),
                endpoint,
                binding.fence.generation().get(),
            );
            eligible.insert(key, binding);
        }
    }
    for (key, listener) in listeners.iter() {
        if !eligible.contains_key(key) {
            listener.stop.store(true, Ordering::Release);
        }
    }
    for (key, binding) in eligible {
        if listeners.len() >= LISTENERS {
            break;
        }
        if listeners.contains_key(&key) {
            continue;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let handle = tasks.spawn_local(server.clone().listen(
            binding.clone(),
            key.2.clone(),
            stop.clone(),
            control.clone(),
        ));
        listeners.insert(key, Listener { task: handle, stop });
    }
}

async fn run(
    prepared: Prepared,
    profile: Arc<ProfileLease>,
    mut local: mpsc::Receiver<PendingStart>,
    bindings: mpsc::Sender<BindingRequest>,
    latest: watch::Receiver<Snapshot>,
    stop: Arc<AtomicBool>,
) -> Result<()> {
    let mut native = Native::start(prepared.server, profile);
    let client_rooms = Arc::new(Semaphore::new(ROOMS));
    let (completed, mut attachments) = mpsc::channel(ROOMS);
    let server = Arc::new(Server {
        route: prepared.route.clone(),
        latest,
        stop: stop.clone(),
        rooms: Arc::new(Semaphore::new(ROOMS)),
        completed,
        ledger: Arc::default(),
    });
    let mut listeners = BTreeMap::new();
    let mut tasks = JoinSet::new();
    let mut interval = tokio::time::interval(POLL);
    while !stop.load(Ordering::Acquire) {
        tokio::select! {
            _ = interval.tick() => {
                native.poll();
                if let Some(supervisor) = &native.ready {
                    manage_listeners(&server, &supervisor.control, &mut listeners, &mut tasks);
                }
            }
            Some(outcome) = tasks.join_next(), if !tasks.is_empty() => {
                if outcome.is_err() {
                    crate::diagnostics::event("terminal_owner", "task_failed");
                }
            }
            Some(start) = local.recv() => {
                let Ok(start) = start.register() else { continue; };
                tasks.spawn_local(client_start(start, bindings.clone(), prepared.route.clone(), client_rooms.clone(), stop.clone()));
            }
            Some(attachment) = attachments.recv() => {
                let stop = stop.clone();
                tasks.spawn_local(async move { let _ = stream::server(attachment, &stop).await; });
            }
        }
    }
    local.close();
    attachments.close();
    while let Ok(attachment) = attachments.try_recv() {
        let _ = attachment.close().await;
    }
    let native_cleanup = native.finish();
    let transport_cleanup = async { while tasks.join_next().await.is_some() {} };
    let (native, _) = tokio::join!(native_cleanup, transport_cleanup);
    let transport = prepared.route.finish().await;
    native.and(transport)
}

async fn client_start(
    start: LocalStart,
    bindings: mpsc::Sender<BindingRequest>,
    route: Route,
    rooms: Arc<Semaphore>,
    stop: Arc<AtomicBool>,
) {
    let permit = match start.reserve() {
        Ok(permit) => permit,
        Err(error) => {
            start.refuse(category(&error)).await;
            return;
        }
    };
    let local_closed = Arc::new(AtomicBool::new(false));
    let pending = async {
        let deadline = start.deadline.min(Instant::now() + Duration::from_secs(1));
        let (reply, response) = oneshot::channel();
        bindings
            .try_send(BindingRequest {
                peer: decode(&start.request.remote)?,
                relays: start.request.relays.clone(),
                session: decode(&start.request.session)?,
                deadline,
                reply,
            })
            .map_err(|_| BINDING)?;
        let verified = timeout_at(deadline, response)
            .await
            .map_err(|_| BINDING_DEADLINE)?
            .map_err(|_| BINDING)??
            .ok_or(BINDING)?;
        if local_closed.load(Ordering::Acquire) {
            return Err(Error::Invalid("terminal_start_closed"));
        }
        let deadline = start.deadline.min(Instant::now() + Duration::from_secs(30));
        let endpoint = route.endpoint(&verified).ok_or(TRANSPORT)?;
        let fence = verified.fence.clone();
        let stopping = stop.clone();
        let closed = local_closed.clone();
        let grant: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
            !closed.load(Ordering::Acquire)
                && !stopping.load(Ordering::Acquire)
                && fence.state() != FenceState::Retired
        });
        let room = route
            .outbound(
                &verified,
                &endpoint,
                deadline,
                &rooms,
                grant.clone(),
                local_closed.clone(),
            )
            .await?;
        selection::request(
            room,
            if start.request.attach == Attach::New {
                Choice::New
            } else {
                Choice::Default
            },
            Size {
                rows: start.request.rows,
                cols: start.request.columns,
            },
            &|| grant(),
        )
        .await
    };
    let result = {
        tokio::pin!(pending);
        tokio::select! {
            biased;
            _ = start.cancelled() => {
                local_closed.store(true, Ordering::Release);
                if let Ok(selection::ClientOutcome::Attached { room, .. }) = pending.await {
                    let _ = room.close().await;
                }
                return;
            }
            result = &mut pending => result,
        }
    };
    match result {
        Ok(selection::ClientOutcome::Attached { room, .. }) => {
            match start.admit_reserved(permit).await {
                Ok(socket) => {
                    let _ = stream::client(room, socket, &stop).await;
                }
                Err(_) => {
                    let _ = room.close().await;
                }
            }
        }
        Ok(selection::ClientOutcome::Busy) => start.refuse("terminal_busy").await,
        Ok(selection::ClientOutcome::Refused(refusal)) => {
            use crate::terminal_backend::supervisor::live::selection_wire::Refusal;
            start
                .refuse(match refusal {
                    Refusal::Capacity => "terminal_capacity",
                    Refusal::Permission => "terminal_grant_denied",
                    Refusal::Unavailable => "terminal_owner_unavailable",
                })
                .await;
        }
        Err(error) => start.refuse(category(&error)).await,
    }
}

fn decode<const N: usize>(value: &str) -> Result<[u8; N]> {
    let mut bytes = [0; N];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| BINDING)?;
    Ok(bytes)
}

pub(crate) fn category(error: &Error) -> &'static str {
    match error {
        Error::Invalid("terminal_capacity") => "terminal_capacity",
        Error::Invalid("terminal_mode_unavailable") => "terminal_mode_unavailable",
        Error::Invalid("terminal_command_binding") => "terminal_command_binding",
        Error::Invalid("terminal_binding_deadline") => {
            crate::diagnostics::event("terminal_binding", "deadline");
            "terminal_attach_deadline"
        }
        Error::Invalid("command_unauthorized" | "terminal_enrollment_grant") => {
            "terminal_grant_denied"
        }
        Error::Timeout => "terminal_attach_deadline",
        Error::Transport("terminal_bootstrap_no_challenge") => "terminal_bootstrap_no_challenge",
        Error::Transport(_) => "terminal_transport_unavailable",
        _ => "terminal_owner_unavailable",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_runtime_core_stale_snapshot_and_listener_backoff() {
        let now = Instant::now();
        let snapshot = Snapshot {
            at: Some(now),
            routes: Vec::new(),
        };
        assert!(!Snapshot::default().fresh_at(now));
        assert!(snapshot.fresh_at(now + SNAPSHOT_VALID - Duration::from_millis(1)));
        assert!(!snapshot.fresh_at(now + SNAPSHOT_VALID));
        assert_eq!(
            [0, 1, 2, 3, 4, u8::MAX].map(|failures| listener_delay(failures).as_millis()),
            [25, 250, 500, 1000, 2000, 2000]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_runtime_core_listener_registration_is_paced() {
        use std::cell::{Cell, RefCell};

        let ledger = Arc::new(AdmissionLedger::default());
        let endpoint = "wss://relay.example/";
        let mut occupied = (0..64u8)
            .map(|id| InboundState::register(ledger.clone(), [id; 32], endpoint.into()).unwrap())
            .collect::<Vec<_>>();
        let eligible = Cell::new(true);
        let attempts = RefCell::new(Vec::new());
        let reports = RefCell::new(Vec::new());
        let start = Instant::now();
        let elapsed = || start.elapsed().as_millis();
        let registration = await_listener_registration(
            || true,
            || eligible.get(),
            || {
                attempts.borrow_mut().push(elapsed());
                InboundState::register(ledger.clone(), [64; 32], endpoint.into())
            },
            || reports.borrow_mut().push(elapsed()),
        );
        tokio::pin!(registration);
        assert!(futures_util::poll!(registration.as_mut()).is_pending());
        for (index, attempt) in [250u64, 750, 1750, 3750, 5750].into_iter().enumerate() {
            let delay = attempt - elapsed() as u64;
            tokio::time::advance(Duration::from_millis(delay - 1)).await;
            assert!(futures_util::poll!(registration.as_mut()).is_pending());
            assert_eq!(attempts.borrow().len(), index + 1);
            tokio::time::advance(Duration::from_millis(1)).await;
            assert!(futures_util::poll!(registration.as_mut()).is_pending());
            assert_eq!(attempts.borrow().last(), Some(&(attempt as u128)));
        }
        assert_eq!(*attempts.borrow(), [0, 250, 750, 1750, 3750, 5750]);
        assert_eq!(*reports.borrow(), [0]);
        for expected in (7750u128..=31750).step_by(2000) {
            tokio::time::advance(Duration::from_secs(2)).await;
            assert!(futures_util::poll!(registration.as_mut()).is_pending());
            assert_eq!(attempts.borrow().last(), Some(&expected));
            if expected < 30_000 {
                assert_eq!(*reports.borrow(), [0]);
            }
        }
        assert_eq!(*reports.borrow(), [0, 31750]);

        eligible.set(false);
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(futures_util::poll!(registration.as_mut()).is_pending());
        assert_eq!(attempts.borrow().last(), Some(&31750));
        drop(occupied.pop());
        eligible.set(true);
        tokio::time::advance(POLL).await;
        assert!(matches!(
            futures_util::poll!(registration.as_mut()),
            std::task::Poll::Ready(Some(_))
        ));
        assert_eq!(attempts.borrow().last(), Some(&33775));
        assert_eq!(*reports.borrow(), [0, 31750]);
    }

    #[tokio::test(start_paused = true)]
    async fn terminal_runtime_core_listener_eviction_ends_registration_wait() {
        use std::cell::Cell;

        let current = Cell::new(true);
        let eligible = Cell::new(false);
        let attempts = Cell::new(0);
        let registration = await_listener_registration(
            || current.get(),
            || eligible.get(),
            || {
                attempts.set(attempts.get() + 1);
                Err(Error::Invalid("terminal_bootstrap_rate"))
            },
            || (),
        );
        tokio::pin!(registration);
        assert!(futures_util::poll!(registration.as_mut()).is_pending());
        assert_eq!(attempts.get(), 0);
        eligible.set(true);
        tokio::time::advance(POLL).await;
        assert!(futures_util::poll!(registration.as_mut()).is_pending());
        assert_eq!(attempts.get(), 1);
        eligible.set(false);
        tokio::time::advance(POLL).await;
        eligible.set(true);
        assert!(futures_util::poll!(registration.as_mut()).is_pending());
        assert_eq!(attempts.get(), 1);

        current.set(false);
        tokio::time::advance(Duration::from_millis(250)).await;
        assert!(matches!(
            futures_util::poll!(registration.as_mut()),
            std::task::Poll::Ready(None)
        ));
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn terminal_runtime_core_room_permits_cover_retirement() {
        let rooms = Arc::new(Semaphore::new(ROOMS));
        let mut live = (0..16)
            .map(|_| rooms.clone().try_acquire_owned().unwrap())
            .collect::<Vec<_>>();
        let retiring = live.pop().unwrap();
        assert!(rooms.clone().try_acquire_owned().is_err());
        drop(retiring);
        let successor = rooms.clone().try_acquire_owned().unwrap();
        assert!(rooms.try_acquire().is_err());
        drop(successor);
        assert_eq!(live.len(), 15);
    }
}
