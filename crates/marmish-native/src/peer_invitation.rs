//! Client receipt and publication on a qualified private Peer origin.

#[cfg(test)]
#[path = "peer_accept_receipt_verify.rs"]
mod accept_receipt_verify;
#[cfg(test)]
#[path = "peer_accept_receipt_wire.rs"]
mod accept_receipt_wire;

#[cfg(test)]
#[path = "peer_daemon_worker.rs"]
pub(crate) mod daemon_peer;
#[cfg(test)]
#[path = "peer_daemon_wss_tests.rs"]
mod daemon_wss_tests;
#[path = "peer_invitation_paths.rs"]
mod paths;
#[cfg(test)]
#[path = "peer_postjoin_gate.rs"]
mod postjoin_gate;
#[cfg(test)]
#[path = "peer_postjoin_verify.rs"]
mod postjoin_verify;
#[cfg(test)]
#[path = "peer_prepared_consent_owner.rs"]
pub(super) mod prepared_consent;
#[cfg(test)]
pub(crate) use prepared_consent::leave_owner::{LeaveSnapshot, TurnFence};
#[cfg(test)]
#[path = "peer_private_wss_tests.rs"]
mod private_wss_tests;
#[cfg(test)]
#[path = "peer_stage_binding.rs"]
mod provisional;
#[cfg(test)]
#[path = "peer_private_activation_publication.rs"]
mod publication_socket;
#[cfg(test)]
#[path = "peer_receipt_publication_verify.rs"]
mod receipt_publication_verify;
#[cfg(test)]
#[path = "peer_stage_retirement.rs"]
mod retirement;
#[cfg(test)]
pub(crate) use store::consent_checkpoint::provisioning::prepared_consent::custody::HeldEvidence;
#[path = "peer_raw_socket.rs"]
mod socket;
#[path = "peer_cursor_store.rs"]
mod store;
#[cfg(test)]
#[path = "peer_invitation_tests.rs"]
mod tests;

