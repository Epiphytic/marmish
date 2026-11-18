//! Physical Peer route evidence retained without a network or admission capability.

use super::{ProfileFiles, qualification::HeldQualification};
use crate::{
    Error, Result,
    identity::{IdentityMaterial, ProfileLease},
};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CLOCK_ERROR: Error = Error::Invalid("peer_qualification_clock");
const CLOCK_TOLERANCE: Duration = Duration::from_secs(2);

#[allow(
    dead_code,
    reason = "checked Peer route remains unmounted pending durable admission"
)]
pub(super) struct CheckedPeerRoute {
    profile: ProfileFiles,
    qualification: HeldQualification,
    account: nostr::key::PublicKey,
    device_slot: [u8; 16],
    clock: ScopeClock,
}

#[allow(
    dead_code,
    reason = "checked Peer route remains unmounted pending durable admission"
)]
impl CheckedPeerRoute {
    pub(super) fn bind(lease: Arc<ProfileLease>, material: &IdentityMaterial) -> Result<Self> {
        Self::bind_using(lease, material, ClockSample::now)
    }

    fn bind_using(
        lease: Arc<ProfileLease>,
        material: &IdentityMaterial,
        mut sample: impl FnMut() -> ClockSample,
    ) -> Result<Self> {
        let profile = ProfileFiles::open(lease)?;
        let clock = ScopeClock::new(sample())?;
        let qualification = {
            let key = profile
                .lease
                .peer_qualification_key_from_material(material)?;
            super::qualification::read_issued_held(&profile, &key, clock.initial_wall.as_secs())?
        };
        let mut route = Self {
            profile,
            qualification,
            account: material.public_key(),
            device_slot: *material.device_slot(),
            clock,
        };
        route.recheck_using(sample)?;
        Ok(route)
    }

    pub(super) fn recheck(&mut self) -> Result<()> {
        self.recheck_using(ClockSample::now)
    }

    fn recheck_using(&mut self, mut sample: impl FnMut() -> ClockSample) -> Result<()> {
        let result = (|| {
            self.profile.recheck()?;
            self.qualification
                .check_time(self.clock.observe(sample())?)?;
            self.qualification.recheck_files(&self.profile)?;
            self.profile.recheck()?;
            self.qualification.check_time(self.clock.observe(sample())?)
        })();
        if result.is_err() {
            self.profile.cancel.cancel();
        }
        result
    }
}

impl Drop for CheckedPeerRoute {
    fn drop(&mut self) {
        self.profile.cancel.cancel();
    }
}

#[allow(
    dead_code,
    reason = "checked Peer route remains unmounted pending durable admission"
)]
struct ClockSample {
    wall: SystemTime,
    mono: Instant,
}

#[allow(
    dead_code,
    reason = "checked Peer route remains unmounted pending durable admission"
)]
impl ClockSample {
    fn now() -> Self {
        Self {
            wall: SystemTime::now(),
            mono: Instant::now(),
        }
    }
}

#[allow(
    dead_code,
    reason = "checked Peer route remains unmounted pending durable admission"
)]
struct ScopeClock {
    initial_wall: Duration,
    initial_mono: Instant,
    previous_wall: Duration,
    previous_mono: Instant,
}

#[allow(
    dead_code,
    reason = "checked Peer route remains unmounted pending durable admission"
)]
impl ScopeClock {
    fn new(sample: ClockSample) -> Result<Self> {
        let initial_wall = sample
            .wall
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CLOCK_ERROR)?;
        Ok(Self {
            initial_wall,
            initial_mono: sample.mono,
            previous_wall: initial_wall,
            previous_mono: sample.mono,
        })
    }

    fn observe(&mut self, sample: ClockSample) -> Result<u64> {
        let wall = sample
            .wall
            .duration_since(UNIX_EPOCH)
            .map_err(|_| CLOCK_ERROR)?;
        if wall < self.previous_wall || sample.mono < self.previous_mono {
            return Err(CLOCK_ERROR);
        }
        let elapsed = sample
            .mono
            .checked_duration_since(self.initial_mono)
            .ok_or(CLOCK_ERROR)?;
        let projected = self.initial_wall.checked_add(elapsed).ok_or(CLOCK_ERROR)?;
        if wall.abs_diff(projected) > CLOCK_TOLERANCE {
            return Err(CLOCK_ERROR);
        }
        self.previous_wall = wall;
        self.previous_mono = sample.mono;
        Ok(wall.max(projected).as_secs())
    }
}

#[cfg(test)]
#[path = "peer_physical_route_tests.rs"]
mod tests;
