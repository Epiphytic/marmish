//! Leased staged-Consent fixture. No connector or production route.

use crate::{
    command_config::Role,
    mdk_adapter::peer_native_signer::{JoinWitness, JoinedAuthority},
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};
use storage_sqlite::SqlCipherKey;
use tempfile::TempDir;

use super::*;
use consent_checkpoint::provisioning::{
    Binding, Reservation,
    stage_journal::{Artifact, FileWitness},
};
pub(crate) use consent_checkpoint::wire::Delivery;
use snapshot_image::{read_image, snapshot_inode, snapshot_sidecars};

#[path = "peer_capped_join.rs"]
pub(in crate::peer_invitation) mod capped_join;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "peer_installed_consent_fixture.rs"]
pub(in crate::peer_invitation) mod installed_consent;
#[path = "peer_pending_join_native_tests.rs"]
mod joined_tests;
#[path = "peer_terminal_leave_owner.rs"]
pub(crate) mod leave_owner;
#[path = "peer_postjoin_proof_tests.rs"]
mod postjoin_tests;
#[path = "peer_pending_terminal_retirement.rs"]
pub(in crate::peer_invitation) mod terminal_retirement;
#[path = "peer_prepared_consent_owner_tests.rs"]
mod tests;

struct SnapshotArea(TempDir);

enum SnapshotCopy {
    Unjoined,
    Joined,
}

impl SnapshotArea {
    fn new() -> Result<Self> {
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .map(Self)
            .map_err(|_| Error::Path)
    }

    fn read(
        &self,
        state: &InvitationState<'_>,
        binding: Binding,
        witness: FileWitness,
        sample: Duration,
        before_open: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<(NativePackage, [u8; 32])> {
        let original = state.paths.reopen_provisional(binding, witness)?;
        self.with_snapshot(&original, SnapshotCopy::Unjoined, before_open, |path| {
            let native = open_existing_stage(state, binding, path)?;
            native.require_unjoined_provisional()?;
            Ok(native.read_provisional(sample)?.event)
        })
    }

    fn read_joined(
        &self,
        state: &InvitationState<'_>,
        stage: (Binding, FileWitness),
        delivery: &Delivery,
        package_digest: [u8; 32],
    ) -> Result<JoinedReadback> {
        let (binding, file) = stage;
        let original = state.paths.reopen_provisional(binding, file)?;
        let witness = JoinWitness::new(delivery, &state.origin_url, package_digest)?;
        let (authority, _) = self.with_snapshot(
            &original,
            SnapshotCopy::Joined,
            |_| Ok(()),
            |path| Ok(open_existing_stage(state, binding, path)?.read_joined(&witness)?),
        )?;
        Ok(JoinedReadback {
            stage,
            carrier: delivery.flight().carrier_id.to_bytes(),
            authority,
        })
    }

    fn with_snapshot<T>(
        &self,
        original: &paths::ProvisionalFile,
        mode: SnapshotCopy,
        before_open: impl FnOnce(&Path) -> Result<()>,
        validate: impl FnOnce(&Path) -> Result<T>,
    ) -> Result<(T, [u8; 32])> {
        let digest = original.digest()?;
        let path = self.0.path().join("image.db");
        snapshot_sidecars(&path)?;
        let mut copy = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_| Error::Path)?;
        let result = (|| {
            let inode = snapshot_inode(&copy.metadata().map_err(|_| Error::Path)?)?;
            let origin = original.witness();
            if inode
                == (
                    u64::from_be_bytes(origin.device),
                    u64::from_be_bytes(origin.inode),
                )
            {
                return Err(Error::StageIdentityMismatch);
            }
            let copied = io::copy(
                &mut original.read_file()?.take(paths::MAX_STAGE_LEN + 1),
                &mut copy,
            )
            .map_err(|_| Error::Path)?;
            drop(copy);
            if copied == 0
                || copied > paths::MAX_STAGE_LEN
                || read_image(&path)? != (digest, inode)
                || original.digest()? != digest
            {
                return Err(Error::StageIdentityMismatch);
            }
            before_open(&path)?;
            if read_image(&path)? != (digest, inode) {
                return Err(Error::StageIdentityMismatch);
            }
            let result = validate(&path);
            let (copy_digest, copy_inode) = read_image(&path)?;
            // Preserve the native error when a rejected unjoined copy changed
            // during an attempted migration. Successful Unjoined attestation
            // still requires byte-for-byte equality.
            if copy_inode != inode
                || (result.is_ok()
                    && matches!(mode, SnapshotCopy::Unjoined)
                    && copy_digest != digest)
                || original.digest()? != digest
            {
                return Err(Error::StageIdentityMismatch);
            }
            result.map(|value| (value, digest))
        })();
        // Leave an unresolved name or sidecar obstructing the next owner turn.
        // TempDir owns cleanup of this fixture on final teardown.
        fs::remove_file(&path).map_err(|_| Error::Path)?;
        snapshot_sidecars(&path)?;
        result
    }
}

