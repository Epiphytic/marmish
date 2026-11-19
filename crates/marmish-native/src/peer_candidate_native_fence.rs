//! Revocation and clock checks for a finite leased receiver turn.

use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
use std::sync::atomic::AtomicUsize;

#[cfg(test)]
use tokio_util::sync::CancellationToken;

use crate::{
    command_limits::continuous_time, peer_candidate_startup::Readiness,
    peer_relay_trust::NativeRouteFence,
};

use super::*;

#[derive(Clone)]
pub(crate) struct NativeWindow(Arc<Window>);

struct Window {
    readiness: Readiness,
    stop: Arc<AtomicBool>,
    anchor: Sample,
    ceiling: Ceiling,
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
    #[cfg(test)]
    expire_schema: AtomicBool,
}

#[derive(Clone)]
struct Ceiling {
    wall: Duration,
    route: RouteFence,
}

#[derive(Clone)]
enum RouteFence {
    Checked(NativeRouteFence),
    #[cfg(test)]
    Fixture(CancellationToken),
}

impl RouteFence {
    fn is_current(&self) -> bool {
        match self {
            Self::Checked(route) => route.is_current(),
            #[cfg(test)]
            Self::Fixture(cancel) => !cancel.is_cancelled(),
        }
    }
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
    #[cfg(test)]
    pub(super) fn start(
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        route_cancel: CancellationToken,
        anchored_wall: Duration,
        end_exclusive: u64,
    ) -> Result<Self> {
        Self::start_with(
            readiness,
            stop,
            anchored_wall,
            Ceiling {
                wall: Duration::from_secs(end_exclusive),
                route: RouteFence::Fixture(route_cancel),
            },
        )
    }

    #[allow(dead_code, reason = "consumed by the separately mounted Manual actor")]
    pub(crate) fn start_checked(
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        end_exclusive: u64,
        route: NativeRouteFence,
    ) -> crate::Result<Self> {
        Self::start_with(
            readiness,
            stop,
            route.wall(),
            Ceiling {
                wall: Duration::from_secs(end_exclusive.min(route.wall_end())),
                route: RouteFence::Checked(route),
            },
        )
        .map_err(|_| crate::Error::Invalid("peer_native_clock"))
    }

    fn start_with(
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        anchored_wall: Duration,
        ceiling: Ceiling,
    ) -> Result<Self> {
        let now = Sample::now()?;
        if now.wall.abs_diff(anchored_wall) > Duration::from_secs(2) {
            return Err(Error::Clock);
        }
        Self::new(
            readiness,
            stop,
            Sample {
                wall: anchored_wall.max(now.wall),
                tick: now.tick,
            },
            ceiling,
            now,
            Duration::from_secs(5),
        )
    }

    fn new(
        readiness: Readiness,
        stop: Arc<AtomicBool>,
        anchor: Sample,
        ceiling: Ceiling,
        floor: Sample,
        limit: Duration,
    ) -> Result<Self> {
        let now = Sample::now()?;
        let projected = anchor
            .wall
            .checked_add(now.tick.checked_sub(anchor.tick).ok_or(Error::Clock)?)
            .ok_or(Error::Clock)?;
        let remaining = ceiling
            .wall
            .checked_sub(now.wall.max(projected))
            .ok_or(Error::Clock)?;
        let window = Self(Arc::new(Window {
            readiness,
            stop,
            anchor,
            ceiling,
            tick_end: now
                .tick
                .checked_add(remaining.min(limit))
                .ok_or(Error::Clock)?,
            floor: Mutex::new(Some(floor)),
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
            #[cfg(test)]
            expire_schema: AtomicBool::new(false),
        }));
        window.sample()?;
        Ok(window)
    }

    fn next(&self, limit: Duration) -> Result<Self> {
        let mut floor = self.0.floor.lock().map_err(|_| Error::Clock)?;
        let before = floor.take().ok_or(Error::Clock)?;
        let now = Sample::now()?;
        self.observe(now, before)?;
        Self::new(
            self.0.readiness.clone(),
            self.0.stop.clone(),
            self.0.anchor,
            self.0.ceiling.clone(),
            now,
            limit,
        )
    }

