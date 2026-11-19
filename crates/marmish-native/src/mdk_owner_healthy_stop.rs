//! Retain one registered native owner through ordinary drain or a physical cut.
use super::{
    MdkOwner,
    physical_cut::{Barrier, Report, Task, cancel_runners, observe_runners, stop_sdk},
};
use crate::{
    Error, Result,
    command_engine::{Admission, CommandEngine, StopOutcome},
    command_lifecycle::keep_fatal_engine,
    dispatcher_fatal::Fatal,
    identity::ProfileLease,
    stream_runner::RunnerHandle,
};
use cgka_traits::TransportAdapter;
use std::{mem::ManuallyDrop, sync::Arc, time::Duration};
use tokio::{
    runtime::Handle,
    task::JoinHandle,
    time::{Instant, timeout_at},
};

const ENGINE_TURN: Duration = Duration::from_millis(100);
const STREAM_GRACE: Duration = Duration::from_millis(100);

pub(crate) struct OwnerReport {
    pub(crate) physical: Report,
    pub(crate) graceful_complete: bool,
    pub(crate) fatal: bool,
}

#[must_use = "retain the exact owner and observe until its physical disposition is known"]
pub(crate) struct HealthyStop {
    owner: ManuallyDrop<Box<MdkOwner>>,
    lease: ManuallyDrop<Arc<ProfileLease>>,
    admission: Admission,
    fatal: Fatal,
    runtime: Handle,
    engine: Engine,
    runners: Vec<RunnerHandle>,
    runners_status: Barrier,
    transport: Option<TransportStop>,
    grace_attempted: bool,
    graceful: bool,
}

struct TransportStop {
    sdk: Task<()>,
    forwarder: Task<()>,
    raw: Barrier,
}

impl HealthyStop {
    pub(crate) fn begin(
        mut owner: Box<MdkOwner>,
        lease: &Arc<ProfileLease>,
        admission: &Admission,
    ) -> std::result::Result<Self, Box<MdkOwner>> {
        let Ok(runtime) = Handle::try_current() else {
            return Err(owner);
        };
        let Some(fatal) = owner.connector.dispatcher_fatal() else {
            return Err(owner);
        };
        if !admission.is_for(fatal)
            || !owner.active_mode.matches_stop_admission(admission)
            || !owner
                .startup_custody
                .as_ref()
                .is_some_and(|root| root.owns(lease))
            || owner.streams.stopping
            || owner
                .commands
                .as_mut()
                .and_then(|service| service.engine_mut())
                .is_some_and(|engine| !engine.matches_stop_admission(admission))
        {
            return Err(owner);
        }
        let fatal = fatal.clone();
        // Failed or indeterminate stops retain the actual owner and flock.
        let mut owner = ManuallyDrop::new(owner);
        let lease = ManuallyDrop::new(lease.clone());
        if let Some(commands) = &mut owner.commands {
            commands.close_admission();
        }
        owner.terminal_authority.retire_all();
        let runners = owner.stream_runner_barriers();
        for runner in &runners {
            runner.suspend_input();
        }
        let engine = owner
            .take_engine_for_stop()
            .map_or(Engine::NeverStarted, |engine| {
                Engine::InOwner(ManuallyDrop::new(Box::new(engine)))
            });
        Ok(Self {
            owner,
            lease,
            admission: admission.clone(),
            fatal,
            runtime,
            engine,
            runners,
            runners_status: Barrier::Pending,
            transport: None,
            grace_attempted: false,
            graceful: false,
        })
    }