fn open_existing_stage(
    state: &InvitationState<'_>,
    binding: Binding,
    path: &Path,
) -> Result<OpenStage> {
    Ok(OpenStage::open_provisional(
        path,
        &SqlCipherKey::raw(*state.paths.provisional_key(binding)),
        state.signer.clone(),
        &state.origin_url,
        state.paths.provisional_label(binding),
        false,
    )?)
}

/// Ephemeral authority bound to one exact stage and verified Delivery.
pub(crate) struct JoinedReadback {
    stage: (Binding, FileWitness),
    carrier: [u8; 32],
    authority: JoinedAuthority,
}

impl JoinedReadback {
    pub(in crate::peer_invitation) fn stage(&self) -> (Binding, FileWitness) {
        self.stage
    }

    pub(in crate::peer_invitation) fn carrier(&self) -> [u8; 32] {
        self.carrier
    }

    pub(in crate::peer_invitation) fn coordinates(&self) -> (&[u8], u64, [u8; 32], u32) {
        self.authority.coordinates()
    }
}

enum Epoch {
    Prepared,
    Custody,
    Publication,
    LeaveEffect,
}

pub(super) struct PreparedConsentOwner<'a> {
    pub(super) state: InvitationState<'a>,
    role: Role,
    custody: bool,
    snapshot: SnapshotArea,
}

impl<'a> PreparedConsentOwner<'a> {
    /// The custody caller must consume its committed JoinStarted guard first.
    pub(in crate::peer_invitation) async fn join_custody_stage(
        &self,
        stage: (Binding, FileWitness),
        delivery: &Delivery,
        package_digest: [u8; 32],
        sample: Duration,
    ) -> Result<()> {
        let original = self.state.paths.reopen_provisional(stage.0, stage.1)?;
        let witness = JoinWitness::new(delivery, &self.state.origin_url, package_digest)?;
        let (_, prepared_image) = self.snapshot.with_snapshot(
            &original,
            SnapshotCopy::Unjoined,
            |_| Ok(()),
            |path| {
                Ok(open_existing_stage(&self.state, stage.0, path)?
                    .require_prepared_join(&witness, sample)?)
            },
        )?;
        if original.digest()? != prepared_image {
            return Err(Error::StageIdentityMismatch);
        }
        let native = open_existing_stage(&self.state, stage.0, original.path())?;
        let result = native.join_once(&witness, sample).await;
        original.digest()?;
        Ok(result?)
    }

    pub(in crate::peer_invitation) fn inspect_joined(
        &self,
        stage: (Binding, FileWitness),
        delivery: &Delivery,
        package_digest: [u8; 32],
    ) -> Result<JoinedReadback> {
        self.snapshot
            .read_joined(&self.state, stage, delivery, package_digest)
    }

