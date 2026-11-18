//! Physical Peer route evidence retained without a network or admission capability.

use super::{
    CA_ERROR, PeerTlsAttempt, PeerTlsWindow, ProfileFiles, RootCertStore, Trust,
    qualification::HeldQualification,
};
use crate::{
    Error, Result,
    identity::{IdentityMaterial, ProfileLease},
    transport::{ArtiDirs, PeerRelayConnector, RelayPolicy, RouteMode, TransportLimits},
};
use nostr::{
    key::{Keys, PublicKey},
    types::Url,
};
use nostr_sdk::transport::websocket::{WebSocketSink, WebSocketStream};
use rustls::pki_types::CertificateDer;
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

const CLOCK_ERROR: Error = Error::Invalid("peer_qualification_clock");
const CLOCK_TOLERANCE: Duration = Duration::from_secs(2);

#[allow(
    dead_code,
    reason = "checked Peer route remains unmounted pending durable admission"
)]
pub(super) struct CheckedPeerRoute {
    profile: Arc<ProfileFiles>,
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
        sample: impl FnMut() -> ClockSample,
    ) -> Result<Self> {
        Self::bind_profile_using(Arc::new(ProfileFiles::open(lease)?), material, sample)
    }

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(super) fn bind_with_profile(
        profile: Arc<ProfileFiles>,
        material: &IdentityMaterial,
    ) -> Result<Self> {
        Self::bind_profile_using(profile, material, ClockSample::now)
    }

    fn bind_profile_using(
        profile: Arc<ProfileFiles>,
        material: &IdentityMaterial,
        mut sample: impl FnMut() -> ClockSample,
    ) -> Result<Self> {
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

    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(super) fn recheck_material(
        &mut self,
        config: &super::HeldFile,
        material: &IdentityMaterial,
    ) -> Result<()> {
        let result = (|| {
            if !self.qualification.matches_config(config)
                || self.account != material.public_key()
                || self.device_slot != *material.device_slot()
            {
                return Err(super::PROFILE_CHANGED);
            }
            let key = self
                .profile
                .lease
                .peer_qualification_key_from_material(material)?;
            self.qualification.verify_issuer(&key)?;
            self.recheck()
        })();
        if result.is_err() {
            self.profile.cancel.cancel();
        }
        result
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

/// Admission is issued only by the paired receiver mount. The private fixture
/// exercises the production route and does not create installed authority.
#[allow(
    dead_code,
    reason = "receiver admission waits for the installed quota and cursor mount"
)]
pub(crate) struct ReceiverRouteAdmission {
    route: CheckedPeerRoute,
    signer: Arc<Keys>,
}

#[allow(
    dead_code,
    reason = "receiver admission waits for the installed quota and cursor mount"
)]
impl ReceiverRouteAdmission {
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    pub(crate) fn registered(
        token: super::candidate_registration::RegisteredRoute,
    ) -> Result<Self> {
        let (route, signer) = token.into_parts();
        if signer.public_key() != route.account {
            return Err(Error::Invalid("peer_identity_binding"));
        }
        Ok(Self { route, signer })
    }

    #[cfg(test)]
    pub(crate) fn fixture(lease: Arc<ProfileLease>, material: &IdentityMaterial) -> Result<Self> {
        Ok(Self {
            route: CheckedPeerRoute::bind(lease, material)?,
            signer: material.signer(),
        })
    }