    pub(crate) async fn observe(&mut self, deadline: Instant) -> OwnerReport {
        self.engine
            .start_turn(&self.fatal, &self.lease, &self.runtime, deadline);
        if self.transport.is_none() {
            if !self.grace_attempted {
                self.grace_attempted = true;
                let fatal = self.fatal.clone();
                self.graceful = Instant::now() < deadline
                    && tokio::select! {
                        biased;
                        _ = fatal.tripped() => false,
                        result = timeout_at(deadline, self.prepare_graceful(deadline)) => {
                            matches!(result, Ok(Ok(()))) && !fatal.is_tripped()
                        }
                    };
            }
            // These actions are physical and safe after a fatal cut. Stop
            // every independent resource before waiting for any of them.
            self.owner.publication.pause();
            self.owner.active = false;
            self.owner.streams.stopping = true;
            cancel_runners(&self.runners);
            self.owner.connector.cancel_terminal_connects();
            let (sdk, forwarder) = stop_sdk(&mut self.owner, &self.runtime);
            self.transport = Some(TransportStop {
                sdk,
                forwarder,
                raw: Barrier::Pending,
            });
        }
        let transport = self.transport.as_mut().expect("transport stop started");
        tokio::join!(
            self.engine
                .observe(&self.fatal, &self.lease, &self.runtime, deadline),
            observe_runners(&self.runners, &mut self.runners_status, deadline),
            transport.sdk.observe(deadline, |()| Barrier::Complete),
            transport
                .forwarder
                .observe(deadline, |()| Barrier::Complete),
            observe_raw(&self.owner, &mut transport.raw, deadline),
        );
        self.report()
    }

    async fn prepare_graceful(&mut self, deadline: Instant) -> Result<()> {
        let mode = self.owner.active_mode.clone();
        mode.admit()?;
        self.owner.stream_begin_stop()?;
        mode.admit()?;
        if !self.owner.stream_runners_finished() {
            let grace = deadline.min(Instant::now() + STREAM_GRACE);
            // An elapsed transport grace still permits physical cancellation.
            match timeout_at(grace, mode.native(self.owner.stream_stop_progress(grace))).await {
                Ok(Ok(Ok(())) | Ok(Err(Error::Timeout))) | Err(_) => {}
                Ok(Ok(Err(error)) | Err(error)) => return Err(error),
            }
            mode.admit()?;
        }
        self.owner.stream_cancel_stop()?;
        mode.admit()?;
        mode.native(self.owner.adapter.deactivate_account(&self.owner.account))
            .await?
            .map_err(|_| Error::Transport("deactivate"))?;
        Ok(())
    }

    fn report(&self) -> OwnerReport {
        let (sdk, forwarder, raw) = self.transport.as_ref().map_or(
            (Barrier::Pending, Barrier::Pending, Barrier::Pending),
            |transport| {
                (
                    transport.sdk.status,
                    transport.forwarder.status,
                    transport.raw,
                )
            },
        );
        let physical = Report {
            engine: self.engine.barrier(),
            runners: self.runners_status,
            sdk,
            forwarder,
            raw,
        };
        let fatal = self.fatal.is_tripped();
        let graceful_complete =
            !fatal && self.graceful && self.engine.quiesced() && physical.all_complete();
        OwnerReport {
            physical,
            graceful_complete,
            fatal,
        }
    }

    /// The caller has independently joined its drained listener. Only this
    /// method can release the registered owner and lease after all outcomes
    /// are known and the three checked cleanup effects have succeeded.
    pub(crate) fn complete(
        mut self,
        cleanup_runtime: impl FnOnce() -> Result<()>,
    ) -> std::result::Result<(), Box<Self>> {
        if !self.report().graceful_complete {
            return Err(Box::new(self));
        }
        // A partially completed cleanup is terminal, including on caller retry.
        self.graceful = false;
        if self.cleanup(cleanup_runtime).is_err() {
            return Err(Box::new(self));
        }
        drop(ManuallyDrop::into_inner(self.owner));
        drop(ManuallyDrop::into_inner(self.lease));
        Ok(())
    }

    fn cleanup(&mut self, cleanup_runtime: impl FnOnce() -> Result<()>) -> Result<()> {
        let mode = self.owner.active_mode.clone();
        mode.admit()?;
        // The Command-only candidate cannot launch a stream. Also refuse
        // cleanup when prior state or an unfinished projection still exists.
        if !self.owner.stream_stop_confirmed()? {
            return Err(Error::Storage("stream_stop_incomplete"));
        }
        mode.admit()?;
        self.owner.journal.close_in_place()?;
        mode.admit()?;
        self.owner
            .startup_custody
            .take()
            .ok_or(Error::Storage("shutdown_account_custody"))?
            .close()?;
        mode.admit()?;
        let cleanup = cleanup_runtime();
        mode.admit()?;
        cleanup
    }
}