    pub(super) fn open(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        role: Role,
        policy: Policy,
        route: &Route,
        wall: Wall,
    ) -> Result<Self> {
        Self::open_with_epoch(lease, material, role, policy, route, wall, Epoch::Prepared)
    }

    pub(super) fn open_custody(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        role: Role,
        policy: Policy,
        route: &Route,
        wall: Wall,
    ) -> Result<Self> {
        Self::open_with_epoch(lease, material, role, policy, route, wall, Epoch::Custody)
    }

    pub(super) fn open_publication(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        role: Role,
        policy: Policy,
        route: &Route,
        wall: Wall,
    ) -> Result<Self> {
        Self::open_with_epoch(
            lease,
            material,
            role,
            policy,
            route,
            wall,
            Epoch::Publication,
        )
    }

    fn open_with_epoch(
        lease: &'a ProfileLease,
        material: &IdentityMaterial,
        role: Role,
        policy: Policy,
        route: &Route,
        wall: Wall,
        epoch: Epoch,
    ) -> Result<Self> {
        if role == Role::Server {
            return Err(Error::Binding);
        }
        let paths = Paths::new(lease, material)?;
        let checked = paths.cursor()?;
        let conn = HardenedPeerConnection::open(checked.path(), *paths.cursor_key())?;
        checked.unchanged()?;
        let signer = material.signer();
        let url = &route.urls[0];
        // SAFETY: reads the Unix effective UID.
        let uid = unsafe { libc::geteuid() };
        let keys = (*signer).clone();
        let checkpoint = match epoch {
            Epoch::Prepared => {
                Checkpoint::attach_prepared_consent_fixture(conn, keys, uid, &[url], policy)?
            }
            Epoch::Custody => {
                Checkpoint::attach_pending_custody_fixture(conn, keys, uid, &[url], policy)?
            }
            Epoch::Publication => {
                Checkpoint::attach_publication_fixture(conn, keys, uid, url, policy)?
            }
            Epoch::LeaveEffect => {
                Checkpoint::attach_leave_effect_fixture(conn, keys, uid, url, policy)?
            }
        };
        let source = checkpoint.bind_source(url)?;
        checked.unchanged()?;
        Ok(Self {
            state: InvitationState {
                paths,
                signer,
                origin: route.id,
                checkpoint,
                source,
                wall,
                origin_url: url.clone(),
            },
            role,
            custody: !matches!(epoch, Epoch::Prepared),
            snapshot: SnapshotArea::new()?,
        })
    }

    pub(in crate::peer_invitation) fn sample(&self) -> Result<Duration> {
        if matches!(self.role, Role::Client | Role::Both) {
            self.state.provisional_sample()
        } else {
            Err(Error::Binding)
        }
    }
}

pub(crate) struct NativeAttestation {
    binding: Binding,
    witness: FileWitness,
    package: NativePackage,
    image: [u8; 32],
}

impl NativeAttestation {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    pub(super) fn fixture_managed(
        (binding, witness): (Binding, FileWitness),
        package: NativePackage,
        image: [u8; 32],
    ) -> Self {
        Self {
            binding,
            witness,
            package,
            image,
        }
    }

