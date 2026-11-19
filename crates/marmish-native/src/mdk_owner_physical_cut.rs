//! Physical shutdown of an ordinary owner after registered dispatcher loss.
use super::MdkOwner;
use crate::{
    Error, Result, command_engine::CommandEngine, command_lifecycle::keep_fatal_engine,
    identity::ProfileLease,
    stream_runner::{ReapObservation, RunnerHandle},
    transport::RelayConnector,
};
use std::{mem::ManuallyDrop, sync::Arc, time::Duration};
use tokio::{
    runtime::Handle,
    task::JoinHandle,
    time::{Instant, sleep_until, timeout_at},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Barrier {
    Pending,
    Complete,
    Incomplete,
    PossiblyLive,
}

fn engine_barrier(result: Result<()>) -> Barrier {
    match result {
        Ok(()) => Barrier::Complete,
        Err(_) => Barrier::PossiblyLive,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Report {
    pub(crate) engine: Barrier,
    pub(crate) runners: Barrier,
    pub(crate) sdk: Barrier,
    pub(crate) forwarder: Barrier,
    pub(crate) raw: Barrier,
}

impl Report {
    pub(crate) fn requires_process_hold(self) -> bool {
        [
            self.engine,
            self.runners,
            self.sdk,
            self.forwarder,
            self.raw,
        ]
        .into_iter()
        .any(|barrier| matches!(barrier, Barrier::Pending | Barrier::PossiblyLive))
    }
}

struct Task<T> {
    status: Barrier,
    handle: Option<JoinHandle<T>>,
}

impl<T> Task<T> {
    fn new(handle: Option<JoinHandle<T>>) -> Self {
        let status = if handle.is_some() {
            Barrier::Pending
        } else {
            Barrier::Complete
        };
        Self { status, handle }
    }

    async fn observe(&mut self, deadline: Instant, check: fn(T) -> Barrier) {
        let Some(handle) = self.handle.as_mut() else {
            return;
        };
        if let Ok(joined) = timeout_at(deadline, handle).await {
            self.status = joined.map_or(Barrier::PossiblyLive, check);
            self.handle = None;
        }
    }
}

// Candidate shutdown may have lost the SQLite dispatcher. These objects have
// no release operation even when all physical barriers settle. The daemon must
// retain them through fatal process exit.
#[must_use]
pub(crate) struct ParkedOrdinary {
    owner: ManuallyDrop<Box<MdkOwner>>,
    lease: ManuallyDrop<Arc<ProfileLease>>,
    refused_engine: ManuallyDrop<Option<Box<CommandEngine>>>,
}

#[must_use]
pub(crate) struct PhysicalCut {
    parked: ParkedOrdinary,
    engine: Task<Result<()>>,
    runners: Vec<RunnerHandle>,
    runners_status: Barrier,
    sdk: Task<()>,
    forwarder: Task<()>,
    raw: Barrier,
}

impl PhysicalCut {
    /// The custody owner must supply the Arc from which it opened this exact
    /// owner. The profile identifier here is a consistency check only.
    pub(super) fn begin(
        mut owner: Box<MdkOwner>,
        lease: &Arc<ProfileLease>,
    ) -> std::result::Result<Self, Box<MdkOwner>> {
        let Ok(runtime) = Handle::try_current() else {
            return Err(owner);
        };
        let Some(fatal) = owner.connector.dispatcher_fatal() else {
            return Err(owner);
        };
        if !fatal.is_tripped()
            || owner.streams.stopping
            || owner.binding.profile_id() != lease.profile_id()
        {
            return Err(owner);
        }
        if owner
            .commands
            .as_mut()
            .and_then(|service| service.engine_mut())
            .is_some_and(|engine| !engine.can_join_dispatcher_cut(fatal))
        {
            return Err(owner);
        }

        // Install the destructor fence before running any cut action.
        let mut parked = ParkedOrdinary {
            owner: ManuallyDrop::new(owner),
            lease: ManuallyDrop::new(lease.clone()),
            refused_engine: ManuallyDrop::new(None),
        };
        let owner = &mut *parked.owner;
        owner.terminal_authority.retire_all();
        owner.publication.pause();
        owner.active = false;
        // The graceful stream_begin_stop writes the Journal and is forbidden.
        owner.streams.stopping = true;
        if let Some(service) = &mut owner.commands {
            service.close_admission();
        }

        let runners = owner.stream_runner_barriers();
        for runner in &runners {
            runner.suspend_input();
            runner.cancel();
        }
        owner.connector.cancel_terminal_connects();

        let mut engine = Task::new(None);
        if let Some(adapter) = owner
            .commands
            .as_mut()
            .and_then(|service| service.take_engine_for_stop())
        {
            match adapter.into_fatal_join() {
                Ok(join) => engine = Task::new(Some(keep_fatal_engine(join, lease.clone()))),
                Err(adapter) => {
                    *parked.refused_engine = Some(adapter);
                    engine.status = Barrier::PossiblyLive;
                }
            }
        }
        let relay = owner.relay.clone();
        let sdk = Task::new(Some(runtime.spawn(async move { relay.shutdown().await })));
        let forwarder = Task::new(owner.forwarder.take());
        let runners_status = if runners.is_empty() {
            Barrier::Complete
        } else {
            Barrier::Pending
        };
        Ok(Self {
            parked,
            engine,
            runners,
            runners_status,
            sdk,
            forwarder,
            raw: Barrier::Pending,
        })
    }

    pub(crate) async fn observe(&mut self, deadline: Instant) -> Report {
        tokio::join!(
            self.engine.observe(deadline, engine_barrier),
            observe_runners(&self.runners, &mut self.runners_status, deadline),
            self.sdk.observe(deadline, |()| Barrier::Complete),
            self.forwarder.observe(deadline, |()| Barrier::Complete),
            observe_raw(&self.parked.owner.connector, &mut self.raw, deadline),
        );
        self.report()
    }

    fn report(&self) -> Report {
        Report {
            engine: self.engine.status,
            runners: self.runners_status,
            sdk: self.sdk.status,
            forwarder: self.forwarder.status,
            raw: self.raw,
        }
    }

    /// Retains the observer and exact lease if any component may still have
    /// live ownership. A known-ended failure can transfer parked custody.
    pub(crate) fn finish(self) -> std::result::Result<(ParkedOrdinary, Report), Box<Self>> {
        let report = self.report();
        if report.requires_process_hold() {
            return Err(Box::new(self));
        }
        Ok((self.parked, report))
    }
}

async fn observe_runners(runners: &[RunnerHandle], status: &mut Barrier, deadline: Instant) {
    if *status != Barrier::Pending {
        return;
    }
    loop {
        let mut observed = Barrier::Complete;
        for runner in runners {
            observed = match (observed, runner.reap_observation()) {
                (_, ReapObservation::Uncertain) => Barrier::PossiblyLive,
                (_, ReapObservation::Pending) => Barrier::Pending,
                (Barrier::Complete, ReapObservation::FailedNoChild) => Barrier::Incomplete,
                _ => observed,
            };
            if observed == Barrier::PossiblyLive {
                break;
            }
        }
        if observed != Barrier::Pending {
            *status = observed;
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            return;
        }
        sleep_until((now + Duration::from_millis(5)).min(deadline)).await;
    }
}

async fn observe_raw(connector: &RelayConnector, status: &mut Barrier, deadline: Instant) {
    if *status == Barrier::Pending {
        *status = match connector.reap_fatal_sockets(deadline).await {
            Ok(()) => Barrier::Complete,
            Err(Error::Timeout) => Barrier::Pending,
            Err(_) => Barrier::PossiblyLive,
        };
    }
}

#[cfg(test)]
#[path = "mdk_owner_physical_cut_tests.rs"]
mod tests;
