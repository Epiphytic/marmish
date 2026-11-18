//! Carry one Terminal flight to a checked native-room handoff.
use super::{Binding, CommandDirection, terminal_live_enrollment as enrollment};
use crate::terminal_bootstrap::BoundSigning;
use crate::terminal_fence::FenceState;
use crate::transport::{ArtiDirs, RelayConnector, RelayPolicy};
use crate::{Error, Result};
use enrollment::{
    EnrollmentCeiling, InboundState, ReadyEnrollment, Reapers, RoomPermit, finish_failed_enrollment,
};
use nostr::key::Keys;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::Instant;

type Grant = Arc<dyn Fn() -> bool + Send + Sync>;
const CAPACITY: Error = Error::Invalid("terminal_capacity");
const UNAVAILABLE: Error = Error::Invalid("terminal_owner_unavailable");

fn trace<T>(stage: &'static str, result: Result<T>) -> Result<T> {
    let category = match &result {
        Ok(_) => "complete",
        Err(Error::Invalid(category) | Error::Transport(category)) => category,
        Err(Error::Timeout) => "timeout",
        Err(_) => "operation_failed",
    };
    crate::diagnostics::event(stage, category);
    result
}

#[derive(Clone)]
pub(crate) struct Route {
    profile: RelayPolicy,
    directories: ArtiDirs,
    signer: Arc<Keys>,
    client_reapers: Arc<Reapers>,
    server_reapers: Arc<Reapers>,
    #[cfg(all(
        any(test, marmish_peer_receiver_candidate),
        target_os = "linux",
        target_arch = "x86_64"
    ))]
    physical_stop: Option<crate::dispatcher_fatal::Fatal>,
    #[cfg(test)]
    fixture_roots: Option<rustls::RootCertStore>,
    #[cfg(test)]
    fixture_quiet_open: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl Route {
    pub(crate) fn new(profile: RelayPolicy, directories: ArtiDirs, signer: Arc<Keys>) -> Self {
        Self {
            profile,
            directories,
            signer,
            client_reapers: Arc::default(),
            server_reapers: Arc::default(),
            #[cfg(all(
                any(test, marmish_peer_receiver_candidate),
                target_os = "linux",
                target_arch = "x86_64"
            ))]
            physical_stop: None,
            #[cfg(test)]
            fixture_roots: None,
            #[cfg(test)]
            fixture_quiet_open: None,
        }
    }

    #[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
    fn physical_fixture(
        profile: RelayPolicy,
        directories: ArtiDirs,
        signer: Arc<Keys>,
        roots: rustls::RootCertStore,
        stop: crate::dispatcher_fatal::Fatal,
    ) -> Result<Self> {
        let mut route = Self::new(profile, directories, signer);
        route.set_fixture_roots(roots)?;
        route.physical_stop = Some(stop);
        Ok(route)
    }

    #[cfg(test)]
    pub(crate) fn set_fixture_roots(&mut self, roots: rustls::RootCertStore) -> Result<()> {
        let [endpoint] = self.profile.urls() else {
            return Err(Error::Invalid("fixture_relay_trust"));
        };
        RelayConnector::for_loopback_fixture(
            self.profile.clone(),
            self.directories.clone(),
            endpoint.clone(),
            roots.clone(),
        )?;
        self.fixture_roots = Some(roots);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn watch_fixture_quiet_open(&mut self, quiet: Arc<AtomicBool>) {
        assert!(self.fixture_roots.is_some());
        self.fixture_quiet_open = Some(quiet);
    }

    pub(crate) async fn finish(&self) -> Result<()> {
        let (client, server) =
            tokio::join!(self.client_reapers.finish(), self.server_reapers.finish());
        client.and(server)
    }

    pub(crate) fn server_available(&self) -> bool {
        self.server_reapers.available()
    }

    pub(crate) fn endpoint(&self, binding: &Binding) -> Option<String> {
        binding
            .context
            .relay_endpoints()
            .iter()
            .find(|url| self.profile.direct_wss_scope(url).is_some())
            .cloned()
    }

    async fn open(
        &self,
        binding: &Binding,
        endpoint: &str,
        deadline: Instant,
        grant: Grant,
        local_closed: Option<Arc<AtomicBool>>,
    ) -> Result<enrollment::Bootstrap> {
        let reapers = match binding.context.direction() {
            CommandDirection::Client => &self.client_reapers,
            CommandDirection::Server => &self.server_reapers,
        };
        if !reapers.available() {
            return Err(UNAVAILABLE);
        }
        let carrier = enrollment::admit(
            binding.context.clone(),
            binding.fence.clone(),
            &self.profile,
            endpoint,
        )?;
        let (signing, codec) = BoundSigning::bind(&binding.context, self.signer.clone())?;
        let result = carrier
            .open_inner(
                signing,
                codec,
                self.connector(endpoint)?,
                deadline,
                Some(enrollment::CarrierScope {
                    reapers: reapers.clone(),
                    grant,
                    local_closed,
                }),
            )
            .await;
        #[cfg(test)]
        if let Some(quiet) = &self.fixture_quiet_open
            && matches!(
                result,
                Err(Error::Transport(enrollment::RELAY_AUTH_CHALLENGE_TIMEOUT))
            )
        {
            quiet.store(true, std::sync::atomic::Ordering::Release);
        }
        result
    }

    fn connector(&self, endpoint: &str) -> Result<RelayConnector> {
        let policy = self
            .profile
            .direct_wss_scope(endpoint)
            .ok_or(Error::Transport("terminal_transport_unavailable"))?;
        #[cfg(test)]
        let connector = if let Some(roots) = &self.fixture_roots {
            let [url] = policy.urls() else {
                return Err(Error::Invalid("fixture_relay_trust"));
            };
            let exact = url.clone();
            RelayConnector::for_loopback_fixture(
                policy,
                self.directories.clone(),
                exact,
                roots.clone(),
            )
        } else {
            RelayConnector::new(policy, self.directories.clone())
        }?;
        #[cfg(not(test))]
        let connector = RelayConnector::new(policy, self.directories.clone())?;
        #[cfg(all(
            any(test, marmish_peer_receiver_candidate),
            target_os = "linux",
            target_arch = "x86_64"
        ))]
        if let Some(fatal) = &self.physical_stop {
            return connector.with_fatal(fatal.clone());
        }
        Ok(connector)
    }

    pub(crate) async fn outbound(
        &self,
        binding: &Binding,
        endpoint: &str,
        deadline: Instant,
        rooms: &Arc<Semaphore>,
        grant: Grant,
        local_closed: Arc<AtomicBool>,
    ) -> Result<ReadyEnrollment> {
        if !self.client_reapers.available() {
            return Err(UNAVAILABLE);
        }
        let permit = rooms.clone().try_acquire_owned().map_err(|_| CAPACITY)?;
        let mut bootstrap = trace(
            "terminal_outbound_open",
            self.open(
                binding,
                endpoint,
                deadline,
                grant.clone(),
                Some(local_closed),
            )
            .await,
        )?;
        let flight = async {
            let remaining = deadline.saturating_duration_since(Instant::now());
            bootstrap.constrain_client_start(EnrollmentCeiling::start(
                remaining.min(Duration::from_secs(30)),
            )?)?;
            trace("terminal_outbound_probe", bootstrap.start_client().await)?;
            trace("terminal_outbound_confirm", bootstrap.finish_client().await)
        }
        .await;
        if let Err(error) = flight {
            return Err(finish_failed_enrollment(
                error,
                bootstrap.close().await,
                None,
                "terminal_outbound_close",
            ));
        }
        let room = trace(
            "terminal_outbound_enroll",
            bootstrap
                .enroll(
                    self.connector(endpoint)?,
                    RoomPermit::new(permit),
                    Some(grant.clone()),
                )
                .await,
        )?;
        complete(room, binding, grant).await
    }

    pub(crate) async fn inbound(
        &self,
        binding: &Binding,
        endpoint: &str,
        deadline: Instant,
        state: &mut InboundState,
        rooms: &Arc<Semaphore>,
        grant: Grant,
    ) -> Result<ReadyEnrollment> {
        let bootstrap = trace(
            "terminal_inbound_open",
            self.open(binding, endpoint, deadline, grant.clone(), None)
                .await,
        )?;
        let (bootstrap, permit) = trace(
            "terminal_inbound_confirm",
            bootstrap.serve(state, rooms, grant.clone()).await,
        )?;
        let room = trace(
            "terminal_inbound_enroll",
            bootstrap
                .enroll(self.connector(endpoint)?, permit, None)
                .await,
        )?;
        complete(room, binding, grant).await
    }
}

