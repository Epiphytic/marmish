//! Private installed-layout qualification of a registered Command daemon.
use super::{LoadedProfile, PreparedProfile};
use crate::{
    Error, Result,
    command_config::CommandConfig,
    command_engine::Admission,
    command_limits::ServiceLifetime,
    daemon_launch::{
        StartupReporter,
        candidate_child::{ChildChannel, Interrupt},
    },
    dispatcher_fatal::Fatal,
    identity::{IdentityMaterial, ProfileLease},
    ipc::{self, Pending, Request, Response},
    mdk_adapter::MdkOwner,
    peer_candidate_startup::{self, Startup},
    peer_invitation::installed_recipient::{Actor, Completion, Exit, PhysicalEnd, Service},
    peer_relay_trust::candidate_registration::{
        ReceiverPreflight, RegisteredReceiver, RegistrationFailure,
    },
    transport::RelayConnector,
};
use std::{
    future::{Future, poll_fn},
    io::Write,
    num::NonZeroU64,
    path::Path,
    pin::{Pin, pin},
    sync::Arc,
    task::Context,
    time::Duration,
};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::{Instant, MissedTickBehavior},
};

enum Selection {
    Stock(Box<PreparedProfile>, RelayConnector),
    Registered(Box<RegisteredStartup>),
    Indeterminate(Arc<ProfileLease>),
}