    pub(super) fn schema_turn(&self) -> Result<SchemaWindow> {
        let window = self.next(Duration::from_secs(15))?;
        #[cfg(test)]
        window.0.expire_schema.store(
            self.0.expire_schema.load(Ordering::Acquire),
            Ordering::Release,
        );
        Ok(SchemaWindow(window))
    }

    fn observe(&self, now: Sample, floor: Sample) -> Result<Duration> {
        let window = &self.0;
        if !window.readiness.is_current()
            || window.stop.load(Ordering::Acquire)
            || !window.ceiling.route.is_current()
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
        if wall >= window.ceiling.wall || now.wall.abs_diff(projected) > Duration::from_secs(2) {
            return Err(Error::Clock);
        }
        Ok(wall)
    }

    pub(super) fn sample(&self) -> Result<Duration> {
        self.checked_sample().map(|(wall, _)| wall)
    }

    pub(in crate::peer_invitation) fn remaining(&self) -> Result<Duration> {
        let (_, tick) = self.checked_sample()?;
        self.0.tick_end.checked_sub(tick).ok_or(Error::Clock)
    }

    fn checked_sample(&self) -> Result<(Duration, Duration)> {
        let mut floor = self.0.floor.lock().map_err(|_| Error::Clock)?;
        let previous = floor.take().ok_or(Error::Clock)?;
        let now = Sample::now()?;
        let wall = self.observe(now, previous)?;
        *floor = Some(now);
        Ok((wall, now.tick))
    }

    pub(super) fn require_offer_limit(&self, offer_expires: u64) -> Result<()> {
        if self.0.ceiling.wall > Duration::from_secs(offer_expires) {
            return Err(Error::Clock);
        }
        self.sample()?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_end_exclusive(&self) -> Duration {
        self.0.ceiling.wall
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
            1 => match &self.0.ceiling.route {
                RouteFence::Fixture(cancel) => cancel.cancel(),
                RouteFence::Checked(_) => self.0.stop.store(true, Ordering::Release),
            },
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
    pub(super) fn expire_schema_after_open(&self) {
        self.0.expire_schema.store(true, Ordering::Release);
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

/// The longer schema-only window never reaches the package publisher.
pub(crate) struct SchemaWindow(NativeWindow);

impl SchemaWindow {
    #[cfg(test)]
    pub(super) fn wait_for_fixture_expiry(&self) -> Result<()> {
        let window = &self.0.0;
        if window.expire_schema.load(Ordering::Acquire) {
            loop {
                let now = continuous_time().map_err(|_| Error::Clock)?;
                if now >= window.tick_end {
                    break;
                }
                std::thread::sleep(window.tick_end - now);
            }
        }
        Ok(())
    }

    pub(super) fn sample(&self) -> Result<Duration> {
        self.0.sample()
    }

    pub(super) fn require_offer_limit(&self, offer_expires: u64) -> Result<()> {
        self.0.require_offer_limit(offer_expires)
    }

    pub(super) fn into_issue(self) -> Result<NativeWindow> {
        self.0.next(Duration::from_secs(5))
    }
}

/// Carries the existing fixture fence without relaxing its checks.
pub(crate) enum NativeFence<'a> {
    Candidate(&'a NativeWindow),
    Schema(&'a SchemaWindow),
    #[cfg(test)]
    Fixture(&'a TurnFence<'a>),
}

impl NativeFence<'_> {
    pub(super) fn wall(&self) -> Result<u64> {
        match self {
            Self::Candidate(window) => window.sample().map(|wall| wall.as_secs()),
            Self::Schema(window) => window.sample().map(|wall| wall.as_secs()),
            #[cfg(test)]
            Self::Fixture(fence) => fence.wall(),
        }
    }

    pub(super) fn require_offer_limit(&self, offer_expires: u64) -> Result<()> {
        match self {
            Self::Candidate(window) => window.require_offer_limit(offer_expires),
            Self::Schema(window) => window.require_offer_limit(offer_expires),
            #[cfg(test)]
            Self::Fixture(_) => Err(Error::Binding),
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
