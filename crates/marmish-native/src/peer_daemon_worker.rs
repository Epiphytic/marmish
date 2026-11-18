//! Private daemon worker for isolated Pending and Activating Peer custody.

use std::{
    fs, io,
    path::Path,
    sync::atomic::AtomicU64,
    thread::{self, JoinHandle},
};

use nostr::key::PublicKey;

use crate::{
    command_config::Role,
    ipc::peer::{
        self, Entry, Failure as IpcFailure, Reply, Request, Selection, State, Work, Worker,
    },
    mdk_adapter::terminal_relay_fixture::PeerMode,
};

use super::{
    prepared_consent::{CustodyProgress, History, PreparedConsentOwner},
    store::consent_checkpoint::{
        Mode,
        provisioning::prepared_consent::custody::daemon::{Due, Inventory, Rotation},
    },
    *,
};

const LIVE_POLL: Duration = Duration::from_millis(250);
const RELAY_TURN: Duration = Duration::from_secs(3);
const RECONNECT_DELAY: Duration = Duration::from_secs(1);
const STOP_REPORT: Duration = Duration::from_secs(15);
const STOPPED: crate::Error = crate::Error::Identity("owned_peer_stop_incomplete");

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admission {
    Off,
    Manual,
    Automatic,
}

pub(crate) struct Bootstrap {
    qualified: QualifiedOrigin,
    recipient: PublicKey,
    founder: PublicKey,
    admission: Admission,
    wall: Wall,
    uncertain_approval: bool,
}

impl Bootstrap {
    pub(crate) fn for_fixture(
        fixture: PeerMode<'_>,
        private: &Path,
        recipient: PublicKey,
        founder: PublicKey,
        admission: Admission,
        fixture_wall: Option<Arc<AtomicU64>>,
    ) -> std::result::Result<Self, IpcFailure> {
        let qualified =
            QualifiedOrigin::for_daemon_peer_fixture(fixture, private, recipient, founder)
                .map_err(|_| IpcFailure::Forbidden)?;
        Ok(Self {
            qualified,
            recipient,
            founder,
            admission,
            wall: fixture_wall.map_or(Wall::System, Wall::Fixture),
            uncertain_approval: false,
        })
    }

    pub(crate) fn with_uncertain_approval_once(mut self) -> Self {
        self.uncertain_approval = true;
        self
    }

    pub(crate) fn eligible(&self, role: Role) -> bool {
        matches!(role, Role::Client | Role::Both) && self.admission != Admission::Off
    }

    fn open<'a>(
        self,
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        role: Role,
    ) -> Result<Service<'a>> {
        if !self.eligible(role) || material.public_key() != self.recipient {
            return Err(Error::Binding);
        }
        let mode = match self.admission {
            Admission::Off => return Err(Error::Binding),
            Admission::Manual => Mode::Manual,
            Admission::Automatic => Mode::Automatic,
        };
        let route = &self.qualified.route;
        // Initialize only a previously absent namespace under the sole lease.
        // Any incomplete or malformed existing namespace uses checked attach.
        match fs::symlink_metadata(lease.root().join("peer")) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Paths::new(lease, material)?.initialize()?;
                let _ = InvitationState::open(
                    lease,
                    material,
                    Policy::default(),
                    route,
                    Wall::System,
                    Checkpoint::attach_for_provisional_proof,
                )?;
                PreparedConsentOwner::open(
                    lease,
                    material,
                    role,
                    Policy::default(),
                    route,
                    Wall::System,
                )?;
            }
            Err(_) => return Err(Error::Path),
            Ok(_) => {}
        }
        let owner = PreparedConsentOwner::open_custody(
            lease,
            material,
            role,
            Policy {
                mode,
                ..Policy::default()
            },
            route,
            self.wall,
        )?;
        Ok(Service {
            owner,
            connector: self.qualified.connector,
            founder: self.founder,
            socket: None,
            reconnect: Instant::now(),
            recovery: true,
            poll_next: false,
            retry_next: true,
            rotation: Rotation::default(),
            uncertain_approval: self.uncertain_approval,
        })
    }
}

pub(crate) struct Runtime {
    stop: peer::Control,
    thread: JoinHandle<crate::Result<()>>,
}

impl Runtime {
    pub(crate) fn start(
        lease: Arc<ProfileLease>,
        material: IdentityMaterial,
        role: Role,
        bootstrap: Bootstrap,
        worker: Worker,
    ) -> crate::Result<Self> {
        let stop = worker.control();
        let thread = thread::Builder::new()
            .name("marmish-peer-custody".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                runtime.block_on(async {
                    if !worker.ready().await {
                        return Ok(());
                    }
                    match bootstrap.open(&lease, &material, role) {
                        Ok(service) => service.run(&lease, worker).await,
                        Err(_) => {
                            worker.close();
                            crate::diagnostics::event("peer_owner", "startup_unavailable");
                        }
                    }
                    Ok(())
                })
            })?;
        Ok(Self { stop, thread })
    }

    pub(crate) async fn join(self) -> crate::Result<()> {
        self.stop.stop();
        let monotonic = Instant::now();
        let continuous = crate::command_limits::continuous_time().ok();
        let mut incomplete = false;
        while !self.thread.is_finished() {
            let elapsed = continuous
                .zip(crate::command_limits::continuous_time().ok())
                .and_then(|(start, now)| now.checked_sub(start))
                .unwrap_or_else(|| monotonic.elapsed());
            if !incomplete && elapsed >= STOP_REPORT {
                crate::diagnostics::event("peer_owner", "owned_stop_incomplete");
                incomplete = true;
            }
            tokio::time::sleep(LIVE_POLL).await;
        }
        let outcome = self.thread.join().map_err(|_| STOPPED)?;
        if incomplete { Err(STOPPED) } else { outcome }
    }
}

