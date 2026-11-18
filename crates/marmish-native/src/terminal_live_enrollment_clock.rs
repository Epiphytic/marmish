//! Suspend-aware admission freshness and bounded consumed Probe IDs.
use crate::command_wire::Account;
use crate::terminal_bootstrap::RumorId;
use crate::{Error, Result};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CLOCK: Error = Error::Invalid("terminal_bootstrap_clock");
const RATE: Error = Error::Invalid("terminal_bootstrap_rate");
const TOLERANCE: Duration = Duration::from_secs(2);
const UNCERTAIN_HOLD: Duration = Duration::from_secs(41);
const REPLAY_RETENTION: Duration = Duration::from_secs(60);
const SCOPES: usize = 64;
const PROBE_LIMIT: usize = 8;

#[derive(Clone, Copy)]
pub(super) struct Sample {
    pub(super) wall: Duration,
    pub(super) tick: Duration,
}

impl Sample {
    pub(super) fn read() -> Result<Self> {
        let tick = crate::command_limits::continuous_time().map_err(|_| CLOCK)?;
        let wall = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CLOCK)?;
        Ok(Self { wall, tick })
    }
}

/// Store once per profile, outside entry and socket replacement.
#[derive(Default)]
pub(super) struct Freshness {
    anchor: Option<Sample>,
    last_tick: Option<Duration>,
    uncertain_since: Option<Duration>,
}

impl Freshness {
    pub(super) fn check(&mut self, sample: Sample) -> Result<u64> {
        if let Some(previous) = self.last_tick.filter(|&previous| sample.tick < previous) {
            self.uncertain_since.get_or_insert(previous);
            return Err(CLOCK);
        }
        self.last_tick = Some(sample.tick);
        let anchor = *self.anchor.get_or_insert(sample);
        let elapsed = sample.tick.checked_sub(anchor.tick).ok_or(CLOCK)?;
        let projected = anchor.wall.checked_add(elapsed).ok_or(CLOCK)?;
        if projected.abs_diff(sample.wall) > TOLERANCE {
            self.uncertain_since.get_or_insert(sample.tick);
            return Err(CLOCK);
        }
        if let Some(since) = self.uncertain_since {
            if sample.tick.checked_sub(since).ok_or(CLOCK)? < UNCERTAIN_HOLD {
                return Err(CLOCK);
            }
            self.uncertain_since = None;
        }
        // Rebase only forward. Within-tolerance jitter therefore cannot
        // accumulate into a backward wall adjustment.
        self.anchor = Some(Sample {
            wall: projected.max(sample.wall),
            tick: sample.tick,
        });
        Ok(sample.wall.as_secs())
    }
}

#[derive(Default)]
struct ConsumedProbes {
    ids: VecDeque<(RumorId, Duration)>,
    listeners: usize,
}

impl ConsumedProbes {
    fn charge(&mut self, id: RumorId, tick: Duration) -> Result<bool> {
        self.sweep(tick);
        if self.ids.iter().any(|entry| entry.0 == id) {
            return Ok(false);
        }
        if self.ids.len() == PROBE_LIMIT {
            return Err(RATE);
        }
        self.ids.push_back((id, tick));
        Ok(true)
    }

    fn sweep(&mut self, tick: Duration) {
        while self
            .ids
            .front()
            .is_some_and(|entry| tick.saturating_sub(entry.1) >= REPLAY_RETENTION)
        {
            self.ids.pop_front();
        }
    }
}

type Scope = (Account, String);

#[derive(Default)]
pub(crate) struct AdmissionLedger(Mutex<Ledger>);

#[derive(Default)]
struct Ledger {
    clock: Freshness,
    scopes: BTreeMap<Scope, ConsumedProbes>,
}

impl Ledger {
    fn sweep(&mut self, sample: Sample) -> Result<()> {
        self.clock.check(sample)?;
        self.scopes.retain(|_, row| {
            row.sweep(sample.tick);
            row.listeners != 0 || !row.ids.is_empty()
        });
        Ok(())
    }
}

