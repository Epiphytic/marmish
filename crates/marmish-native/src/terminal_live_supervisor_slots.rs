//! Checked slot selection and serialized advancement of parked replacements.

use super::slot_policy::{self as policy, Effect, Intent, Outcome, TicketEffect, WriterState};
use super::takeover::{Departure, Phase, Replacement, Startup};
use super::*;
use crate::terminal_offers::{MintError, Writer};
use crate::terminal_selection_wire_v2::{Choice, Target};

const DEPARTURE_SLICE: Duration = Duration::from_millis(250);
const SLOT_UNITS: usize = 4;

#[derive(Clone, Copy)]
pub(super) struct Launch {
    pub(super) slot: SlotId,
    pub(super) inventory: tmux::Inventory,
    pub(super) writer: Option<Claim>,
    pub(super) rows: u16,
    pub(super) cols: u16,
}

pub(super) enum Plan {
    Busy {
        slot: SlotId,
        writer: Claim,
        offer: Option<Ticket>,
    },
    Replace {
        slot: SlotId,
        old: Claim,
    },
    Launch(Launch),
}

pub(super) struct Parked {
    pub(super) slot: SlotId,
    pub(super) replacement: Replacement,
    inventory: tmux::Inventory,
    start: VerifiedStart,
    deadline: Duration,
    reply: Option<oneshot::Sender<StartOutcome>>,
    _permit: StartPermit,
}

type Refused = (oneshot::Sender<StartOutcome>, Error);

impl ControlOwner {
    pub(super) fn writer(&self, slot: SlotId) -> Option<&Active> {
        self.active.get(&slot)?.iter().find(|link| {
            link.role == Role::Writer
                && link.retiring.is_none()
                && link
                    .confirmation
                    .as_ref()
                    .is_none_or(|value| value.active())
                && matches!(link.port.source.check(), GuardResult::Applied(()))
        })
    }

    fn classify(&self, slot: SlotId) -> policy::State {
        let writer = self
            .writer(slot)
            .map(|_| WriterState::Confirmed)
            .or_else(|| {
                self.active
                    .get(&slot)?
                    .iter()
                    .any(|link| {
                        link.role == Role::Writer
                            && link.retiring.is_none()
                            && link.port.source.reason().is_none()
                    })
                    .then_some(WriterState::Unconfirmed)
            });
        policy::State::classify(
            self.quarantine.contains(&slot),
            self.parked.iter().any(|work| work.slot == slot),
            writer,
            self.pending_departure(slot) || self.active.contains_key(&slot),
        )
    }

    fn native_accounting(&self) -> usize {
        let live = self
            .active
            .iter()
            .filter(|(slot, _)| !self.parked.iter().any(|work| &work.slot == *slot))
            .map(|(_, links)| links.len())
            .sum::<usize>();
        live + self
            .parked
            .iter()
            .map(|work| work.replacement.units())
            .sum::<usize>()
    }

