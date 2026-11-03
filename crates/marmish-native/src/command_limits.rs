//! Pure bounded-service decisions. These helpers confer no command authority.
use crate::{Error, Result};
use std::{num::NonZeroU64, time::Duration};

pub(crate) const MAX_UPTIME: Duration = Duration::from_secs(1800);
#[derive(Clone, Copy, Default)]
pub(crate) struct LimitOverrides {
    pub max_commands: Option<NonZeroU64>,
    pub max_uptime: Option<Duration>,
}
pub(crate) fn parse_max_commands(text: &str) -> Result<NonZeroU64> {
    let value = decimal(text)?;
    if value > i64::MAX as u64 {
        return Err(Error::Invalid("command_max_commands"));
    }
    NonZeroU64::new(value).ok_or(Error::Invalid("command_max_commands"))
}
pub(crate) fn parse_max_uptime(text: &str) -> Result<Duration> {
    let (digits, scale) = if let Some(s) = text.strip_suffix('m') {
        (s, 60)
    } else {
        (text.strip_suffix('s').unwrap_or(text), 1)
    };
    let seconds = decimal(digits)?
        .checked_mul(scale)
        .filter(|v| (1..=MAX_UPTIME.as_secs()).contains(v))
        .ok_or(Error::Invalid("command_max_uptime"))?;
    Ok(Duration::from_secs(seconds))
}
fn decimal(text: &str) -> Result<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid("command_limit_integer"));
    }
    text.parse()
        .map_err(|_| Error::Invalid("command_limit_integer"))
}
/// Supply only durable actor facts after the current projection/ACK turn completes.
pub(crate) struct CountDrain {
    pub maximum: Option<u64>,
    pub admitted: u64,
    pub reserved: u64,
    pub finished: u64,
    pub control_idle: bool,
    pub publications_terminal: bool,
}
impl CountDrain {
    pub fn ready(&self) -> bool {
        self.maximum.is_some_and(|n| n > 0 && self.admitted >= n)
            && self.reserved == 0
            && self.finished == self.admitted
            && self.control_idle
            && self.publications_terminal
    }
}
/// A launch-local suspend-aware deadline; never reconstruct from wall time.
/// Capture before profile/config/database/network work. Callers must recheck after
/// every bounded wait (at most 100ms) and before new admission/dispatch.
pub(crate) struct ServiceLifetime {
    origin: Duration,
    deadline: Option<Duration>,
}
impl ServiceLifetime {
    pub fn start(limit: Option<Duration>) -> Result<Self> {
        Self::at(continuous_time()?, limit)
    }
    fn at(now: Duration, limit: Option<Duration>) -> Result<Self> {
        if limit.is_some_and(|d| d.is_zero() || d > MAX_UPTIME) {
            return Err(Error::Invalid("command_max_uptime"));
        }
        let deadline = limit
            .map(|d| {
                now.checked_add(d)
                    .ok_or(Error::Invalid("command_lifetime_overflow"))
            })
            .transpose()?;
        Ok(Self {
            origin: now,
            deadline,
        })
    }
    /// Apply effective configuration against the captured launch origin, never now.
    pub fn constrain(mut self, limit: Option<Duration>) -> Result<Self> {
        let candidate = Self::at(self.origin, limit)?.deadline;
        self.deadline = match (self.deadline, candidate) {
            (Some(old), Some(new)) => Some(old.min(new)),
            (old, None) => old,
            (None, new) => new,
        };
        Ok(self)
    }
    pub fn remaining(&self) -> Result<Option<Duration>> {
        Ok(self.remaining_at(continuous_time()?))
    }
    fn remaining_at(&self, now: Duration) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_sub(now))
    }
    pub fn expired(&self) -> Result<bool> {
        Ok(self.remaining()?.is_some_and(|d| d.is_zero()))
    }
}
#[cfg(target_os = "linux")]
fn continuous_time() -> Result<Duration> {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes a valid timespec into this live exclusive pointer.
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut time) } != 0 {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    if time.tv_sec < 0 || !(0..1_000_000_000).contains(&time.tv_nsec) {
        return Err(Error::Invalid("command_continuous_clock"));
    }
    Ok(Duration::new(time.tv_sec as u64, time.tv_nsec as u32))
}
#[cfg(target_os = "macos")]
fn continuous_time() -> Result<Duration> {
    unsafe extern "C" {
        fn mach_continuous_time() -> u64;
    }
    let mut base = libc::mach_timebase_info_data_t { numer: 0, denom: 0 };
    // SAFETY: the system API writes to a live, exclusively borrowed timebase struct.
    if unsafe { libc::mach_timebase_info(&mut base) } != 0 || base.denom == 0 {
        return Err(Error::Invalid("command_continuous_clock"));
    }
    // SAFETY: this system clock function takes no arguments and has no preconditions.
    let ticks = unsafe { mach_continuous_time() };
    let ns = u128::from(ticks)
        .checked_mul(u128::from(base.numer))
        .ok_or(Error::Invalid("command_continuous_clock"))?
        / u128::from(base.denom);
    let seconds = u64::try_from(ns / 1_000_000_000)
        .map_err(|_| Error::Invalid("command_continuous_clock"))?;
    Ok(Duration::new(seconds, (ns % 1_000_000_000) as u32))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strict_flags_and_checked_bounds() {
        for text in ["1800", "1800s", "30m"] {
            assert_eq!(parse_max_uptime(text).unwrap(), MAX_UPTIME);
        }
        for text in [
            "",
            "0",
            "31m",
            "1h",
            "+1",
            " 1",
            "1.0",
            "1M",
            "18446744073709551615m",
        ] {
            assert!(parse_max_uptime(text).is_err(), "{text}");
        }
        assert_eq!(parse_max_commands("3").unwrap().get(), 3);
        for text in [
            "0",
            "-1",
            "1s",
            "9223372036854775808",
            "18446744073709551616",
        ] {
            assert!(parse_max_commands(text).is_err(), "{text}");
        }
    }
    #[test]
    fn launch_budget_never_extends_and_counts_suspend() {
        let life =
            ServiceLifetime::at(Duration::from_secs(10), Some(Duration::from_secs(30))).unwrap();
        assert_eq!(
            life.remaining_at(Duration::from_secs(11)),
            Some(Duration::from_secs(29))
        );
        assert_eq!(
            life.remaining_at(Duration::from_secs(40)),
            Some(Duration::ZERO)
        );
        assert_eq!(
            life.remaining_at(Duration::from_secs(1000)),
            Some(Duration::ZERO)
        );
        assert_eq!(
            ServiceLifetime::at(Duration::ZERO, None)
                .unwrap()
                .remaining_at(MAX_UPTIME),
            None
        );
        let captured = ServiceLifetime::at(Duration::from_secs(10), None)
            .unwrap()
            .constrain(Some(Duration::from_secs(30)))
            .unwrap();
        assert_eq!(
            captured.remaining_at(Duration::from_secs(39)),
            Some(Duration::from_secs(1))
        );
        let captured = captured.constrain(Some(MAX_UPTIME)).unwrap();
        assert_eq!(
            captured.remaining_at(Duration::from_secs(40)),
            Some(Duration::ZERO)
        );
        assert!(ServiceLifetime::at(Duration::MAX, Some(MAX_UPTIME)).is_err());
        assert!(ServiceLifetime::at(Duration::ZERO, Some(Duration::ZERO)).is_err());
    }
    #[test]
    fn count_requires_every_durable_drain_condition() {
        let mut d = CountDrain {
            maximum: Some(3),
            admitted: 3,
            reserved: 0,
            finished: 3,
            control_idle: true,
            publications_terminal: true,
        };
        assert!(d.ready());
        d.reserved = 1;
        assert!(!d.ready());
        d.reserved = 0;
        d.finished = 2;
        assert!(!d.ready());
        d.finished = 3;
        d.control_idle = false;
        assert!(!d.ready());
        d.control_idle = true;
        d.publications_terminal = false;
        assert!(!d.ready());
        d.publications_terminal = true;
        d.maximum = Some(4);
        assert!(!d.ready());
        d.maximum = None;
        assert!(!d.ready());
    }
    #[test]
    fn actual_clock_is_available_and_monotonic() {
        let first = continuous_time().unwrap();
        assert!(continuous_time().unwrap() >= first);
        let life = ServiceLifetime::start(Some(MAX_UPTIME)).unwrap();
        assert!(!life.expired().unwrap());
        assert!(life.remaining().unwrap().unwrap() <= MAX_UPTIME);
        let overrides = LimitOverrides::default();
        assert!(overrides.max_commands.is_none() && overrides.max_uptime.is_none());
    }
}