pub(crate) struct InboundState {
    ledger: Arc<AdmissionLedger>,
    scope: Scope,
}

impl InboundState {
    pub(crate) fn register(
        ledger: Arc<AdmissionLedger>,
        peer: Account,
        endpoint: String,
    ) -> Result<Self> {
        Self::register_at(ledger, (peer, endpoint), Sample::read()?)
    }

    fn register_at(ledger: Arc<AdmissionLedger>, scope: Scope, sample: Sample) -> Result<Self> {
        {
            let mut locked = ledger.0.lock().map_err(|_| CLOCK)?;
            locked.sweep(sample)?;
            if locked.scopes.len() == SCOPES && !locked.scopes.contains_key(&scope) {
                return Err(RATE);
            }
            let row = locked.scopes.entry(scope.clone()).or_default();
            row.listeners = row.listeners.checked_add(1).ok_or(RATE)?;
        }
        Ok(Self { ledger, scope })
    }

    pub(super) fn charge(&self, id: RumorId, sample: Sample) -> Result<bool> {
        let mut locked = self.ledger.0.lock().map_err(|_| CLOCK)?;
        locked.sweep(sample)?;
        locked
            .scopes
            .get_mut(&self.scope)
            .ok_or(RATE)?
            .charge(id, sample.tick)
    }

    pub(super) fn check(&self, sample: Sample) -> Result<()> {
        self.ledger.0.lock().map_err(|_| CLOCK)?.sweep(sample)
    }
}

#[cfg(test)]
impl Default for InboundState {
    fn default() -> Self {
        Self::register(Arc::default(), [0; 32], String::new()).expect("fixture clock")
    }
}