    pub(super) fn plan(
        &mut self,
        start: &VerifiedStart,
        budget: &AdmissionBudget<'_>,
    ) -> Result<Plan> {
        if !self.config.terminal_allowed(&start.account) {
            return Err(DENIED);
        }
        if !valid_size(start.rows, start.cols) {
            return Err(REQUEST);
        }
        let rows = self.registry.slots()?;
        let (scope, choice, target) = match start.selection {
            StartChoice::Checked { scope, request } => {
                if scope.client != start.account
                    || scope.generation != start.source.command_generation().get()
                    || request.size.rows != start.rows
                    || request.size.cols != start.cols
                {
                    return Err(DENIED);
                }
                (Some(scope), request.choice, request.target)
            }
            #[cfg(test)]
            StartChoice::Default => (None, Choice::Regular, Target::Default),
            #[cfg(test)]
            StartChoice::New => (None, Choice::New, Target::Default),
        };
        let row = match target {
            Target::Default => rows
                .iter()
                .copied()
                .find(|row| row.account == start.account && row.default),
            Target::Named(id) => Some(
                rows.iter()
                    .copied()
                    .find(|row| row.account == start.account && row.id.0 == id)
                    .ok_or(MISSING)?,
            ),
            Target::Offer(ticket) => Some(
                rows.iter()
                    .copied()
                    .find(|row| row.account == start.account && row.id.0 == ticket.slot)
                    .ok_or(STALE_OFFER)?,
            ),
        };
        let now = guard::sample().ok_or(STOPPED)?.continuous;
        let current_writer = row
            .and_then(|row| {
                self.writer(row.id)
                    .map(|link| writer_record(row.id, link.claim))
            })
            .transpose()?;
        let intent = match (choice, target) {
            (Choice::Regular, _) => Intent::Regular,
            (Choice::New, Target::Default) => Intent::New,
            (Choice::Observe | Choice::Takeover | Choice::New, Target::Offer(ticket)) => {
                let effect = match choice {
                    Choice::Observe => TicketEffect::Observe,
                    Choice::Takeover => TicketEffect::Takeover,
                    Choice::New => TicketEffect::New,
                    Choice::Regular => return Err(REQUEST),
                };
                let matches = current_writer.is_some_and(|writer| {
                    writer.generation == ticket.generation && writer.claim == ticket.claim
                });
                Intent::Ticketed { effect, matches }
            }
            (Choice::Observe, _) => Intent::Observe,
            (Choice::Takeover, _) => Intent::TakeoverOffer,
            (Choice::New, Target::Named(_)) => return Err(REQUEST),
        };
        let state = row.map_or(policy::State::Idle, |row| self.classify(row.id));
        let effect = match policy::decide(state, intent) {
            Outcome::Busy => {
                return self.busy(current_writer.ok_or(UNAVAILABLE)?, scope, now, budget);
            }
            Outcome::Start(effect) => effect,
            Outcome::Unavailable => return Err(UNAVAILABLE),
            Outcome::WriterMissing => return Err(WRITER_MISSING),
            Outcome::StaleOffer => return Err(STALE_OFFER),
            Outcome::ReplacementPending => return Err(REPLACEMENT_PENDING),
        };
        let offered = match target {
            Target::Offer(ticket) => Some((
                scope.ok_or(STALE_OFFER)?,
                current_writer.ok_or(STALE_OFFER)?,
                ticket,
            )),
            _ => None,
        };
        if let Some((scope, writer, ticket)) = offered
            && !self.offers.contains(scope, writer, ticket, now)
        {
            return Err(STALE_OFFER);
        }
        if effect != Effect::Takeover && self.native_accounting() >= MAX_LIVE {
            return Err(CAPACITY);
        }
        if effect == Effect::Observer
            && current_writer
                .and_then(|writer| self.active.get(&SlotId(writer.slot)))
                .is_some_and(|links| links.len() >= SLOT_UNITS)
        {
            return Err(CAPACITY);
        }
        budget.check(false)?;
        if let Some((scope, writer, ticket)) = offered
            && !self.offers.consume(scope, writer, ticket, now)
        {
            return Err(STALE_OFFER);
        }
        if !self.manager.drain_pending() {
            return Err(CLEANUP);
        }
        match effect {
            Effect::Takeover => {
                let writer = current_writer.ok_or(WRITER_MISSING)?;
                Ok(Plan::Replace {
                    slot: SlotId(writer.slot),
                    old: claim_of(writer),
                })
            }
            Effect::Observer => {
                let writer = current_writer.ok_or(WRITER_MISSING)?;
                let slot = SlotId(writer.slot);
                let linked = self.writer(slot).ok_or(WRITER_MISSING)?;
                let (rows, cols) = match linked.port.checked_geometry(linked.claim, budget.due) {
                    io::GeometryHint::Size(size) => size,
                    _ => return Err(UNAVAILABLE),
                };
                Ok(Plan::Launch(Launch {
                    slot,
                    inventory: linked.inventory,
                    writer: Some(linked.claim),
                    rows,
                    cols,
                }))
            }
            Effect::Writer | Effect::New => {
                let existing = (effect == Effect::Writer).then_some(row).flatten();
                let default = effect == Effect::Writer && matches!(target, Target::Default);
                let (slot, inventory) = self.select(budget, rows.len(), existing, default)?;
                budget.check(false)?;
                self.manager.wait_clients_until(
                    slot,
                    inventory,
                    0,
                    tmux::ClientChange::Departing,
                    budget.until,
                )?;
                Ok(Plan::Launch(Launch {
                    slot,
                    inventory,
                    writer: None,
                    rows: start.rows,
                    cols: start.cols,
                }))
            }
        }
    }