async fn complete(
    mut room: enrollment::GrantedEnrollment,
    binding: &Binding,
    grant: Grant,
) -> Result<ReadyEnrollment> {
    let owner = room.owner_mut();
    let disjoint = |group: &[u8; 32], route: &[u8; 32]| {
        [
            binding.context.mls_group(),
            binding.context.transport_group(),
        ]
        .into_iter()
        .all(|existing| existing != group && existing != route)
    };
    let established = async {
        match binding.context.direction() {
            CommandDirection::Client => {
                trace("terminal_outbound_offer", owner.offer().await)?;
                trace("terminal_outbound_join", owner.join(disjoint).await)?;
                trace("terminal_outbound_receipts", owner.client_receipts().await)
            }
            CommandDirection::Server => {
                trace("terminal_inbound_found", owner.found(disjoint).await)?;
                trace("terminal_inbound_receipts", owner.server_receipts().await)
            }
        }
    }
    .await;
    if let Err(error) = established {
        return Err(finish_failed_enrollment(
            error,
            room.close().await,
            None,
            "terminal_established_close",
        ));
    }
    room.ready(&|| grant() && binding.fence.state() == FenceState::Live)
        .await
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64"))]
#[path = "terminal_enrollment_fatal_tests.rs"]
mod physical_stop_tests;