impl Drop for InboundState {
    fn drop(&mut self) {
        let Ok(mut locked) = self.ledger.0.lock() else {
            return;
        };
        if let Some(row) = locked.scopes.get_mut(&self.scope) {
            row.listeners -= 1;
            if row.listeners == 0 && row.ids.is_empty() {
                locked.scopes.remove(&self.scope);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn time(wall: u64, tick: u64) -> Sample {
        Sample {
            wall: Duration::from_secs(wall),
            tick: Duration::from_secs(tick),
        }
    }

    #[test]
    fn drift_cannot_make_a_consumed_probe_eligible_after_recovery() {
        let mut clock = Freshness::default();
        assert_eq!(clock.check(time(1_000, 0)).unwrap(), 1_000);
        assert_eq!(clock.check(time(1_002, 0)).unwrap(), 1_002);
        let mut ids = ConsumedProbes::default();
        assert!(ids.charge([1; 32], time(1_002, 0).tick).unwrap());
        for seconds in [58, 59] {
            let at_floor = time(1_000 + seconds, seconds);
            assert_eq!(clock.check(at_floor).unwrap(), at_floor.wall.as_secs());
            assert!(!ids.charge([1; 32], at_floor.tick).unwrap());
        }
        ids.sweep(Duration::from_secs(60));
        assert!(ids.ids.is_empty());
        assert!(clock.check(time(1_010, 62)).is_err());
        assert!(clock.check(time(1_110, 110)).unwrap() > 1_002 + 5 + 35);
        assert!(clock.check(time(1_028, 115)).is_err());
        assert!(clock.check(time(1_153, 153)).is_err());
        assert_eq!(clock.check(time(1_160, 160)).unwrap(), 1_160);
        assert!(clock.check(time(1_200, 159)).is_err());
    }

    #[test]
    fn duplicate_ids_do_not_refill_eight_admissions() {
        let origin = Duration::from_secs(15);
        let mut ids = ConsumedProbes::default();
        for id in 0..8 {
            assert!(ids.charge([id; 32], origin).unwrap());
        }
        let before_expiry = origin + REPLAY_RETENTION - Duration::from_nanos(1);
        assert!(!ids.charge([0; 32], before_expiry).unwrap());
        assert!(ids.charge([8; 32], before_expiry).is_err());
        assert!(ids.charge([8; 32], origin + REPLAY_RETENTION).unwrap());
        assert_eq!(ids.ids.len(), 1);
    }

    #[test]
    fn clock_and_duplicate_history_survive_listener_replacement() {
        let ledger = Arc::new(AdmissionLedger::default());
        let scope = ([1; 32], "wss://first.invalid/".to_owned());
        let listener =
            InboundState::register_at(ledger.clone(), scope.clone(), time(1000, 0)).unwrap();
        assert!(listener.charge([3; 32], time(1000, 0)).unwrap());
        drop(listener);
        let replacement =
            InboundState::register_at(ledger.clone(), scope.clone(), time(1010, 10)).unwrap();
        assert!(!replacement.charge([3; 32], time(1010, 10)).unwrap());
        assert!(replacement.charge([4; 32], time(1008, 15)).is_err());
        drop(replacement);
        assert!(InboundState::register_at(ledger.clone(), scope.clone(), time(1050, 50)).is_err());
        let other = ([4; 32], "wss://another.invalid/".to_owned());
        assert!(InboundState::register_at(ledger.clone(), other, time(1055, 55)).is_err());
        let listener = InboundState::register_at(ledger, scope, time(1060, 60)).unwrap();
        assert!(listener.charge([3; 32], time(1060, 60)).unwrap());
    }

    #[test]
    fn terminal_runtime_core_listener_clock_refusals_preserve_replay_state() {
        let ledger = Arc::new(AdmissionLedger::default());
        let scope = ([1; 32], "wss://relay.example/".to_owned());
        let listener =
            InboundState::register_at(ledger.clone(), scope.clone(), time(1000, 0)).unwrap();
        assert!(listener.charge([3; 32], time(1000, 0)).unwrap());
        drop(listener);

        let other = ([2; 32], scope.1.clone());
        assert!(matches!(
            InboundState::register_at(ledger.clone(), other.clone(), time(1008, 15)),
            Err(Error::Invalid("terminal_bootstrap_clock"))
        ));
        for tick in 16..56 {
            assert!(matches!(
                InboundState::register_at(ledger.clone(), other.clone(), time(1000 + tick, tick)),
                Err(Error::Invalid("terminal_bootstrap_clock"))
            ));
        }
        {
            let state = ledger.0.lock().unwrap();
            assert_eq!(state.clock.uncertain_since, Some(Duration::from_secs(15)));
            assert_eq!(state.scopes.len(), 1);
            let row = &state.scopes[&scope];
            assert_eq!(row.ids, [([3; 32], Duration::ZERO)]);
            assert_eq!(row.listeners, 0);
        }
        let recovered = InboundState::register_at(ledger, scope, time(1056, 56)).unwrap();
        assert!(!recovered.charge([3; 32], time(1056, 56)).unwrap());
    }

    #[test]
    fn full_scope_table_reclaims_only_expired_reaped_rows() {
        let ledger = Arc::new(AdmissionLedger::default());
        let mut listeners = Vec::new();
        let origin = "wss://first.invalid/".to_owned();
        for peer in 0..64 {
            listeners.push(
                InboundState::register_at(
                    ledger.clone(),
                    ([peer; 32], origin.clone()),
                    time(1000, 0),
                )
                .unwrap(),
            );
        }
        let extra = ([64; 32], origin);
        listeners[63].charge([1; 32], time(1000, 0)).unwrap();
        assert!(InboundState::register_at(ledger.clone(), extra.clone(), time(1000, 0)).is_err());
        drop(listeners.pop());
        assert!(InboundState::register_at(ledger.clone(), extra.clone(), time(1059, 59)).is_err());
        assert!(InboundState::register_at(ledger, extra, time(1060, 60)).is_ok());
    }
}