    fn busy(
        &mut self,
        writer: Writer,
        scope: Option<Scope>,
        now: Duration,
        budget: &AdmissionBudget<'_>,
    ) -> Result<Plan> {
        budget.check(false)?;
        let links = &self.active;
        self.offers.prune(now, |_, writer| {
            links.get(&SlotId(writer.slot)).is_some_and(|links| {
                links.iter().any(|link| {
                    link.claim == claim_of(writer)
                        && link.retiring.is_none()
                        && link.port.source.reason().is_none()
                })
            })
        });
        let offer = scope
            .map(|scope| {
                self.offers
                    .mint(scope, writer, random()?, now)
                    .map_err(|error| match error {
                        MintError::Capacity => CAPACITY,
                        MintError::Invalid => UNAVAILABLE,
                    })
            })
            .transpose()?;
        Ok(Plan::Busy {
            slot: SlotId(writer.slot),
            writer: claim_of(writer),
            offer,
        })
    }

    pub(super) fn exact_native(&mut self, native: &Native) -> Option<&mut Active> {
        self.active.get_mut(&native.slot)?.iter_mut().find(|link| {
            link.claim == native.claim
                && link.retiring.is_none()
                && Arc::ptr_eq(&link.port, &native.port)
        })
    }

    pub(super) fn remove_unstarted(&mut self, slot: SlotId, claim: Claim) {
        if let Some(links) = self.active.get_mut(&slot) {
            links.retain(|link| link.claim != claim);
            if links.is_empty() {
                self.active.remove(&slot);
            }
        }
    }

    pub(super) fn reserve_replacement(
        &mut self,
        slot: SlotId,
        old: Claim,
        start: VerifiedStart,
        reply: oneshot::Sender<StartOutcome>,
        permit: StartPermit,
        deadline: Duration,
    ) -> std::result::Result<(), Refused> {
        let Some(now) = guard::sample().map(|sample| sample.continuous) else {
            return Err((reply, STOPPED));
        };
        if self.stopping.load(Ordering::Acquire)
            || start.room_hold.is_none()
            || reply.is_closed()
            || !matches!(start.source.check(), GuardResult::Applied(()))
        {
            return Err((reply, STOPPED));
        }
        let Some(writer) = self.writer(slot).filter(|link| link.claim == old) else {
            return Err((reply, WRITER_MISSING));
        };
        let inventory = writer.inventory;
        let writer_port = Arc::clone(&writer.port);
        let links = &self.active[&slot];
        let observers: Vec<_> = links
            .iter()
            .filter(|link| link.writer == Some(old))
            .map(|link| link.claim)
            .collect();
        let Some(replacement) = Replacement::begin(old, &observers, deadline, now) else {
            return Err((reply, UNAVAILABLE));
        };
        self.parked.push_back(Parked {
            slot,
            replacement,
            inventory,
            start,
            deadline,
            reply: Some(reply),
            _permit: permit,
        });
        for observer in links.iter().filter(|link| link.writer == Some(old)) {
            observer.port.request_close(Reason::Replaced);
        }
        writer_port.request_close(Reason::Replaced);
        Ok(())
    }

    pub(super) fn advance_parked(&mut self, install: &mpsc::SyncSender<Installing>) -> bool {
        let Some(mut parked) = self.parked.pop_front() else {
            return false;
        };
        let sample = guard::sample().map(|sample| sample.continuous);
        let now = sample.unwrap_or(Duration::MAX);
        if self.stopping.load(Ordering::Acquire)
            || sample.is_none()
            || parked.reply.as_ref().is_some_and(|reply| reply.is_closed())
            || !self.config.terminal_allowed(&parked.start.account)
            || !matches!(parked.start.source.check(), GuardResult::Applied(()))
        {
            parked.replacement.withdraw();
        }
        let mut sampled = false;
        match parked.replacement.phase(now) {
            Phase::VerifyDeparture(epoch) => {
                let result = self.sample_departure(
                    parked.slot,
                    parked.inventory,
                    parked.deadline.saturating_sub(now),
                );
                sampled = true;
                let now = guard::sample().map_or(Duration::MAX, |sample| sample.continuous);
                parked.replacement.record_departure(epoch, result, now);
            }
            Phase::StartSuccessor => {
                self.start_successor(&mut parked, install);
                sampled = true;
            }
            Phase::RetiringSuccessor(claim) => {
                if let Some(link) = self
                    .active
                    .get(&parked.slot)
                    .and_then(|links| links.iter().find(|link| link.claim == claim))
                {
                    link.port.request_close(Reason::Detached);
                }
            }
            _ => (),
        }
        let now = guard::sample().map_or(Duration::MAX, |sample| sample.continuous);
        match parked.replacement.phase(now) {
            Phase::Quarantine => {
                self.quarantine.insert(parked.slot);
                if let Some(reply) = parked.reply.take() {
                    refuse(reply, UNAVAILABLE);
                }
            }
            Phase::Detached => {
                if let Some(reply) = parked.reply.take() {
                    refuse(reply, UNAVAILABLE);
                }
            }
            Phase::Transferred => (),
            _ => self.parked.push_back(parked),
        }
        sampled
    }