struct RegisteredStartup {
    lease: Arc<ProfileLease>,
    receiver: RegisteredReceiver,
    connector: RelayConnector,
    material: IdentityMaterial,
    peers: Vec<String>,
    commands: CommandConfig,
    lifetime: Arc<ServiceLifetime>,
    retention: Duration,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QualificationIntent {
    CommandOnly,
    PassiveRecipient,
    ManualRecipient,
    ManualApprovalRecipient,
}

impl QualificationIntent {
    fn is_recipient(self) -> bool {
        !matches!(self, Self::CommandOnly)
    }
}

struct RegisteredOwner {
    lease: Arc<ProfileLease>,
    receiver: Option<RegisteredReceiver>,
    recipient_choice: Option<(QualificationIntent, IdentityMaterial)>,
    recipient: Recipient,
    fatal: Fatal,
    admission: Option<Admission>,
    owner: Option<Box<MdkOwner>>,
}

fn eligible(loaded: &LoadedProfile) -> bool {
    use crate::command_config::peer_intent::Receive;

    loaded.lease.was_ready_at_acquire()
        && !loaded.policy.needs_tor()
        && loaded
            .commands
            .as_ref()
            .is_some_and(|config| config.peer_receive() == Some(&Receive::Manual))
}

fn select(
    prepared: PreparedProfile,
    connector: RelayConnector,
    eligible_before_prepare: bool,
) -> Result<Selection> {
    let PreparedProfile { loaded, material } = prepared;
    if !eligible_before_prepare
        || connector.policy().needs_tor()
        || connector.policy().urls() != loaded.policy.urls()
        || connector.policy().mode() != loaded.policy.mode()
    {
        return Ok(Selection::Stock(
            Box::new(PreparedProfile { loaded, material }),
            connector,
        ));
    }
    let mut loaded = loaded;
    let commands = loaded
        .commands
        .take()
        .ok_or(Error::Invalid("commands_disabled"))?;
    let lifetime = Arc::new(loaded.origin.constrain(commands.limits().max_uptime)?);
    if lifetime.expired()? {
        return Err(Error::Timeout);
    }
    let lease = Arc::new(loaded.lease);
    let preflight = ReceiverPreflight::prepare(&lease, &material, &commands);
    if let Ok(Some(preflight)) = preflight {
        match preflight.register(&lease, &material) {
            Ok(receiver) => {
                return Ok(Selection::Registered(Box::new(RegisteredStartup {
                    lease,
                    receiver,
                    connector,
                    material,
                    peers: loaded.peers,
                    commands,
                    lifetime,
                    retention: loaded.retention,
                })));
            }
            Err(RegistrationFailure::Untouched) => {}
            Err(RegistrationFailure::LocalUnavailable) => {
                return Err(Error::Storage("candidate_registration_conflict"));
            }
            Err(RegistrationFailure::Indeterminate) => {
                return Ok(Selection::Indeterminate(lease));
            }
        }
    }
    let lease = Arc::try_unwrap(lease).map_err(|_| Error::Identity("candidate_startup_custody"))?;
    Ok(Selection::Stock(
        Box::new(PreparedProfile {
            loaded: LoadedProfile {
                lease,
                commands: Some(commands),
                ..loaded
            },
            material,
        }),
        connector,
    ))
}

impl RegisteredStartup {
    /// Returned resources are put in custody before the next admission.
    fn open(self, executable: &Path, intent: QualificationIntent) -> (RegisteredOwner, Result<()>) {
        let mut selected = RegisteredOwner {
            fatal: self.receiver.forwarding_verifier().fatal(),
            lease: self.lease,
            receiver: Some(self.receiver),
            recipient_choice: None,
            recipient: Recipient::default(),
            owner: None,
            admission: None,
        };
        let opened = (|| {
            let fence = selected
                .receiver
                .as_mut()
                .ok_or(Error::Identity("candidate_startup_custody"))?
                .take_command_fence()
                .map_err(|_| Error::Storage("candidate_dispatcher_lost"))?;
            let admission = Admission::from_registered(fence);
            selected.admission = Some(admission.clone());
            let connector = self.connector.with_fatal(selected.fatal.clone())?;
            let storage = admission.connection_fence();
            crate::native_preflight::check_with_fence(
                &selected.lease,
                &self.material,
                Some(&storage),
            )?;
            let owner = MdkOwner::open_candidate(
                &selected.lease,
                &self.material,
                connector,
                self.peers,
                admission.clone(),
            )?;
            let owner = selected.owner.insert(Box::new(owner));
            owner.set_retention(self.retention)?;
            owner.reconcile_client_inventory(Some(&self.commands))?;
            if !admission.admit() {
                return Err(Error::Storage("candidate_dispatcher_lost"));
            }
            selected
                .lease
                .check_opened_binding(owner.opened_binding())?;
            let layout = crate::native_layout::resolve(executable)?;
            owner.enable_candidate_commands(
                &selected.lease,
                &self.material,
                self.commands,
                layout,
                admission.clone(),
            )?;
            if !admission.admit() {
                return Err(Error::Storage("candidate_dispatcher_lost"));
            }
            owner.command_set_lifetime(self.lifetime);
            if intent.is_recipient() {
                selected.recipient_choice = Some((intent, self.material));
            }
            Ok(())
        })();
        (selected, opened)
    }
}

#[derive(Default)]
struct Recipient {
    startup: Option<Startup>,
    actor: Option<Actor>,
    completion: Option<Completion>,
}

impl Recipient {
    fn start(
        &mut self,
        lease: &Arc<ProfileLease>,
        (intent, material): (QualificationIntent, IdentityMaterial),
        receiver: RegisteredReceiver,
        fatal: &Fatal,
    ) -> Result<ipc::CandidateListener> {
        let registered_fatal = receiver.forwarding_verifier().fatal();
        check_recipient_fatal(fatal, &registered_fatal)?;
        let (startup, registration, worker) = peer_candidate_startup::pair();
        let (listener, service) = match intent {
            QualificationIntent::PassiveRecipient => (
                ipc::CandidateListener::passive(
                    registered_fatal,
                    ipc::PassiveRegistration {
                        path: lease.socket_path(),
                        registration,
                    },
                ),
                Service::Passive,
            ),
            QualificationIntent::ManualRecipient | QualificationIntent::ManualApprovalRecipient => {
                let Some((mut offer, producer)) = ipc::peer_read_listener::Offer::new(
                    lease.socket_path(),
                    registration,
                    NonZeroU64::MIN,
                    registered_fatal.clone(),
                ) else {
                    return Ok(ipc::CandidateListener::registered(registered_fatal, None));
                };
                let approval = (intent == QualificationIntent::ManualApprovalRecipient)
                    .then(|| offer.pair_approval(lease.clone()));
                (
                    ipc::CandidateListener::registered(registered_fatal, Some(offer)),
                    Service::Manual(producer, approval),
                )
            }
            QualificationIntent::CommandOnly => {
                return Err(Error::Identity("candidate_startup_custody"));
            }
        };
        match Actor::spawn(worker, lease.clone(), material, receiver, service) {
            Ok(actor) => {
                self.startup = Some(startup);
                self.actor = Some(actor);
                Ok(listener)
            }
            Err(_) => {
                crate::diagnostics::event("registered_recipient", "worker_spawn_failed");
                Ok(ipc::CandidateListener::registered(fatal.clone(), None))
            }
        }
    }

