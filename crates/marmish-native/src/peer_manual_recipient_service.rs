//! Bounded Manual service on the recovered owner's existing registered port.

use super::*;
use crate::{
    diagnostics::pending_refresh::{self, Outcome, Timing, nanos},
    ipc::peer_read::pending_view::AcceptedTiming,
    peer_candidate_startup::Readiness,
    peer_invitation::{
        receiver_inventory::{
            managed_delivery::SavedVisit,
            manual_ingress::Ingress,
            manual_work::{ManualDue, ManualPackageGuard},
            v2_pending::{PendingClock, PendingTurn},
        },
        socket::owner::recipient::managed_consent::{Publication, Wake},
    },
};

const PERIOD: Duration = Duration::from_millis(500);
const PROBE: Duration = Duration::from_millis(20);

pub(super) async fn serve<'profile>(
    owner: &mut RecoveredOwner<'profile>,
    lease: &'profile ProfileLease,
    quota: &ReceiverQuota<'profile>,
    port: &mut PeerRecipientPort,
    fence: &mut StartupFence<'_>,
    producer: Producer,
) -> Exit {
    let stop = fence.manual_stop.expect("Manual worker owns its stop cell");
    let stop_on_exit = StopOnExit(Some(stop));
    let mut service = ManualService {
        owner,
        lease,
        quota,
        port,
        fence,
        stop,
        producer,
    };
    let readiness = service.fence.ready.readiness();
    let exit = match service.run(readiness).await {
        Ok(()) => Exit::LocalUnavailable,
        Err(exit) => exit,
    };
    stop_on_exit.set();
    exit
}

struct ManualService<'turn, 'profile, 'guard> {
    owner: &'turn mut RecoveredOwner<'profile>,
    lease: &'profile ProfileLease,
    quota: &'turn ReceiverQuota<'profile>,
    port: &'turn mut PeerRecipientPort,
    fence: &'turn mut StartupFence<'guard>,
    stop: &'guard Arc<AtomicBool>,
    producer: Producer,
}