    fn start_successor(&mut self, parked: &mut Parked, install: &mpsc::SyncSender<Installing>) {
        let Some(reply) = parked.reply.take() else {
            return;
        };
        let stopping = Arc::clone(&self.stopping);
        let attempt = (|| {
            let budget = AdmissionBudget::fixed(&parked.start, &reply, &stopping, parked.deadline)?;
            let row = self.row(parked.slot)?;
            let current = match recovery::inventory(
                &mut self.registry,
                &mut self.manager,
                row,
                budget.until.min(Instant::now() + DEPARTURE_SLICE),
            ) {
                Ok(current) if current == parked.inventory => current,
                _ => {
                    self.quarantine.insert(parked.slot);
                    return Err(UNAVAILABLE);
                }
            };
            budget.check(false)?;
            let claim = self.allocate_claim()?;
            let now = guard::sample().ok_or(STOPPED)?.continuous;
            if !parked.replacement.begin_successor(claim, now) {
                return Err(UNAVAILABLE);
            }
            let launch = Launch {
                slot: parked.slot,
                inventory: current,
                writer: None,
                rows: parked.start.rows,
                cols: parked.start.cols,
            };
            let native = match self.launch(&budget, claim, launch) {
                Ok(native) => native,
                Err(error) => {
                    let pending = self
                        .active
                        .get(&parked.slot)
                        .is_some_and(|links| links.iter().any(|link| link.claim == claim));
                    let outcome = if pending {
                        Startup::NeedsCleanup
                    } else {
                        Startup::NeverStarted
                    };
                    parked.replacement.startup(claim, outcome, now);
                    return Err(error);
                }
            };
            let confirmation = match self.prepare_confirmation(&native, budget.due) {
                Ok(confirmation) => confirmation,
                Err(error) => {
                    parked
                        .replacement
                        .startup(claim, Startup::NeedsCleanup, now);
                    self.retire(native);
                    return Err(error);
                }
            };
            let built = guard::sample().map_or(Duration::MAX, |sample| sample.continuous);
            parked.replacement.startup(
                claim,
                Startup::Ready {
                    confirm_until: confirmation.until,
                },
                built,
            );
            if parked.replacement.phase(built) != Phase::AwaitingConfirm {
                self.retire(native);
                return Err(DEADLINE);
            }
            Ok((native, confirmation))
        })();
        match attempt {
            Ok((native, confirmation)) => {
                self.install_native(native, parked.deadline, Some(confirmation), reply, install)
            }
            Err(error) => {
                parked.replacement.withdraw();
                refuse(reply, error);
            }
        }
    }

    fn sample_departure(
        &mut self,
        slot: SlotId,
        inventory: tmux::Inventory,
        remaining: Duration,
    ) -> Departure {
        if !self.manager.drain_pending() {
            return Departure::TimedOut;
        }
        let until = Instant::now() + remaining.min(DEPARTURE_SLICE);
        match self.manager.client_count(slot, inventory, until) {
            Ok(0) => Departure::Empty,
            Ok(_) => Departure::Occupied,
            Err(_) if Instant::now() >= until => Departure::TimedOut,
            Err(_) => Departure::Uncertain,
        }
    }

    pub(super) fn pending_departure(&self, slot: SlotId) -> bool {
        self.departure
            .iter()
            .any(|(candidate, _)| *candidate == slot)
    }

    pub(super) fn sample_idle_departure(&mut self) {
        let Some((slot, inventory)) = self.departure.pop_front() else {
            return;
        };
        if self.active.contains_key(&slot) {
            self.departure.push_back((slot, inventory));
            return;
        }
        match self.sample_departure(slot, inventory, DEPARTURE_SLICE) {
            Departure::Empty => (),
            Departure::Uncertain => {
                self.recover_exit(slot);
                if self.row(slot).is_ok() {
                    self.quarantine.insert(slot);
                }
            }
            Departure::Occupied | Departure::TimedOut => {
                self.departure.push_back((slot, inventory));
            }
        }
    }
}