struct Service<'a> {
    owner: PreparedConsentOwner<'a>,
    connector: RelayConnector,
    founder: PublicKey,
    socket: Option<Owner>,
    reconnect: Instant,
    recovery: bool,
    poll_next: bool,
    retry_next: bool,
    rotation: Rotation,
    uncertain_approval: bool,
}

impl Service<'_> {
    async fn run(mut self, lease: &ProfileLease, mut worker: Worker) {
        while worker.registered() && !worker.stopped() {
            if let Some(work) = worker.try_recv() {
                self.handle(lease, &worker, work);
            }
            if worker.stopped() {
                break;
            }
            let due = self.local_turn(&worker).await;
            // Keep a started native turn alive until it returns or leaves the
            // durable fence. Relay futures are cancellable as ownership loss.
            let result = tokio::select! {
                biased;
                () = worker.stopping() => break,
                result = self.relay_turn(due) => result,
            };
            if result.is_err() {
                self.socket = None;
                self.reconnect = Instant::now() + RECONNECT_DELAY;
                self.recovery = true;
                self.poll_next = false;
            }
        }
        worker.close();
    }

    async fn local_turn(&mut self, worker: &Worker) -> Option<Selector> {
        let retry = self.retry_next;
        self.retry_next = !self.retry_next;
        if retry && self.socket.is_some() {
            match self.owner.retry_custody_due() {
                Ok(None) => {}
                Ok(Some(_)) | Err(_) => return None,
            }
        }
        let due = self.next_due().ok().flatten()?;
        if worker.stopped() {
            return None;
        }
        let result = match due {
            Due::Publish(key) => return Some(key),
            Due::Provision(key) => self.prepare(key, false).await,
            Due::Resume(key) => self.prepare(key, true).await,
            Due::Join(key, generation) => self
                .owner
                .advance_custody(key, generation, |_| Ok(()))
                .await
                .map(|_: CustodyProgress| ()),
            Due::Automatic(key, generation) => self.automatic(worker, key, generation),
        };
        if result.is_err() {
            crate::diagnostics::event("peer_owner", "custody_turn_unavailable");
        }
        None
    }

    fn next_due(&mut self) -> Result<Option<Due>> {
        let wall = self.owner.available_sample()?.as_secs();
        Ok(self.owner.state.checkpoint.daemon_due(
            &self.owner.state.source,
            wall,
            &mut self.rotation,
            self.socket.is_some(),
            self.founder,
        )?)
    }

    async fn prepare(&mut self, key: Selector, resume: bool) -> Result<()> {
        let wall = self.owner.available_sample()?.as_secs();
        let checkpoint = &mut self.owner.state.checkpoint;
        let source = &self.owner.state.source;
        let reservation = if resume {
            checkpoint.recover_provisioning(source, key, wall)?
        } else {
            checkpoint.reserve_provisioning(source, key, wall)?
        };
        self.owner
            .state
            .prepare_provisional(&reservation, |_| Ok(()))
            .await?;
        self.owner
            .queue_prepared_consent(&reservation, |_| Ok(()))?;
        Ok(())
    }

    async fn relay_turn(&mut self, publish: Option<Selector>) -> Result<()> {
        let Some(socket) = &mut self.socket else {
            let now = Instant::now();
            if now < self.reconnect {
                tokio::time::sleep_until(self.reconnect.min(now + LIVE_POLL)).await;
                return Ok(());
            }
            self.owner.available_sample()?;
            let state = &self.owner.state;
            let socket = Owner::open(
                &self.connector,
                &state.origin_url,
                state.signer.public_key(),
                &state.signer,
                now + RELAY_TURN,
            )
            .await?;
            self.owner.available_sample()?;
            self.socket = Some(socket);
            return Ok(());
        };
        let deadline = Instant::now() + RELAY_TURN;
        if let Some(key) = publish {
            self.owner
                .publish_consent_wss(socket, key, deadline, || Ok(()))
                .await?;
            return Ok(());
        }
        if !self.poll_next {
            let start = if self.recovery {
                ScanStart::Recovery
            } else {
                ScanStart::Periodic
            };
            match self.owner.scan_wss(socket, start, deadline).await? {
                History::NoWork => self.recovery = false,
                History::Committed => {
                    self.recovery = false;
                    self.poll_next = true;
                    return Ok(());
                }
                History::Held(_) => return Err(Error::Binding),
            }
        }
        self.poll_next = false;
        if let Some(record) = socket.wait_live(Instant::now() + LIVE_POLL).await? {
            self.owner.receive_wss(socket, &record)?;
        }
        Ok(())
    }
}

#[path = "peer_daemon_decision.rs"]
mod decision;