    fn stop(&self) {
        if let Some(startup) = &self.startup {
            startup.stop();
        }
        if let Some(actor) = &self.actor {
            actor.stop();
        }
    }

    async fn wait_until(&mut self, deadline: Instant) {
        if let Some(actor) = self.actor.as_mut()
            && self.completion.is_none()
            && let Ok(completion) = actor.join_before(deadline).await
        {
            self.completion = Some(completion);
            let category = match completion.exit {
                Exit::Stopped => "stopped",
                Exit::LocalUnavailable => "local_unavailable",
                Exit::GlobalFault => "dispatcher_lost",
                Exit::WorkerFailed => "worker_failed",
            };
            crate::diagnostics::event("registered_recipient", category);
        }
    }

    fn pending(&self) -> bool {
        self.actor.is_some() && self.completion.is_none()
    }

    fn unproved(&self) -> bool {
        self.completion
            .is_some_and(|completion| completion.physical == PhysicalEnd::Unproved)
    }
}

fn check_recipient_fatal(fatal: &Fatal, registered_fatal: &Fatal) -> Result<()> {
    fatal
        .same_lifetime(registered_fatal)
        .then_some(())
        .ok_or(Error::Identity("candidate_startup_custody"))
}

#[derive(Default)]
struct Listener {
    drain: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<()>>>,
    outcome: Option<std::result::Result<Result<()>, tokio::task::JoinError>>,
}

impl Listener {
    fn start(
        &mut self,
        lease: &ProfileLease,
        registered: ipc::CandidateListener,
    ) -> Result<(mpsc::Receiver<Pending>, watch::Sender<Response>)> {
        let listener = ipc::bind(&lease.socket_path())?;
        let (drain, drained) = oneshot::channel();
        let (queue, requests) = mpsc::channel(64);
        let (status, snapshots) = watch::channel(Response::success(
            serde_json::json!({"state": "activating"}),
        ));
        let task = tokio::spawn(async move {
            ipc::serve_with_listener(
                listener,
                queue,
                snapshots,
                None,
                #[cfg(test)]
                None,
                Some(drained),
                #[cfg(test)]
                None,
                registered,
            )
            .await
        });
        self.drain = Some(drain);
        self.task = Some(task);
        Ok((requests, status))
    }