fn writer_record(slot: SlotId, claim: Claim) -> Result<Writer> {
    Ok(Writer {
        slot: slot.0,
        claim: claim.random,
        generation: NonZeroU64::new(claim.generation).ok_or(UNAVAILABLE)?,
    })
}

fn claim_of(writer: Writer) -> Claim {
    Claim {
        random: writer.claim,
        generation: writer.generation.get(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::source;
    use super::*;

    #[tokio::test]
    #[ignore = "uses native profile to initialize the control owner"]
    async fn terminal_live_supervisor_confirm_transfers_capacity_before_next_parked_turn() {
        let fixture = super::super::super::tests::Fixture::new().await;
        let (profile, material) = fixture.identity();
        let account = fixture.accounts[0];
        let (wake, _reader) = Wake::open().unwrap();
        let mut owner = ControlOwner {
            config: CommandConfig::load(profile.root()).unwrap(),
            registry: Registry::open(&profile, &material).unwrap(),
            manager: Manager::new(&fixture.tmux, profile.root()).unwrap(),
            daemon: fixture.daemon.clone(),
            active: HashMap::new(),
            offers: Ledger::default(),
            parked: VecDeque::new(),
            departure: VecDeque::new(),
            quarantine: HashSet::new(),
            wake,
            stopping: Arc::default(),
            profile: Arc::new(profile),
        };
        let inventory = tmux::Inventory {
            session: 1,
            pid: 2,
            directory: (3, 4),
            socket: (5, 6),
        };
        let slot = SlotId([7; 16]);
        let earlier = SlotId([8; 16]);
        let sample = guard::sample().unwrap();
        let deadline = sample.continuous + Duration::from_secs(5);
        let now = sample.continuous;
        let old = Claim {
            random: [9; 16],
            generation: 1,
        };
        let successor = Claim {
            random: [10; 16],
            generation: 2,
        };
        let (successor_source, _) = source(10);
        let confirmation = Confirmation::new(deadline);
        let permits = Arc::new(StartLimit::default());
        for (slot, source) in [
            (earlier, source(9).0),
            (slot, Arc::clone(&successor_source)),
        ] {
            owner.parked.push_back(Parked {
                slot,
                replacement: Replacement::begin(old, &[], deadline, now).unwrap(),
                inventory,
                start: VerifiedStart {
                    account,
                    selection: StartChoice::Default,
                    rows: 24,
                    cols: 80,
                    source,
                    deadline,
                    room_hold: Some(Arc::new(())),
                },
                deadline,
                reply: None,
                _permit: permits.try_acquire().unwrap(),
            });
        }
        let parked = owner.parked.back_mut().unwrap();
        assert!(parked.replacement.reaped(old));
        assert!(parked.replacement.record_departure(
            super::super::takeover::DepartureEpoch::Old,
            Departure::Empty,
            now
        ));
        assert!(parked.replacement.begin_successor(successor, now));
        assert!(parked.replacement.startup(
            successor,
            Startup::Ready {
                confirm_until: deadline
            },
            now
        ));
        assert_eq!(
            successor_source.install(successor, sample),
            GuardResult::Applied(())
        );
        owner.active.insert(
            slot,
            vec![Active {
                account,
                inventory,
                writer: None,
                claim: successor,
                role: Role::Writer,
                confirmation: Some(Arc::clone(&confirmation)),
                pending_start: None,
                port: Port::new(Arc::clone(&successor_source), owner.wake.clone()),
                retiring: None,
            }],
        );

        assert_eq!(owner.native_accounting(), 2);
        assert_eq!(owner.classify(slot), policy::State::Reserved);
        let (reply, mut received) = oneshot::channel();
        owner.confirm_one(confirm::Request {
            slot,
            claim: successor,
            role: Role::Writer,
            source: successor_source,
            until: deadline,
            reply,
        });
        assert!(received.try_recv().unwrap().is_ok());
        assert_eq!(owner.parked.len(), 1);
        assert_eq!(owner.parked.front().unwrap().slot.0, earlier.0);
        assert_eq!(owner.classify(slot), policy::State::Confirmed);
        assert_eq!(owner.native_accounting(), 2);
    }
}