    pub(super) fn require(
        &self,
        stage: (Binding, FileWitness),
        artifact: Artifact,
        wall: u64,
    ) -> consent_checkpoint::Result<&[u8]> {
        let found = Artifact::new(self.package.id, &self.package.bytes, self.package.expires);
        if stage != (self.binding, self.witness) || artifact != found || wall >= found.expires {
            return Err(consent_checkpoint::Error::Package);
        }
        Ok(&self.package.bytes)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Cut {
    BeforeSign,
    AfterSign,
    BeforeQueueCommit,
    Queued,
}

impl PreparedConsentOwner<'_> {
    fn attest(
        &self,
        (binding, witness): (Binding, FileWitness),
        sample: Duration,
    ) -> Result<NativeAttestation> {
        if self.state.checkpoint.is_installed_custody() {
            return Err(Error::Binding);
        }
        let (package, image) = self
            .snapshot
            .read(&self.state, binding, witness, sample, |_| Ok(()))?;
        Ok(NativeAttestation {
            binding,
            witness,
            package,
            image,
        })
    }

    fn verify_custody(&self, proof: &NativeAttestation) -> Result<()> {
        let file = self
            .state
            .paths
            .reopen_provisional(proof.binding, proof.witness)?;
        if file.digest()? != proof.image {
            return Err(Error::StageIdentityMismatch);
        }
        Ok(())
    }

    pub(super) fn queue_prepared_consent(
        &mut self,
        reservation: &Reservation,
        mut after: impl FnMut(Cut) -> Result<()>,
    ) -> Result<consent_checkpoint::wire::Transcript> {
        self.state
            .checkpoint
            .require_reservation_owner(reservation)?;
        let (_, _, _, selector) = reservation.stage_binding().coordinates();
        let wall = self.sample()?.as_secs();
        if self.custody
            && self
                .state
                .checkpoint
                .custody_owned(&self.state.source, selector, wall)?
        {
            return Err(consent_checkpoint::Error::Custodied.into());
        }
        if let Some(transcript) = self.recover_queued_consent(selector)? {
            return Ok(transcript);
        }
        let sample = self.sample()?;
        let candidate = self.state.checkpoint.inspect_prepared_consent(
            &self.state.source,
            reservation,
            sample.as_secs(),
        )?;
        let proof = self.attest(candidate.stage(), sample)?;
        after(Cut::BeforeSign)?;
        let signed = self.state.checkpoint.sign_prepared_consent(
            &candidate,
            &proof,
            self.sample()?.as_secs(),
        )?;
        after(Cut::AfterSign)?;
        self.verify_custody(&proof)?;
        let wall = self.sample()?.as_secs();
        let queued = self.state.checkpoint.queue_prepared_consent(
            &self.state.source,
            reservation,
            signed,
            wall,
            |_| {
                after(Cut::BeforeQueueCommit)
                    .map_err(|_| consent_checkpoint::Error::Cursor(store::Error::Storage))
            },
        )?;
        after(Cut::Queued)?;
        self.verify_custody(&proof)?;
        Ok(self
            .state
            .checkpoint
            .verify_queued_consent(&queued, &proof, wall)?)
    }

    fn recover_queued_consent(
        &mut self,
        selector: Selector,
    ) -> Result<Option<consent_checkpoint::wire::Transcript>> {
        let sample = self.sample()?;
        let Some(queued) = self.state.checkpoint.recover_queued_consent(
            &self.state.source,
            selector,
            sample.as_secs(),
        )?
        else {
            return Ok(None);
        };
        let proof = self.attest(queued.stage(), sample)?;
        let transcript = self.state.checkpoint.verify_queued_consent(
            &queued,
            &proof,
            self.sample()?.as_secs(),
        )?;
        self.verify_custody(&proof)?;
        Ok(Some(transcript))
    }
}

use consent_checkpoint::provisioning::prepared_consent::Attempt;

#[derive(Clone, Copy)]
enum SinkResponse {
    Positive,
    Negative,
    WrongOrigin,
    WrongCarrier,
    WrongSequence,
}

struct FixtureSink {
    owner: Arc<()>,
    origin: Origin,
    ready: bool,
    response: SinkResponse,
    last: Option<Vec<u8>>,
}

struct Receipt {
    owner: Arc<()>,
    origin: Origin,
    carrier: [u8; 32],
    sequence: u64,
}

impl FixtureSink {
    fn ready_for(&self, origin: Origin) -> bool {
        self.ready && self.origin == origin
    }