use crate::{
    identity::{IdentityMaterial, ProfileLease},
    mdk_adapter::peer_native_signer::{NativePackage, OpenStage, StageError},
    transport::RelayConnector,
};
use hkdf::Hkdf;
use nostr::key::Keys;
use paths::{Paths, StageSlot};
use sha2::Sha256;
use socket::{
    owner::Owner,
    wire::{Arrival, Failure},
};
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use store::{
    CursorStore, HardenedPeerConnection, Origin, Page, RawRecord, ScanStart,
    consent_checkpoint::{
        self, Checkpoint, Local, Material, Policy, Route, Selector, Source, Status, Work,
    },
};
use tokio::{net::UnixStream, time::Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Error {
    Path,
    #[cfg(test)]
    StageIdentityMismatch,
    Clock,
    Binding,
    Cursor(store::Error),
    Checkpoint(consent_checkpoint::Error),
    Socket(Failure),
    Native(StageError),
}

impl From<store::Error> for Error {
    fn from(error: store::Error) -> Self {
        Self::Cursor(error)
    }
}
impl From<consent_checkpoint::Error> for Error {
    fn from(error: consent_checkpoint::Error) -> Self {
        Self::Checkpoint(error)
    }
}
impl From<Failure> for Error {
    fn from(error: Failure) -> Self {
        Self::Socket(error)
    }
}
impl From<StageError> for Error {
    fn from(error: StageError) -> Self {
        Self::Native(error)
    }
}

type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
enum CustodyDecision {
    Manual(Local),
    Automatic,
}

struct QualifiedOrigin {
    connector: RelayConnector,
    route: Route,
}

impl QualifiedOrigin {
    #[cfg(test)]
    fn for_fixture(relay: &socket::fixture::Relay, private: &std::path::Path) -> Result<Self> {
        Ok(Self {
            connector: relay.connector(private),
            route: Route::configured(relay.url.as_str())?,
        })
    }

    #[cfg(test)]
    fn for_daemon_peer_fixture(
        mode: crate::mdk_adapter::terminal_relay_fixture::PeerMode<'_>,
        private: &std::path::Path,
        recipient: nostr::key::PublicKey,
        founder: nostr::key::PublicKey,
    ) -> Result<Self> {
        let (origin, connector) = mode
            .qualify(private, recipient, founder)
            .ok_or(Error::Binding)?;
        Ok(Self {
            connector,
            route: Route::configured(origin)?,
        })
    }

    #[cfg(test)]
    fn for_shared_peer_fixture(
        relay: &crate::mdk_adapter::terminal_relay_fixture::Relay,
        private: &std::path::Path,
    ) -> Result<Self> {
        Ok(Self {
            connector: relay.connector(private, true, None),
            route: Route::configured(relay.url.as_str())?,
        })
    }
}

enum Wall {
    System,
    #[cfg(test)]
    Fixture(Arc<std::sync::atomic::AtomicU64>),
}

impl Wall {
    fn sample(&self) -> Result<Duration> {
        match self {
            Self::System => SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|_| Error::Clock),
            #[cfg(test)]
            Self::Fixture(wall) => Ok(Duration::from_secs(
                wall.load(std::sync::atomic::Ordering::Relaxed),
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PublishResult {
    Pending,
    Throttled,
    Accepted,
    Rejected,
}

struct InvitationState<'a> {
    #[cfg(test)]
    origin_url: String,
    paths: Paths<'a>,
    signer: Arc<Keys>,
    origin: Origin,
    checkpoint: Checkpoint,
    source: Source,
    wall: Wall,
}

impl<'a> InvitationState<'a> {
    fn open(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        policy: Policy,
        route: &Route,
        wall: Wall,
        open_checkpoint: impl FnOnce(
            CursorStore,
            Keys,
            u32,
            &[&str],
            Policy,
        ) -> consent_checkpoint::Result<Checkpoint>,
    ) -> Result<(Self, paths::CheckedFile)> {
        let paths = Paths::new(lease, material)?;
        let checked = paths.cursor()?;
        let conn = HardenedPeerConnection::open(checked.path(), *paths.cursor_key())?;
        checked.unchanged()?;
        let signer = material.signer();
        let account = signer.public_key();
        let origin = route.id;
        let url = &route.urls[0];
        let cursor = CursorStore::attach(conn, account.to_bytes(), &[origin])?;
        // SAFETY: reads this Unix process's effective UID.
        let owner_uid = unsafe { libc::geteuid() };
        let mut checkpoint = open_checkpoint(cursor, (*signer).clone(), owner_uid, &[url], policy)?;
        let source = checkpoint.bind_source(url)?;
        checkpoint
            .cursor
            .require_available(origin, wall.sample()?.as_secs())?;
        Ok((
            Self {
                #[cfg(test)]
                origin_url: url.clone(),
                paths,
                signer,
                origin,
                checkpoint,
                source,
                wall,
            },
            checked,
        ))
    }
}

fn verified_local(checkpoint: &Checkpoint, connection: &UnixStream) -> Result<Local> {
    let creds = connection.peer_cred().map_err(|_| Error::Binding)?;
    Ok(checkpoint.authorize_local(creds.uid())?)
}

struct InvitationLane<'a> {
    paths: Paths<'a>,
    signer: Arc<Keys>,
    origin: Origin,
    checkpoint: Checkpoint,
    source: Source,
    socket: Owner,
    recovery: bool,
    wall: Wall,
}

impl<'a> InvitationLane<'a> {
    fn initialize(lease: &ProfileLease, material: &IdentityMaterial) -> Result<()> {
        Paths::new(lease, material)?.initialize()
    }

    fn allocate(lease: &ProfileLease, material: &IdentityMaterial, slot: StageSlot) -> Result<()> {
        Paths::new(lease, material)?.allocate(slot)
    }

    async fn open(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        policy: Policy,
        qualified: QualifiedOrigin,
        wall: Wall,
        deadline: Instant,
    ) -> Result<Self> {
        let (mut state, checked) = InvitationState::open(
            lease,
            material,
            policy,
            &qualified.route,
            wall,
            Checkpoint::attach,
        )?;
        let url = &qualified.route.urls[0];
        let account = state.signer.public_key();
        let socket =
            Owner::open(&qualified.connector, url, account, &state.signer, deadline).await?;
        checked.unchanged()?;
        state
            .checkpoint
            .cursor
            .require_available(state.origin, state.wall.sample()?.as_secs())?;
        Ok(Self {
            paths: state.paths,
            signer: state.signer,
            origin: state.origin,
            checkpoint: state.checkpoint,
            source: state.source,
            socket,
            recovery: true,
            wall: state.wall,
        })
    }

    fn eligible_wall(&mut self) -> Result<u64> {
        Ok(self.eligible_sample()?.as_secs())
    }

    fn eligible_sample(&mut self) -> Result<Duration> {
        let sample = self.wall.sample()?;
        self.checkpoint
            .cursor
            .require_available(self.origin, sample.as_secs())?;
        Ok(sample)
    }

    async fn history_step(&mut self, deadline: Instant) -> Result<bool> {
        let wall = self.eligible_wall()?;
        let start = if self.recovery {
            ScanStart::Recovery
        } else {
            ScanStart::Periodic
        };
        let Some(cursor) = self.checkpoint.cursor.scan(self.origin, start, wall)? else {
            return Ok(false);
        };
        self.recovery = false;
        let page = self
            .socket
            .page([cursor.lower, cursor.until], deadline)
            .await?;
        if !self.socket.accepts_page(&page) {
            return Err(Error::Binding);
        }
        let wall = self.eligible_wall()?;
        let records: Vec<_> = page.records().iter().map(record).collect();
        let receipts = records
            .iter()
            .map(|raw| {
                self.checkpoint
                    .admit(&self.source, *raw, wall)
                    .map(|disposition| disposition.receipt)
            })
            .collect::<consent_checkpoint::Result<Vec<_>>>()?;
        let (_, [since, until]) = page.request();
        self.checkpoint.cursor.commit_page(
            &cursor,
            &Page {
                origin: self.origin,
                since,
                until,
                records: &records,
            },
            &receipts,
            wall,
        )?;
        Ok(true)
    }

    async fn live_step(&mut self, deadline: Instant) -> Result<Option<Status>> {
        self.eligible_wall()?;
        let Some(live) = self.socket.wait_live(deadline).await? else {
            return Ok(None);
        };
        if !self.socket.accepts_live(&live) {
            return Err(Error::Binding);
        }
        let wall = self.eligible_wall()?;
        let disposition = self
            .checkpoint
            .admit(&self.source, record(live.arrival()), wall)?;
        Ok(Some(disposition.status))
    }

    fn deferred_step(&mut self) -> Result<Option<Status>> {
        let wall = self.eligible_wall()?;
        let Some(snapshot) = self.checkpoint.cursor.due(self.origin, wall)? else {
            return Ok(None);
        };
        let status = self
            .checkpoint
            .admit(&self.source, snapshot.record(), wall)?
            .status;
        if status == Status::Deferred {
            self.checkpoint.cursor.retry_again(&snapshot, wall)?;
        }
        Ok(Some(status))
    }

    fn approve(&mut self, connection: &UnixStream, key: Selector) -> Result<()> {
        let local = verified_local(&self.checkpoint, connection)?;
        let wall = self.eligible_wall()?;
        Ok(self.checkpoint.approve(&local, key, wall)?)
    }

    fn discard(&mut self, connection: &UnixStream, key: Selector) -> Result<()> {
        let local = verified_local(&self.checkpoint, connection)?;
        let wall = self.eligible_wall()?;
        Ok(self.checkpoint.discard(&local, key, wall)?)
    }

    fn summaries(&mut self, connection: &UnixStream) -> Result<Vec<(Selector, bool)>> {
        let local = verified_local(&self.checkpoint, connection)?;
        let wall = self.eligible_wall()?;
        Ok(self.checkpoint.summaries(&local, &self.source, wall)?)
    }

    async fn publish_approved(
        &mut self,
        key: Selector,
        slot: StageSlot,
        deadline: Instant,
    ) -> Result<PublishResult> {
        let sample = self.eligible_sample()?;
        let wall = sample.as_secs();
        let new = match self.checkpoint.approved_work(key, wall, None)? {
            Work::Pending => return Ok(PublishResult::Pending),
            Work::NeedPackage { origin } if origin == self.origin => true,
            Work::Queued(outbox) if outbox.origin == self.origin => {
                if outbox.acknowledged_at.is_some()
                    || outbox.retry_at.is_some_and(|retry| wall < retry)
                {
                    return Ok(PublishResult::Throttled);
                }
                false
            }
            _ => return Err(Error::Binding),
        };
        self.socket.ready_to_publish(deadline)?;
        let package = self.stage_package(key, slot, sample, new).await?;
        let wall = self.eligible_wall()?;
        if new {
            let material = Material {
                origin: self.origin,
                selector: key,
                package: &package.bytes,
                expires: package
                    .expires
                    .min(wall.checked_add(86_400).ok_or(Error::Clock)?),
            };
            if !matches!(
                self.checkpoint.approved_work(key, wall, Some(material))?,
                Work::Queued(_)
            ) {
                return Err(Error::Binding);
            }
        }

        self.socket.ready_to_publish(deadline)?;
        let Some(attempt) =
            self.checkpoint
                .reserve_attempt(&self.source, key, package.id, &package.bytes, wall)?
        else {
            return Ok(PublishResult::Throttled);
        };
        let carrier = attempt.carrier();
        let receipt = self
            .socket
            .publish_bytes(attempt.bytes(), key.server, carrier, deadline)
            .await?;
        if !self.socket.accepted(&receipt, carrier) {
            return Ok(PublishResult::Rejected);
        }
        let wall = self.eligible_wall()?;
        self.checkpoint
            .record_acceptance(&self.source, attempt, wall)?;
        Ok(PublishResult::Accepted)
    }

    async fn stage_package(
        &self,
        key: Selector,
        slot: StageSlot,
        sample: Duration,
        new: bool,
    ) -> Result<NativePackage> {
        let path = self.paths.stage(slot)?;
        let db_key = self.paths.stage_key(slot);
        let slot_id = slot_id(&db_key, self.origin, key);
        let stage = OpenStage::open(
            path.path(),
            *db_key,
            self.signer.clone(),
            self.socket.origin(),
            slot_id,
        )?;
        path.unchanged()?;
        let package = stage.package(sample, new).await?;
        path.unchanged()?;
        Ok(package)
    }
}

fn slot_id(key: &[u8; 32], origin: Origin, selector: Selector) -> String {
    let mut slot = [0; 16];
    Hkdf::<Sha256>::new(None, key)
        .expand_multi_info(
            &[
                b"marmish/peer/native-slot/1",
                &origin,
                &selector.server.to_bytes(),
                &selector.id,
                &selector.revision,
            ],
            &mut slot,
        )
        .expect("fixed HKDF output length is valid");
    hex::encode(slot)
}

fn record(arrival: &Arrival) -> RawRecord<'_> {
    RawRecord {
        id: arrival.id,
        stamp: arrival.stamp,
        bytes: arrival.body.get().as_bytes(),
    }
}