type EngineCustody = ManuallyDrop<Box<CommandEngine>>;
type Turn = JoinHandle<(EngineCustody, Result<StopOutcome>)>;

enum Engine {
    NeverStarted,
    InOwner(EngineCustody),
    InTurn(Turn),
    Joined { quiesced: bool },
    FatalJoin(Task<Result<()>>),
    Uncertain(Option<EngineCustody>),
}

impl Engine {
    fn barrier(&self) -> Barrier {
        match self {
            Self::NeverStarted | Self::Joined { .. } => Barrier::Complete,
            Self::InOwner(_) | Self::InTurn(_) => Barrier::Pending,
            Self::FatalJoin(task) => task.status,
            Self::Uncertain(_) => Barrier::PossiblyLive,
        }
    }

    fn quiesced(&self) -> bool {
        matches!(self, Self::NeverStarted | Self::Joined { quiesced: true })
    }

    fn start_turn(
        &mut self,
        fatal: &Fatal,
        lease: &Arc<ProfileLease>,
        runtime: &Handle,
        deadline: Instant,
    ) {
        if fatal.is_tripped() {
            let retained = match self {
                Self::InOwner(_) | Self::Uncertain(Some(_)) => {
                    let old = std::mem::replace(self, Self::Uncertain(None));
                    match old {
                        Self::InOwner(engine) | Self::Uncertain(Some(engine)) => engine,
                        _ => unreachable!(),
                    }
                }
                _ => return,
            };
            *self = if retained.can_join_dispatcher_cut(fatal) {
                match ManuallyDrop::into_inner(retained).into_fatal_join() {
                    Ok(join) => {
                        Self::FatalJoin(Task::new(Some(keep_fatal_engine(join, lease.clone()))))
                    }
                    Err(engine) => Self::Uncertain(Some(ManuallyDrop::new(engine))),
                }
            } else {
                Self::Uncertain(Some(retained))
            };
        } else if Instant::now() < deadline && matches!(self, Self::InOwner(_)) {
            let Self::InOwner(mut engine) = std::mem::replace(self, Self::Uncertain(None)) else {
                unreachable!();
            };
            let slice = ENGINE_TURN.min(deadline.saturating_duration_since(Instant::now()));
            *self = Self::InTurn(runtime.spawn_blocking(move || {
                let outcome = engine.stop_outcome(std::time::Instant::now() + slice);
                (engine, outcome)
            }));
        }
    }

    async fn observe(
        &mut self,
        fatal: &Fatal,
        lease: &Arc<ProfileLease>,
        runtime: &Handle,
        deadline: Instant,
    ) {
        loop {
            self.start_turn(fatal, lease, runtime, deadline);
            match self {
                Self::InTurn(turn) => {
                    let Ok(result) = timeout_at(deadline, turn).await else {
                        return;
                    };
                    *self = match result {
                        Err(_) => Self::Uncertain(None),
                        Ok((engine, Ok(StopOutcome::Ordinary(report)))) if report.complete => {
                            if engine.worker_joined() {
                                drop(ManuallyDrop::into_inner(engine));
                                Self::Joined {
                                    quiesced: report.quiesced,
                                }
                            } else {
                                Self::Uncertain(Some(engine))
                            }
                        }
                        Ok((engine, Ok(StopOutcome::Ordinary(_)))) => Self::InOwner(engine),
                        Ok((engine, Ok(StopOutcome::Fatal))) if fatal.is_tripped() => {
                            Self::InOwner(engine)
                        }
                        Ok((engine, _)) => {
                            *self = Self::Uncertain(Some(engine));
                            return;
                        }
                    };
                }
                Self::FatalJoin(task) => {
                    task.observe(deadline, |result| {
                        if result.is_ok() {
                            Barrier::Complete
                        } else {
                            Barrier::PossiblyLive
                        }
                    })
                    .await;
                    return;
                }
                _ => return,
            }
        }
    }
}

async fn observe_raw(owner: &MdkOwner, barrier: &mut Barrier, deadline: Instant) {
    if *barrier == Barrier::Pending {
        *barrier = match owner.connector.reap_registered_sockets(deadline).await {
            Ok(()) => Barrier::Complete,
            Err(Error::Timeout) => Barrier::Pending,
            Err(_) => Barrier::PossiblyLive,
        };
    }
}