    async fn exited(&mut self) {
        poll_fn(|cx| {
            if self.poll_exit(cx) {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await
    }

    fn poll_exit(&mut self, cx: &mut Context<'_>) -> bool {
        if let Some(task) = self.task.as_mut()
            && let std::task::Poll::Ready(outcome) = Pin::new(task).poll(cx)
        {
            self.outcome = Some(outcome);
            self.task = None;
        }
        self.outcome.is_some()
    }

    async fn wait_until(&mut self, deadline: Instant) {
        if let Some(task) = self.task.as_mut()
            && let Ok(outcome) = tokio::time::timeout_at(deadline, task).await
        {
            self.outcome = Some(outcome);
            self.task = None;
        }
    }

    fn exit_error(&mut self) -> Error {
        match self.outcome.as_mut() {
            Some(Ok(Err(error))) => {
                std::mem::replace(error, Error::Invalid("ipc_listener_stopped"))
            }
            Some(Err(_)) => Error::Identity("owned_stop_incomplete"),
            _ => Error::Invalid("ipc_listener_stopped"),
        }
    }

    fn start_drain(&mut self) {
        if let Some(drain) = self.drain.take() {
            let _ = drain.send(());
        }
    }
}

fn admit(admission: &Admission) -> Result<()> {
    if !admission.admit() {
        return Err(Error::Storage("candidate_dispatcher_lost"));
    }
    Ok(())
}

async fn finite_dispatch(
    owner: &mut MdkOwner,
    profile: &str,
    rooms: &mut crate::command_local::RoomIntents,
    request: Request,
) -> Result<Response> {
    match request {
        Request::CommandPrepare { .. }
        | Request::CommandSubmit { .. }
        | Request::CommandStatus { .. }
        | Request::CommandCancel { .. }
        | Request::CommandPoll { .. }
        | Request::CommandResponse { .. }
        | Request::CommandCursors { .. }
        | Request::CommandUnconsumed { .. }
        | Request::CommandInvocations { .. }
        | Request::CommandConsume { .. } => super::dispatch(owner, profile, rooms, request).await,
        Request::ClientInventory { request } => Ok(Response::inventory(
            owner.checked_client_inventory(request)?,
        )),
        _ => Err(Error::Invalid("registered_command_only")),
    }
}

#[cfg(test)]
#[path = "peer_daemon_registered_loss_tests.rs"]
pub(crate) mod registered_loss;

#[cfg(test)]
#[path = "peer_daemon_registered_manual_listener_tests.rs"]
mod manual_listener_tests;

impl RegisteredOwner {
    async fn serve(
        &mut self,
        listener: &mut Listener,
        channel: &mut Option<ChildChannel>,
    ) -> Result<()> {
        let admission = self
            .admission
            .as_ref()
            .ok_or(Error::Storage("candidate_dispatcher_lost"))?;
        let owner = self
            .owner
            .as_mut()
            .ok_or(Error::Identity("candidate_startup_custody"))?;
        let lifetime = owner
            .command_lifetime()
            .ok_or(Error::Identity("candidate_startup_custody"))?;
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut shutdown = pin!(async {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
        });
        let mut expired = pin!(async {
            while !lifetime.expired().unwrap_or(true) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        admit(admission)?;
        #[cfg(test)]
        registered_loss::own_admission(admission);
        let registered_listener = match self.recipient_choice.take() {
            Some(choice) => {
                let receiver = self
                    .receiver
                    .take()
                    .ok_or(Error::Identity("candidate_startup_custody"))?;
                self.recipient
                    .start(&self.lease, choice, receiver, &self.fatal)?
            }
            None => ipc::CandidateListener::registered(self.fatal.clone(), None),
        };
        let (mut requests, status) = listener.start(&self.lease, registered_listener)?;
        let fatal = &self.fatal;
        let mut interrupt = |cx: &mut Context<'_>| {
            if shutdown.as_mut().poll(cx).is_ready() {
                Some(Interrupt::Shutdown)
            } else if expired.as_mut().poll(cx).is_ready() {
                Some(Interrupt::Expired)
            } else if listener.poll_exit(cx) {
                Some(Interrupt::ListenerExited)
            } else {
                None
            }
        };

        let mut stopped = pin!(fatal.tripped());
        tokio::select! {
            biased;
            _ = stopped.as_mut() => return Err(Error::Storage("candidate_dispatcher_lost")),
            reason = poll_fn(|cx| match interrupt(cx) {
                Some(reason) => std::task::Poll::Ready(reason),
                None => std::task::Poll::Pending,
            }) => return Err(interruption(reason, listener)),
            outcome = owner.activate(Instant::now() + Duration::from_secs(150)) => outcome?,
        }
        // The status published after C is computed under the same admission.
        // Any post-C cut suppresses the status and proceeds to live shutdown.
        admit(admission)?;
        let mut snapshot =
            serde_json::to_value(owner.status()?).map_err(|_| Error::Invalid("status_encoding"))?;
        snapshot["state"] = serde_json::json!("active");
        if let Some(channel) = channel.take() {
            let result = channel.exchange(admission, fatal, &mut interrupt).await;
            if let Err(failure) = result {
                #[cfg(test)]
                registered_loss::readiness_failure(failure);
                return Err(report_failure(failure, listener));
            }
        } else {
            let reason = poll_fn(|cx| std::task::Poll::Ready(interrupt(cx))).await;
            if let Some(reason) = reason {
                return Err(interruption(reason, listener));
            }
            admit(admission)?;
        }
        admit(admission)?;
        status.send_replace(Response::success(snapshot));
        if let Some(startup) = self.recipient.startup.as_ref() {
            let reason = poll_fn(|cx| std::task::Poll::Ready(interrupt(cx))).await;
            if let Some(reason) = reason {
                return Err(interruption(reason, listener));
            }
            if lifetime.expired()? {
                return Err(Error::Timeout);
            }
            admit(admission)?;
            startup.ordinary_ready();
        }
        let profile = self
            .lease
            .root()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(Error::Invalid("identity_name"))?;

        let mut rooms = crate::command_local::RoomIntents::default();
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            admit(admission)?;
            if owner.command_count_exit_ready()? {
                return Ok(());
            }
            let work = async {
                tokio::select! {
                    pending = requests.recv() => pending.map(Some)
                        .ok_or(Error::Invalid("ipc_listener_stopped")),
                    _ = interval.tick() => Ok(None),
                }
            };
            // Stops are prioritized independently of the fair selection
            // between local requests and transport progress.
            let next = tokio::select! {
                biased;
                _ = stopped.as_mut() => return Err(Error::Storage("candidate_dispatcher_lost")),
                _ = shutdown.as_mut() => return Ok(()),
                _ = expired.as_mut() => return Ok(()),
                _ = listener.exited() => return Err(listener.exit_error()),
                work = work => work?,
            };
            match next {
                Some(pending) => {
                    admit(admission)?;
                    if pending.deadline <= Instant::now() || pending.respond.is_closed() {
                        continue;
                    }
                    let reply = tokio::select! {
                        biased;
                        _ = stopped.as_mut() => return Err(Error::Storage("candidate_dispatcher_lost")),
                        _ = shutdown.as_mut() => return Ok(()),
                        _ = expired.as_mut() => return Ok(()),
                        _ = listener.exited() => return Err(listener.exit_error()),
                        result = finite_dispatch(owner, profile, &mut rooms, pending.request) => result,
                    };
                    admit(admission)?;
                    if matches!(reply, Err(Error::Storage("candidate_dispatcher_lost"))) {
                        return reply.map(|_| ());
                    }
                    let _ = pending.respond.send(reply.unwrap_or_else(super::failure));
                }
                None => {
                    let turn = tick(owner, &mut rooms);
                    let degraded = tokio::select! {
                        biased;
                        _ = stopped.as_mut() => return Err(Error::Storage("candidate_dispatcher_lost")),
                        _ = shutdown.as_mut() => return Ok(()),
                        _ = expired.as_mut() => return Ok(()),
                        _ = listener.exited() => return Err(listener.exit_error()),
                        result = turn => result?,
                    };
                    admit(admission)?;
                    let mut snapshot = serde_json::to_value(owner.status()?)
                        .map_err(|_| Error::Invalid("status_encoding"))?;
                    snapshot["state"] = serde_json::json!(if degraded {
                        "transport_degraded"
                    } else {
                        "active"
                    });
                    admit(admission)?;
                    status.send_replace(Response::success(snapshot));
                }
            }
        }
    }
}

async fn tick(owner: &mut MdkOwner, rooms: &mut crate::command_local::RoomIntents) -> Result<bool> {
    let degraded = match owner.tick(Instant::now() + Duration::from_secs(15)).await {
        Ok(()) => false,
        Err(Error::Transport(_) | Error::Timeout) => true,
        Err(error) => return Err(error),
    };
    if let Some(remote) = rooms.next(Instant::now()) {
        let deadline =
            Instant::now() + Duration::from_secs(crate::command_local::ROOM_TURN_SECONDS);
        if let Err(error) = owner.command_open_room_turn(remote, deadline).await {
            if matches!(error, Error::Storage("candidate_dispatcher_lost")) {
                return Err(error);
            }
            rooms.failed(remote, Instant::now(), super::room_failure_category(&error));
        }
    }
    Ok(degraded)
}

fn interruption(reason: Interrupt, listener: &mut Listener) -> Error {
    match reason {
        Interrupt::Shutdown => Error::Invalid("candidate_startup_stopped"),
        Interrupt::Expired => Error::Timeout,
        Interrupt::ListenerExited => listener.exit_error(),
    }
}

fn report_failure(
    failure: crate::daemon_launch::candidate_child::Failure,
    listener: &mut Listener,
) -> Error {
    use crate::daemon_launch::candidate_child::Failure;
    match failure {
        Failure::Fatal | Failure::Admission => Error::Storage("candidate_dispatcher_lost"),
        Failure::Interrupted(reason) => interruption(reason, listener),
        Failure::TimedOut => Error::Timeout,
        Failure::Acknowledgement | Failure::ParentEof | Failure::Socket(_) => {
            Error::DaemonStartup("registered readiness exchange failed".into())
        }
    }
}

pub(crate) async fn run(
    config: super::Config,
    startup: Option<StartupReporter>,
    executable: &Path,
    fixture_roots: Option<Vec<rustls::pki_types::CertificateDer<'static>>>,
    intent: QualificationIntent,
) -> Result<()> {
    let mut channel = startup.as_ref().map(ChildChannel::take).transpose()?;
    let origin = ServiceLifetime::start(None)?;
    let (prepared, eligible) = tokio::task::spawn_blocking(move || {
        let loaded = LoadedProfile::load(config, origin)?;
        let eligible = eligible(&loaded);
        Ok::<_, Error>((loaded.prepare()?, eligible))
    })
    .await
    .map_err(|_| Error::Identity("initialization_worker"))??;
    let connector = match fixture_roots {
        None => prepared.relay_connector()?,
        Some(certificates) => {
            // LoadedProfile has verified the configured Command relay and the
            // explicit relay policy agree before any identity preparation.
            let policy = prepared.loaded.policy.clone();
            let [url] = policy.urls() else {
                return Err(Error::Invalid("fixture_relay_trust"));
            };
            let url = url.clone();
            let mut roots = rustls::RootCertStore::empty();
            for certificate in certificates {
                roots
                    .add(certificate)
                    .map_err(|_| Error::Invalid("fixture_relay_trust"))?;
            }
            let lease = &prepared.loaded.lease;
            RelayConnector::for_loopback_fixture(
                policy,
                crate::transport::ArtiDirs {
                    state: lease.arti_state().to_owned(),
                    cache: lease.arti_cache().to_owned(),
                },
                url,
                roots,
            )?
        }
    };
    match select(prepared, connector, eligible)? {
        Selection::Stock(prepared, connector) => {
            if channel.is_some() || intent.is_recipient() {
                return Err(Error::Invalid("candidate_startup_not_registered"));
            }
            let opened = super::open_stock(
                *prepared,
                connector,
                true,
                Some(executable),
                #[cfg(test)]
                None,
            )?;
            super::serve_opened(
                opened,
                None,
                #[cfg(test)]
                None,
                #[cfg(test)]
                None,
            )
            .await
        }
        Selection::Indeterminate(lease) => {
            drop(channel);
            crate::diagnostics::event("registered_shutdown", "registration_indeterminate");
            std::mem::forget(lease);
            std::process::exit(1);
        }
        Selection::Registered(registered) => {
            let (mut custody, result) = registered.open(executable, intent);
            let mut listener = Listener::default();
            let result = match result {
                Ok(()) => custody.serve(&mut listener, &mut channel).await,
                Err(error) => Err(error),
            };
            if let Some((channel, admission)) = channel.zip(custody.admission.as_ref()) {
                if let Err(error) = &result {
                    let mut log = std::io::stderr().lock();
                    let _ = match error {
                        Error::DaemonStartup(_) => {
                            writeln!(log, "marmish qualification startup failed")
                        }
                        _ => writeln!(log, "marmish qualification startup: {error}"),
                    };
                }
                channel.fail_before_offer(admission);
            }
            finish(custody, listener, result).await
        }
    }
}

async fn finish(
    mut custody: RegisteredOwner,
    mut listener: Listener,
    result: Result<()>,
) -> Result<()> {
    use crate::mdk_adapter::healthy_stop::RegisteredStop;

    let began = Instant::now();
    custody.recipient.stop();
    listener.start_drain();
    let Some(owner) = custody.owner.take() else {
        // A canceled or failed guarded open owns its partial resources. Its
        // startup adapter has disposed or parked each root and notifier.
        crate::diagnostics::event("registered_shutdown", "native_startup_failed");
        std::mem::forget(custody);
        std::process::exit(1);
    };
    let Some(admission) = custody.admission.as_ref() else {
        retain_unproved((custody, listener, owner)).await;
    };
    let mut stop =
        match RegisteredStop::begin(owner, &custody.lease, admission, began, result.is_ok()) {
            Ok(stop) => stop,
            Err(owner) => retain_unproved((custody, listener, owner)).await,
        };

    let mut last_notice = None;
    loop {
        let deadline = Instant::now() + Duration::from_secs(1);
        let (owner, (), ()) = tokio::join!(
            stop.observe(deadline),
            listener.wait_until(deadline),
            custody.recipient.wait_until(deadline),
        );
        if custody.recipient.unproved() {
            retain_unproved((custody, listener, stop)).await;
        }
        if listener.task.is_none()
            && !custody.recipient.pending()
            && !owner.physical.requires_process_hold()
        {
            let listener_clean = listener
                .outcome
                .as_ref()
                .is_none_or(|outcome| matches!(outcome, Ok(Ok(()))));
            if listener_clean && owner.graceful_complete && !owner.fatal {
                match stop.complete(|| custody.lease.cleanup_runtime()) {
                    Ok(()) => return result,
                    Err(parked) => stop = *parked,
                }
            }
            crate::diagnostics::event("registered_shutdown", "owned_stop_failed");
            #[cfg(test)]
            registered_loss::before_fatal_exit(
                &owner,
                matches!(listener.outcome.as_ref(), Some(Ok(Ok(())))),
                owner.fatal,
            );
            std::mem::forget((custody, listener, stop));
            std::process::exit(1);
        }
        let now = Instant::now();
        if now.duration_since(began) >= Duration::from_secs(15)
            && last_notice.is_none_or(|then| now.duration_since(then) >= Duration::from_secs(30))
        {
            crate::diagnostics::event("registered_shutdown", "owned_stop_incomplete");
            last_notice = Some(now);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn retain_unproved<T>(custody: T) -> ! {
    std::mem::forget(custody);
    crate::diagnostics::event("registered_shutdown", "owned_stop_unproved");
    loop {
        std::future::pending::<()>().await;
    }
}
