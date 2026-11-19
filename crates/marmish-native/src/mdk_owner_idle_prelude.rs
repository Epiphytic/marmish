//! Permit one guarded idle cleanup turn at a time before the physical cut.

use super::{Admission, Barrier, HealthyStop, OwnerCustody, OwnerReport, Report, StopIntake};
use crate::{
    Error, Result, command_engine::CleanupState, identity::ProfileLease, mdk_adapter::MdkOwner,
};
use std::{
    sync::Arc,
    time::{Duration, Instant as Clock},
};
use tokio::{
    task::JoinHandle,
    time::{Instant, timeout_at},
};

const BUDGET: Duration = Duration::from_secs(6);
const WAIT: Duration = Duration::from_millis(10);

const _: fn() = || {
    fn require_send_static<T: Send + 'static>() {}
    require_send_static::<Box<MdkOwner>>();
};

/// Private constructor keeps the shared command cleanup code tied to this
/// registration and to the one absolute budget.
pub(crate) struct IdleFence {
    admission: Admission,
    end: Clock,
}

impl IdleFence {
    pub(crate) fn check(&self) -> Result<()> {
        check(&self.admission, self.end)
    }
}

fn check(admission: &Admission, end: Clock) -> Result<()> {
    if !admission.admit() {
        return Err(Error::Transport("dispatcher_fatal"));
    }
    if Clock::now() >= end {
        return Err(Error::Timeout);
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Step {
    Begin,
    Poll(u64),
}

enum Progress {
    Started(u64),
    Polled {
        state: CleanupState,
        fence: Option<u64>,
        progressed: bool,
    },
}

enum Disposition {
    Proved,
    Failed,
    Expired,
}

impl Disposition {
    fn from_error(error: &Error) -> Self {
        match error {
            Error::Timeout => Self::Expired,
            _ => Self::Failed,
        }
    }
}

type Turn = JoinHandle<(OwnerCustody, Result<Progress>)>;

enum State {
    InOwner {
        owner: OwnerCustody,
        step: Step,
        ready_at: Instant,
    },
    InTurn {
        join: Turn,
        step: Step,
    },
    Disposition {
        owner: OwnerCustody,
        result: Disposition,
    },
    Parked,
}

struct IdlePrelude {
    state: State,
    terminal: Option<Disposition>,
    intake: StopIntake,
    end: Clock,
}

impl IdlePrelude {
    fn start(&mut self, owner: OwnerCustody, step: Step) {
        let mut owner = owner;
        let fence = IdleFence {
            admission: self.intake.admission.clone(),
            end: self.end,
        };
        let join = self.intake.runtime.spawn_blocking(move || {
            let result = trusted_turn(&mut owner, &step, &fence);
            (owner, result)
        });
        self.state = State::InTurn { join, step };
    }

    fn check(&self) -> Result<()> {
        check(&self.intake.admission, self.end)
    }

    async fn observe(&mut self, observation: Instant) {
        let expiry = Instant::from_std(self.end);
        let fatal = self.intake.fatal.clone();
        loop {
            if self.terminal.is_none()
                && let Err(error) = self.check()
            {
                self.terminal = Some(Disposition::from_error(&error));
            }
            match &mut self.state {
                State::InOwner { ready_at, .. } => {
                    if self.terminal.is_none() {
                        let limit = observation.min(expiry);
                        if Instant::now() >= limit {
                            return;
                        }
                        if *ready_at > Instant::now() {
                            if *ready_at >= limit {
                                return;
                            }
                            tokio::select! {
                                biased;
                                _ = fatal.tripped() => continue,
                                _ = tokio::time::sleep_until(*ready_at) => continue,
                            }
                        }
                    }
                    let State::InOwner { owner, step, .. } =
                        std::mem::replace(&mut self.state, State::Parked)
                    else {
                        unreachable!("owner checked above")
                    };
                    if let Some(result) = self.terminal.take() {
                        self.state = State::Disposition { owner, result };
                        return;
                    }
                    self.start(owner, step);
                }
                State::InTurn { join, .. } => {
                    if Instant::now() >= observation {
                        return;
                    }
                    let result = if self.terminal.is_some() {
                        timeout_at(observation, join).await
                    } else {
                        tokio::select! {
                            biased;
                            _ = fatal.tripped() => {
                                self.terminal = Some(Disposition::Failed);
                                continue;
                            }
                            result = timeout_at(observation.min(expiry), join) => result,
                        }
                    };
                    let Ok(result) = result else {
                        if self.terminal.is_none() && Instant::now() >= expiry {
                            self.terminal = Some(Disposition::Expired);
                            continue;
                        }
                        return;
                    };
                    let State::InTurn { step, .. } =
                        std::mem::replace(&mut self.state, State::Parked)
                    else {
                        unreachable!("turn checked above")
                    };
                    if let Ok((owner, result)) = result {
                        self.state = self.accept(owner, step, result);
                    }
                }
                State::Disposition { .. } | State::Parked => return,
            }
        }
    }

    fn accept(&mut self, owner: OwnerCustody, step: Step, result: Result<Progress>) -> State {
        let result = self.check().and(result);
        match (self.terminal.take(), step, result) {
            (Some(result), _, _) => State::Disposition { owner, result },
            (None, _, Err(error)) => State::Disposition {
                owner,
                result: Disposition::from_error(&error),
            },
            (None, Step::Begin, Ok(Progress::Started(fence))) => State::InOwner {
                owner,
                step: Step::Poll(fence),
                ready_at: Instant::now(),
            },
            (
                None,
                Step::Poll(expected),
                Ok(Progress::Polled {
                    state,
                    fence: Some(actual),
                    progressed,
                }),
            ) if expected == actual => match state {
                CleanupState::Draining => State::InOwner {
                    owner,
                    step: Step::Poll(expected),
                    ready_at: Instant::now() + if progressed { Duration::ZERO } else { WAIT },
                },
                CleanupState::Quiesced => State::Disposition {
                    owner,
                    result: Disposition::Proved,
                },
                _ => State::Disposition {
                    owner,
                    result: Disposition::Failed,
                },
            },
            _ => State::Disposition {
                owner,
                result: Disposition::Failed,
            },
        }
    }

    fn report(&self) -> OwnerReport {
        OwnerReport {
            physical: Report {
                engine: if matches!(self.state, State::Parked | State::InTurn { .. }) {
                    Barrier::PossiblyLive
                } else {
                    Barrier::Pending
                },
                runners: Barrier::Pending,
                sdk: Barrier::Pending,
                forwarder: Barrier::Pending,
                raw: Barrier::Pending,
            },
            graceful_complete: false,
            fatal: self.intake.fatal.is_tripped(),
        }
    }
}

fn trusted_turn(owner: &mut MdkOwner, step: &Step, fence: &IdleFence) -> Result<Progress> {
    fence.check()?;
    let service = owner
        .commands
        .as_mut()
        .ok_or(Error::Invalid("command_engine_missing"))?;
    let result = match step {
        Step::Begin => service
            .begin_idle_cleanup(&mut owner.journal, fence)
            .map(Progress::Started),
        Step::Poll(expected) => service
            .poll_idle_cleanup(&mut owner.journal, *expected, fence)
            .map(|((state, fence), progressed)| Progress::Polled {
                state,
                fence,
                progressed,
            }),
    };
    fence.check()?;
    result
}

#[must_use = "retain the owner until the shutdown disposition is proved"]
pub(crate) struct RegisteredStop(Phase);

enum Phase {
    Prelude(Option<Box<IdlePrelude>>),
    Physical(Box<HealthyStop>),
}

impl RegisteredStop {
    pub(crate) fn begin(
        owner: Box<MdkOwner>,
        lease: &Arc<ProfileLease>,
        admission: &Admission,
        began: Instant,
        active_clean: bool,
    ) -> std::result::Result<Self, Box<MdkOwner>> {
        let (intake, owner) = StopIntake::prepare(owner, lease, admission)?;
        let started = owner
            .commands
            .as_ref()
            .is_some_and(|service| service.engine_present_readonly());
        if !active_clean || !started || intake.fatal.is_tripped() {
            // A started engine only earns trusted cleanup on the new prelude
            // path. Keep the existing no-engine and startup/error handling.
            return Ok(Self(Phase::Physical(Box::new(HealthyStop::from_prepared(
                intake,
                owner,
                !active_clean || !started,
            )))));
        }
        Ok(Self(Phase::Prelude(Some(Box::new(IdlePrelude {
            intake,
            state: State::InOwner {
                owner,
                step: Step::Begin,
                ready_at: began,
            },
            terminal: None,
            end: began.into_std() + BUDGET,
        })))))
    }

    pub(crate) async fn observe(&mut self, deadline: Instant) -> OwnerReport {
        if let Phase::Prelude(slot) = &mut self.0 {
            let prelude = slot.as_mut().expect("owned prelude");
            prelude.observe(deadline).await;
            let State::Disposition { result, .. } = &prelude.state else {
                return prelude.report();
            };
            let proved = matches!(result, Disposition::Proved);
            if !proved && Instant::now() >= deadline {
                return prelude.report();
            }
            let IdlePrelude { state, intake, .. } = *slot.take().expect("observed disposition");
            let State::Disposition { owner, .. } = state else {
                unreachable!("disposition checked above");
            };
            // Transfer the returned owner and lease without awaiting. A proved
            // prelude gives the single graceful attempt a fresh physical slice.
            let physical = Box::new(HealthyStop::from_prepared(intake, owner, proved));
            let handoff = proved.then(|| physical.report());
            self.0 = Phase::Physical(physical);
            if let Some(report) = handoff {
                return report;
            }
        }
        match &mut self.0 {
            Phase::Physical(physical) => physical.observe(deadline).await,
            Phase::Prelude(_) => unreachable!("prelude reported or handed off above"),
        }
    }

    pub(crate) fn complete(
        self,
        cleanup_runtime: impl FnOnce() -> Result<()>,
    ) -> std::result::Result<(), Box<Self>> {
        match self.0 {
            Phase::Physical(physical) => physical
                .complete(cleanup_runtime)
                .map_err(|physical| Box::new(Self(Phase::Physical(physical)))),
            Phase::Prelude(prelude) => Err(Box::new(Self(Phase::Prelude(prelude)))),
        }
    }
}