impl ManualService<'_, '_, '_> {
    fn check(&self) -> Result<(), Exit> {
        let fatal = final_exit(self.fence.forwarding, Exit::LocalUnavailable) == Exit::GlobalFault;
        classify_stop(
            fatal,
            self.stop.load(Ordering::Acquire),
            self.fence.stop.is_cancelled(),
            self.fence.ready.is_current(),
        )
        .map_or(Ok(()), Err)
    }

    fn local_failure(&self) -> Exit {
        self.check().err().unwrap_or(Exit::LocalUnavailable)
    }

    async fn run(&mut self, readiness: Readiness) -> Result<(), Exit> {
        let mut schedule = Schedule::new(Instant::now());
        let mut prior_publish = None;
        self.check()?;
        let mut ingress = Ingress::recover(
            self.owner,
            self.port,
            readiness.clone(),
            self.stop.clone(),
            self.fence.forwarding.clone(),
        )
        .map_err(|_| self.local_failure())?;
        let mut history_complete = false;
        loop {
            self.check()?;
            if !history_complete {
                let history = tokio::select! {
                    biased;
                    () = self.fence.fatal.tripped() => return Err(Exit::GlobalFault),
                    () = self.fence.stop.cancelled() => return Err(self.local_failure()),
                    () = self.fence.ready.stopped() => return Err(self.local_failure()),
                    result = ingress.history_turn(self.owner, self.port, None) => result,
                };
                history_complete = history.map_err(|_| self.local_failure())?;
                self.check()?;
            }

            let now = Instant::now();
            let mut managed = if schedule.managed_retry <= now {
                self.owner
                    .due_manual_work(self.port)
                    .map_err(|_| self.local_failure())?
            } else {
                None
            };
            let mut saved = if schedule.saved_retry <= now {
                self.owner
                    .probe_due_managed_delivery(self.port)
                    .map_err(|_| self.local_failure())?
            } else {
                None
            };
            self.check()?;
            let deadline = schedule.live_deadline(
                Instant::now(),
                !history_complete || managed.is_some() || saved.is_some(),
            );
            let live = tokio::select! {
                biased;
                () = self.fence.fatal.tripped() => return Err(Exit::GlobalFault),
                () = self.fence.stop.cancelled() => return Err(self.local_failure()),
                () = self.fence.ready.stopped() => return Err(self.local_failure()),
                result = ingress.live_turn(self.owner, self.port, deadline, || self.producer.invalidate()) => result,
            };
            let live_changed = live.map_err(|_| self.local_failure())?.is_some();
            self.check()?;
            if live_changed && schedule.saved_retry <= Instant::now() {
                saved = self
                    .owner
                    .probe_due_managed_delivery(self.port)
                    .map_err(|_| self.local_failure())?;
            }
            let saved_visited = saved.is_some();

            if let Some(coordinate) = saved {
                self.producer.invalidate();
                let outcome = self
                    .owner
                    .service_saved_delivery(
                        self.quota,
                        self.port,
                        coordinate,
                        &readiness,
                        self.stop,
                        self.fence.forwarding,
                    )
                    .map_err(|_| self.local_failure())?;
                self.check()?;
                match outcome {
                    SavedVisit::Stop => return Err(self.local_failure()),
                    SavedVisit::Transient => schedule.saved_retry = schedule.next(Instant::now()),
                    SavedVisit::Waiting
                    | SavedVisit::Applied
                    | SavedVisit::Retired
                    | SavedVisit::Reobserve => {}
                }
            }
            if (live_changed || saved_visited) && schedule.managed_retry <= Instant::now() {
                managed = self
                    .owner
                    .due_manual_work(self.port)
                    .map_err(|_| self.local_failure())?;
            }
            if let Some(managed) = managed
                && self.managed(managed, &readiness).await?
            {
                schedule.managed_retry = schedule.next(Instant::now());
            }
            if let Some(tick) = schedule.tick(Instant::now())
                && history_complete
            {
                let (result, outcome, accepted) = self.refresh(&readiness);
                if pending_refresh::enabled() {
                    pending_refresh::emit(tick.timing(outcome, accepted, prior_publish));
                    if let Some(accepted) = accepted {
                        prior_publish = Some(accepted);
                    }
                }
                result?;
            }
        }
    }

    async fn managed(&mut self, work: ManualDue, readiness: &Readiness) -> Result<bool, Exit> {
        let clock = |readiness, stop, forwarding| PendingClock::Candidate {
            readiness,
            stop,
            forwarding: Some(forwarding),
        };
        self.check()?;
        let forwarding = self.fence.forwarding;
        match work {
            ManualDue::Provision(selector) | ManualDue::Resume(selector) => {
                self.owner
                    .manual_package_turn(
                        self.quota,
                        self.port,
                        selector,
                        matches!(work, ManualDue::Provision(_)),
                        ManualPackageGuard {
                            readiness,
                            stop: self.stop,
                            forwarding,
                        },
                    )
                    .map_err(|_| self.local_failure())?;
            }
            ManualDue::Publish(selector) => {
                let outcome = self
                    .port
                    .publish_manual_consent(
                        self.owner,
                        self.quota,
                        selector,
                        readiness.clone(),
                        self.stop.clone(),
                        forwarding,
                    )
                    .await;
                self.check()?;
                match outcome {
                    Publication::Unavailable => return Ok(true),
                    Publication::Rejected | Publication::Settled(Wake::Completed(_)) => {}
                    Publication::NotSent(_)
                    | Publication::PossiblyExposed(_)
                    | Publication::Settled(Wake::Failed) => {
                        return Err(self.local_failure());
                    }
                }
            }
            ManualDue::Held(selector, generation) => {
                self.producer.invalidate();
                let result = (|| {
                    let mut pending = PendingTurn::borrow(self.owner, self.lease)?;
                    let armed = pending.arm(
                        self.port,
                        selector,
                        generation,
                        clock(readiness, self.stop, forwarding),
                    )?;
                    // JoinStarted is already durable. Whichever result the
                    // measured join returns, only the next readback may advance.
                    pending.ingest(self.port, armed, clock(readiness, self.stop, forwarding))
                })()
                .map_err(|_| self.local_failure())?;
                if result.result.is_err() {
                    return self.check().map(|()| false);
                }
            }
            ManualDue::JoinStarted(selector, generation) => {
                self.producer.invalidate();
                PendingTurn::borrow(self.owner, self.lease)
                    .and_then(|mut turn| {
                        turn.qualify(
                            self.port,
                            selector,
                            generation,
                            clock(readiness, self.stop, forwarding),
                        )
                    })
                    .map_err(|_| self.local_failure())?;
            }
        }
        self.check()?;
        Ok(false)
    }

    fn refresh(
        &mut self,
        readiness: &Readiness,
    ) -> (Result<(), Exit>, Outcome, Option<AcceptedTiming>) {
        let Some(ticket) = self.producer.read_ticket() else {
            return (self.check(), Outcome::TicketUnavailable, None);
        };
        let (outcome, accepted) = match self.owner.current_manual_pending(self.port) {
            Ok(Some((selector, generation))) => {
                let clock = PendingClock::Candidate {
                    readiness,
                    stop: self.stop,
                    forwarding: Some(self.fence.forwarding),
                };
                // A refused proof already refuses its exact ticket. The
                // actual port, stop and forwarding are checked independently.
                match PendingTurn::observe_with_ticket(
                    self.owner,
                    self.lease,
                    self.port,
                    selector,
                    generation,
                    (clock, &self.producer, ticket),
                ) {
                    Ok(accepted) => (Outcome::Published, Some(accepted)),
                    Err(_) => (Outcome::ProofRefused, None),
                }
            }
            Ok(None) => {
                self.producer.refuse(ticket);
                (Outcome::NoPending, None)
            }
            Err(_) => {
                self.producer.refuse(ticket);
                return (Err(self.local_failure()), Outcome::SelectionFailed, None);
            }
        };
        (self.check(), outcome, accepted)
    }
}