    fn publish(&mut self, attempt: &Attempt) -> Result<Option<Receipt>> {
        if !self.ready_for(attempt.origin()) {
            return Err(Error::Binding);
        }
        self.last = Some(attempt.bytes().to_vec());
        let mut receipt = Receipt {
            owner: self.owner.clone(),
            origin: self.origin,
            carrier: attempt.carrier(),
            sequence: attempt.sequence(),
        };
        match self.response {
            SinkResponse::Positive => {}
            SinkResponse::Negative => return Ok(None),
            SinkResponse::WrongOrigin => receipt.origin[0] ^= 1,
            SinkResponse::WrongCarrier => receipt.carrier[0] ^= 1,
            SinkResponse::WrongSequence => receipt.sequence ^= 1,
        }
        Ok(Some(receipt))
    }

    fn owns(&self, receipt: &Receipt, attempt: &Attempt) -> bool {
        Arc::ptr_eq(&self.owner, &receipt.owner)
            && self.ready_for(receipt.origin)
            && receipt.origin == attempt.origin()
            && receipt.carrier == attempt.carrier()
            && receipt.sequence == attempt.sequence()
    }
}

struct Reserved {
    attempt: Attempt,
    native: NativeAttestation,
    sink: Arc<()>,
}

struct Sent {
    reserved: Reserved,
    receipt: Receipt,
}

pub(crate) struct MatchingAck(Attempt);

impl MatchingAck {
    pub(super) fn into_attempt(self) -> Attempt {
        self.0
    }
}

impl PreparedConsentOwner<'_> {
    fn reserve_attempt(
        &mut self,
        selector: Selector,
    ) -> Result<Option<(Attempt, NativeAttestation)>> {
        let sample = self.sample()?;
        let Some(queued) = self.state.checkpoint.recover_queued_consent(
            &self.state.source,
            selector,
            sample.as_secs(),
        )?
        else {
            return Ok(None);
        };
        let native = self.attest(queued.stage(), sample)?;
        let wall = self.sample()?.as_secs();
        self.verify_custody(&native)?;
        let attempt = self.state.checkpoint.reserve_prepared_attempt(
            &self.state.source,
            queued,
            &native,
            wall,
        )?;
        self.verify_custody(&native)?;
        Ok(attempt.map(|attempt| (attempt, native)))
    }

    fn reserve_prepared_attempt(
        &mut self,
        selector: Selector,
        sink: &FixtureSink,
    ) -> Result<Option<Reserved>> {
        if !sink.ready_for(self.state.origin) {
            return Ok(None);
        }
        Ok(self
            .reserve_attempt(selector)?
            .map(|(attempt, native)| Reserved {
                attempt,
                native,
                sink: sink.owner.clone(),
            }))
    }

    fn publish_prepared_attempt(
        &mut self,
        reserved: Reserved,
        sink: &mut FixtureSink,
    ) -> Result<Option<Sent>> {
        if !Arc::ptr_eq(&reserved.sink, &sink.owner) {
            return Err(Error::Binding);
        }
        let wall = self.sample()?.as_secs();
        self.verify_custody(&reserved.native)?;
        self.state
            .checkpoint
            .check_current_attempt(&self.state.source, &reserved.attempt, wall)?;
        let result = sink.publish(&reserved.attempt)?;
        self.verify_custody(&reserved.native)?;
        Ok(result.map(|receipt| Sent { reserved, receipt }))
    }

    fn settle_prepared_ack(&mut self, sent: Sent, sink: &FixtureSink) -> Result<()> {
        let Sent { reserved, receipt } = sent;
        if !Arc::ptr_eq(&reserved.sink, &sink.owner) || !sink.owns(&receipt, &reserved.attempt) {
            return Err(Error::Binding);
        }
        let wall = self.sample()?.as_secs();
        self.verify_custody(&reserved.native)?;
        self.state.checkpoint.settle_prepared_ack(
            &self.state.source,
            MatchingAck(reserved.attempt),
            wall,
        )?;
        self.verify_custody(&reserved.native)?;
        Ok(())
    }
}

#[path = "peer_prepared_consent_wss.rs"]
mod wss;
pub(super) use wss::{CustodyProgress, History, JoinCut, Received, VerifiedDelivery};
