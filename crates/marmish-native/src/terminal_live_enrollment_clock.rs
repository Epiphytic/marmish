//! Suspend-aware admission freshness and bounded consumed Probe IDs.
use crate::terminal_bootstrap::RumorId;
use crate::{Error, Result};
use std::collections::VecDeque;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CLOCK: Error = Error::Invalid("terminal_bootstrap_clock");
const RATE: Error = Error::Invalid("terminal_bootstrap_rate");
const TOLERANCE: Duration = Duration::from_secs(2);
const UNCERTAIN_HOLD: Duration = Duration::from_secs(41);
const REPLAY_RETENTION: Duration = Duration::from_secs(45);
const PROBE_INTERVAL: Duration = Duration::from_secs(60);
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

/// One sender ledger for the initial single-binding writer scope. Replacing a
/// transport or verified binding generation leaves the ledger in place.
#[derive(Default)]
pub(super) struct ConsumedProbes {
    ids: VecDeque<(RumorId, Duration)>,
    last: Option<Duration>,
}

impl ConsumedProbes {
    /// Call only after checking the profile clock and authenticating the
    /// Probe. Duplicates never charge or refresh the original lifetime.
    pub(super) fn charge(&mut self, id: RumorId, tick: Duration) -> Result<bool> {
        self.sweep(tick);
        if self.ids.iter().any(|entry| entry.0 == id) {
            return Ok(false);
        }
        if self.ids.len() >= PROBE_LIMIT
            || self
                .last
                .is_some_and(|last| tick.saturating_sub(last) < PROBE_INTERVAL)
        {
            return Err(RATE);
        }
        self.ids.push_back((id, tick));
        self.last = Some(tick);
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
        for seconds in [43, 44] {
            let at_floor = time(1_000 + seconds, seconds);
            assert_eq!(clock.check(at_floor).unwrap(), at_floor.wall.as_secs());
            assert!(!ids.charge([1; 32], at_floor.tick).unwrap());
        }
        ids.sweep(Duration::from_secs(45));
        assert!(ids.ids.is_empty());
        assert!(clock.check(time(1_010, 62)).is_err());
        assert!(clock.check(time(1_110, 110)).unwrap() > 1_002 + 5 + 35);
        assert!(clock.check(time(1_028, 115)).is_err());
        assert!(clock.check(time(1_153, 153)).is_err());
        assert_eq!(clock.check(time(1_160, 160)).unwrap(), 1_160);
        assert!(clock.check(time(1_200, 159)).is_err());
    }

    #[test]
    fn duplicates_are_inert_and_failures_do_not_refill_the_probe_interval() {
        let mut ids = ConsumedProbes::default();
        let current = Duration::from_secs(15);
        assert!(ids.charge([1; 32], current).unwrap());
        assert!(
            !ids.charge([1; 32], current + Duration::from_secs(30))
                .unwrap()
        );
        let first_sweep = current + Duration::from_secs(59);
        assert!(ids.charge([2; 32], first_sweep).is_err());
        assert!(ids.ids.is_empty());
        assert!(ids.charge([2; 32], current + PROBE_INTERVAL).unwrap());
    }
}