fn classify_stop(fatal: bool, cell: bool, cancelled: bool, ready: bool) -> Option<Exit> {
    if fatal {
        Some(Exit::GlobalFault)
    } else if cell || cancelled || !ready {
        Some(Exit::Stopped)
    } else {
        None
    }
}

struct Schedule {
    anchor: Instant,
    refresh: Instant,
    managed_retry: Instant,
    saved_retry: Instant,
}

impl Schedule {
    fn new(anchor: Instant) -> Self {
        Self {
            anchor,
            refresh: anchor + PERIOD,
            managed_retry: anchor,
            saved_retry: anchor,
        }
    }

    fn live_deadline(&self, now: Instant, work: bool) -> Instant {
        if work || self.refresh <= now {
            now + PROBE
        } else {
            self.refresh
        }
    }

    fn next(&self, now: Instant) -> Instant {
        let remainder = now.saturating_duration_since(self.anchor).as_nanos() % PERIOD.as_nanos();
        now + (PERIOD - Duration::from_nanos(remainder as u64))
    }

    fn tick(&mut self, now: Instant) -> Option<Tick> {
        if now < self.refresh {
            return None;
        }
        let overdue = now.duration_since(self.refresh).as_nanos();
        let missed = overdue / PERIOD.as_nanos();
        let lag = Duration::from_nanos((overdue % PERIOD.as_nanos()) as u64);
        self.refresh = self.next(now);
        Some(Tick {
            schedule_ns: nanos(now.duration_since(self.anchor) - lag),
            start_lag_ns: nanos(lag),
            missed_ticks: u64::try_from(missed).unwrap_or(u64::MAX),
        })
    }
}

struct Tick {
    schedule_ns: i64,
    start_lag_ns: i64,
    missed_ticks: u64,
}

impl Tick {
    fn timing(
        self,
        outcome: Outcome,
        accepted: Option<AcceptedTiming>,
        prior: Option<AcceptedTiming>,
    ) -> Timing {
        let pair = accepted.zip(prior);
        Timing {
            schedule_ns: self.schedule_ns,
            start_lag_ns: self.start_lag_ns,
            missed_ticks: self.missed_ticks,
            outcome,
            proof_age: accepted.and_then(AcceptedTiming::age),
            publish_gap: pair.and_then(|(current, prior)| current.gap(prior)),
            prior_slack: pair.and_then(|(current, prior)| current.prior_slack(prior)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_stop_precedence_keeps_fatal_independent() {
        assert_eq!(
            classify_stop(true, true, true, false),
            Some(Exit::GlobalFault)
        );
        assert_eq!(classify_stop(false, true, false, true), Some(Exit::Stopped));
        assert_eq!(classify_stop(false, false, true, true), Some(Exit::Stopped));
        assert_eq!(
            classify_stop(false, false, false, false),
            Some(Exit::Stopped)
        );
        assert_eq!(classify_stop(false, false, false, true), None);
    }

    #[test]
    fn manual_refresh_skips_missed_periods_without_drift() {
        let origin = Instant::now();
        let mut schedule = Schedule::new(origin);
        assert_eq!(schedule.live_deadline(origin, false), origin + PERIOD);
        assert_eq!(schedule.live_deadline(origin, true), origin + PROBE);
        assert!(
            schedule
                .tick(origin + PERIOD - Duration::from_millis(1))
                .is_none()
        );
        let late = origin + Duration::from_millis(7_013);
        let tick = schedule.tick(late).unwrap();
        assert_eq!(tick.schedule_ns, 7_000_000_000);
        assert_eq!(tick.start_lag_ns, 13_000_000);
        assert_eq!(tick.missed_ticks, 13);
        assert!(schedule.tick(late).is_none());
        assert_eq!(schedule.refresh, origin + Duration::from_millis(7_500));
        assert_eq!(schedule.live_deadline(late, false), schedule.refresh);
    }
}
