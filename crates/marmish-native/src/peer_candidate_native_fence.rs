//! Revocation and clock checks for a finite leased receiver turn.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
use std::sync::atomic::AtomicUsize;

use crate::{command_limits::continuous_time, peer_candidate_startup::Readiness};
use tokio_util::sync::CancellationToken;

use super::*;

#[derive(Clone)]
pub(crate) struct NativeWindow(Arc<Window>);

struct Window {
    readiness: Readiness,
    stop: Arc<AtomicBool>,
    route_cancel: CancellationToken,
    anchor: Sample,
    wall_end: Duration,
    tick_end: Duration,
    floor: Mutex<Option<Sample>>,
    #[cfg(test)]
    stop_after_signing: AtomicBool,
    #[cfg(test)]
    signs: AtomicUsize,
    #[cfg(test)]
    copy_cut: AtomicUsize,
    #[cfg(test)]
    trace_timing: AtomicBool,
    #[cfg(test)]
    crash_generation: AtomicBool,
}

#[derive(Clone, Copy)]
struct Sample {
    wall: Duration,
    tick: Duration,
}

impl Sample {
    fn now() -> Result<Self> {
        Ok(Self {
            wall: Wall::System.sample()?,
            tick: continuous_time().map_err(|_| Error::Clock)?,
        })
    }
}

impl NativeWindow {
    pub(super) fn start(
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        route_cancel: CancellationToken,
        anchored_wall: Duration,
        end_exclusive: u64,
    ) -> Result<Self> {
        let now = Sample::now()?;
        let anchor = Sample {
            wall: anchored_wall.max(now.wall),
            tick: now.tick,
        };
        let wall_end = Duration::from_secs(end_exclusive);
        let window = Self(Arc::new(Window {
            readiness,
            stop,
            route_cancel,
            anchor,
            wall_end,
            tick_end: now
                .tick
                .checked_add(
                    wall_end
                        .checked_sub(anchor.wall)
                        .ok_or(Error::Clock)?
                        .min(Duration::from_secs(5)),
                )
                .ok_or(Error::Clock)?,
            floor: Mutex::new(Some(now)),
            #[cfg(test)]
            stop_after_signing: AtomicBool::new(false),
            #[cfg(test)]
            signs: AtomicUsize::new(0),
            #[cfg(test)]
            copy_cut: AtomicUsize::new(0),
            #[cfg(test)]
            trace_timing: AtomicBool::new(false),
            #[cfg(test)]
            crash_generation: AtomicBool::new(false),
        }));
        if now.wall.abs_diff(anchored_wall) > Duration::from_secs(2) {
            return Err(Error::Clock);
        }
        window.sample()?;
        Ok(window)
    }

    fn observe(&self, now: Sample, floor: Sample) -> Result<Duration> {
        let window = &self.0;
        if !window.readiness.is_current()
            || window.stop.load(Ordering::Acquire)
            || window.route_cancel.is_cancelled()
            || now.wall < floor.wall
            || now.tick < floor.tick
            || now.tick >= window.tick_end
        {
            return Err(Error::Clock);
        }
        let elapsed = now
            .tick
            .checked_sub(window.anchor.tick)
            .ok_or(Error::Clock)?;
        let projected = window
            .anchor
            .wall
            .checked_add(elapsed)
            .ok_or(Error::Clock)?;
        let wall = now.wall.max(projected);
        if wall >= window.wall_end || now.wall.abs_diff(projected) > Duration::from_secs(2) {
            return Err(Error::Clock);
        }
        Ok(wall)
    }

    pub(super) fn sample(&self) -> Result<Duration> {
        let mut floor = self.0.floor.lock().map_err(|_| Error::Clock)?;
        let previous = floor.take().ok_or(Error::Clock)?;
        let now = Sample::now()?;
        let wall = self.observe(now, previous)?;
        *floor = Some(now);
        Ok(wall)
    }

    pub(super) fn require_offer_limit(&self, offer_expires: u64) -> Result<()> {
        if self.0.wall_end > Duration::from_secs(offer_expires) {
            return Err(Error::Clock);
        }
        self.sample()?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn stop_after_signing(&self) {
        self.0.stop_after_signing.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn signs(&self) -> usize {
        self.0.signs.load(Ordering::Acquire)
    }

    #[cfg(test)]
    pub(crate) fn signed_fixture(&self) {
        self.0.signs.fetch_add(1, Ordering::AcqRel);
        if self.0.stop_after_signing.load(Ordering::Acquire) {
            self.0.stop.store(true, Ordering::Release);
        }
    }

    #[cfg(test)]
    pub(super) fn set_copy_cut(&self, crash: bool) {
        self.0
            .copy_cut
            .store(if crash { 2 } else { 1 }, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn copy_cut(&self) {
        match self.0.copy_cut.swap(0, Ordering::AcqRel) {
            0 => {}
            1 => self.0.route_cancel.cancel(),
            2 => std::process::exit(78),
            _ => unreachable!("closed fixture cut enum"),
        }
    }

    #[cfg(test)]
    pub(super) fn trace_timing(&self) {
        self.0.trace_timing.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn timing(&self, stage: &'static str, started: std::time::Instant) {
        if self.0.trace_timing.load(Ordering::Acquire) {
            println!(
                "package_fixture_timing {stage}_ms={:.3}",
                started.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    #[cfg(test)]
    pub(super) fn crash_generation(&self) {
        self.0.crash_generation.store(true, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn before_generation_fixture(&self) {
        if self.0.crash_generation.load(Ordering::Acquire) {
            crate::peer_quota_vfs::dispatch::arm_package_generation_crash();
        }
    }

    pub(crate) fn stage_sample(&self) -> std::result::Result<Duration, StageError> {
        self.sample().map_err(|_| StageError::Unavailable)
    }

    pub(crate) fn check(&self) -> std::result::Result<(), StageError> {
        self.sample()
            .map(|_| ())
            .map_err(|_| StageError::Unavailable)
    }
}

/// Carries the existing fixture fence without relaxing its checks.
pub(crate) enum NativeFence<'a> {
    Candidate(&'a NativeWindow),
    #[cfg(test)]
    Fixture(&'a TurnFence<'a>),
}

impl NativeFence<'_> {
    pub(super) fn wall(&self) -> Result<u64> {
        match self {
            Self::Candidate(window) => window.sample().map(|wall| wall.as_secs()),
            #[cfg(test)]
            Self::Fixture(fence) => fence.wall(),
        }
    }

    pub(crate) fn check(&self) -> std::result::Result<(), StageError> {
        self.wall().map(|_| ()).map_err(|_| StageError::Unavailable)
    }
}

#[cfg(test)]
impl<'a> From<&'a TurnFence<'a>> for NativeFence<'a> {
    fn from(value: &'a TurnFence<'a>) -> Self {
        Self::Fixture(value)
    }
}