    pub(crate) async fn open(
        mut self,
        max_frame_bytes: usize,
        deadline: tokio::time::Instant,
    ) -> Result<(LivePhysicalRoute, Arc<Keys>, WebSocketSink, WebSocketStream)> {
        if self.signer.public_key() != self.route.account {
            return Err(Error::Invalid("peer_identity_binding"));
        }
        self.route.recheck()?;
        let profile = &self.route.profile;
        let (relay, ca) = self.route.qualification.live_relay();
        let endpoint = relay.origin.clone();
        let (roots, anchor) = match (&relay.trust, ca) {
            (Trust::CaSha256(_), Some(ca)) => {
                let mut roots = RootCertStore::empty();
                roots
                    .add(CertificateDer::from(ca.bytes.clone()))
                    .map_err(|_| CA_ERROR)?;
                (roots, Some(ca.window.clone()))
            }
            (Trust::SpkiSha256(_), None) => (crate::transport::bundled_tls_roots(), None),
            _ => return Err(CA_ERROR),
        };
        let attempt = PeerTlsAttempt::new(profile.clone(), relay.trust.clone(), roots, anchor)?;
        let policy = RelayPolicy::parse(
            &[endpoint.to_string()],
            RouteMode::Off,
            TransportLimits {
                max_relays: 1,
                max_frame_bytes,
                ..TransportLimits::default()
            },
        )?;
        let dirs = ArtiDirs {
            state: profile.lease.arti_state().to_owned(),
            cache: profile.lease.arti_cache().to_owned(),
        };
        let dial = PeerLiveDial {
            endpoint: endpoint.clone(),
            policy,
            dirs,
            attempt,
            cancel: profile.cancel.clone(),
        };
        let deadline = deadline.min(
            self.route
                .clock
                .projected_end(self.route.qualification.end_exclusive()?)?,
        );
        let (sink, stream, window) = PeerRelayConnector::open_live(dial, deadline).await?;
        let mut live = LivePhysicalRoute {
            route: self.route,
            endpoint,
            window,
            ceiling: None,
        };
        live.recheck()?;
        Ok((live, self.signer, sink, stream))
    }
}

pub(crate) struct PeerLiveDial {
    endpoint: Url,
    policy: RelayPolicy,
    dirs: ArtiDirs,
    attempt: PeerTlsAttempt,
    cancel: CancellationToken,
}

impl PeerLiveDial {
    pub(crate) fn take(
        self,
    ) -> (
        Url,
        RelayPolicy,
        ArtiDirs,
        PeerTlsAttempt,
        CancellationToken,
    ) {
        (
            self.endpoint,
            self.policy,
            self.dirs,
            self.attempt,
            self.cancel,
        )
    }
}

pub(crate) struct LivePhysicalRoute {
    route: CheckedPeerRoute,
    endpoint: Url,
    window: PeerTlsWindow,
    ceiling: Option<tokio::time::Instant>,
}

impl LivePhysicalRoute {
    pub(crate) fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    pub(crate) fn account(&self) -> PublicKey {
        self.route.account
    }

    pub(crate) fn cancellation(&self) -> CancellationToken {
        self.route.profile.cancel.clone()
    }

    pub(crate) fn check_use(&mut self, deadline: &mut tokio::time::Instant) -> Result<u64> {
        let wall = self.observe()?;
        *deadline = (*deadline).min(self.ceiling.ok_or(CLOCK_ERROR)?);
        Ok(wall)
    }

    pub(crate) fn observe(&mut self) -> Result<u64> {
        self.recheck()?;
        let end = self
            .route
            .qualification
            .end_exclusive()?
            .min(self.window.end_exclusive()?);
        let projected = self.route.clock.projected_end(end)?;
        let ceiling = self.ceiling.map_or(projected, |prior| prior.min(projected));
        self.ceiling = Some(ceiling);
        if tokio::time::Instant::now() >= ceiling {
            self.route.profile.cancel.cancel();
            return Err(CLOCK_ERROR);
        }
        Ok(self.route.clock.effective_wall().as_secs())
    }

    pub(crate) fn clamp_wall(
        &self,
        deadline: &mut tokio::time::Instant,
        wall_end: u64,
    ) -> Result<()> {
        *deadline = (*deadline).min(self.route.clock.projected_end(wall_end)?);
        Ok(())
    }

    pub(crate) fn recheck(&mut self) -> Result<()> {
        self.route.recheck()?;
        if let Err(error) = self
            .window
            .check(self.route.clock.effective_wall().as_secs())
        {
            self.route.profile.cancel.cancel();
            return Err(error);
        }
        Ok(())
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
    effective_wall: Duration,
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
            effective_wall: initial_wall,
        })
    }

    fn projected_end(&self, end: u64) -> Result<tokio::time::Instant> {
        let remaining = Duration::from_secs(end)
            .checked_sub(self.effective_wall)
            .ok_or(CLOCK_ERROR)?;
        self.previous_mono
            .checked_add(remaining)
            .map(tokio::time::Instant::from_std)
            .ok_or(CLOCK_ERROR)
    }

    fn effective_wall(&self) -> Duration {
        self.effective_wall
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
        self.effective_wall = wall.max(projected);
        Ok(self.effective_wall.as_secs())
    }
}

#[cfg(test)]
#[path = "peer_physical_route_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::qualified_recipient_fixture;

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
pub(crate) use tests::{qualified_founder_fixture, qualified_manual_recipient_fixture};
